"""Scenario loader and event runner for the Bolid Mesh model."""
from __future__ import annotations

from typing import Any

from .core import (
    LEASE_TIMEOUT_MS,
    MODE_NAMES,
    MODE_TIMEOUT_MS,
    MODES,
    OPCODES,
    PREPARE_TIMEOUT_MS,
    SCHEMA,
    STATUS_REPLAY,
    DropRule,
    Message,
    Node,
    ScenarioError,
    World,
)


def integer(value: Any, label: str) -> int:
    if isinstance(value, bool):
        raise ScenarioError(f"{label} must be an integer")
    if isinstance(value, int):
        return value
    if isinstance(value, str):
        try:
            return int(value, 0)
        except ValueError as exc:
            raise ScenarioError(f"{label} is not an integer: {value!r}") from exc
    raise ScenarioError(f"{label} must be an integer")


def mode(value: Any) -> int:
    if not isinstance(value, str) or value not in MODES:
        raise ScenarioError(f"unknown mode {value!r}; expected one of {sorted(MODES)}")
    return MODES[value]


def opcode(value: Any) -> int:
    if not isinstance(value, str) or value not in OPCODES:
        raise ScenarioError(f"unknown opcode {value!r}; expected one of {sorted(OPCODES)}")
    return OPCODES[value]


def load_world(data: dict[str, Any]) -> World:
    if data.get("schema") != SCHEMA:
        raise ScenarioError(f"scenario schema must be {SCHEMA!r}")
    name = str(data.get("name") or "unnamed")
    gateway_id = str(data.get("gateway") or "")
    node_data = data.get("nodes")
    if not isinstance(node_data, list) or len(node_data) < 2:
        raise ScenarioError("scenario needs at least two nodes")
    nodes: dict[str, Node] = {}
    addresses: set[int] = set()
    for raw in node_data:
        if not isinstance(raw, dict):
            raise ScenarioError("each node must be an object")
        node_id = str(raw.get("id") or "")
        address = integer(raw.get("address"), f"node {node_id} address")
        if not node_id or node_id in nodes:
            raise ScenarioError(f"duplicate/empty node id {node_id!r}")
        if address <= 0 or address > 0x7FFF or address in addresses:
            raise ScenarioError(f"invalid/duplicate unicast address {address:#x}")
        addresses.add(address)
        nodes[node_id] = Node(
            node_id=node_id,
            address=address,
            x=float(raw.get("x", 0.0)),
            y=float(raw.get("y", 0.0)),
            relay=bool(raw.get("relay", False)),
            device=bool(raw.get("device", node_id != gateway_id)),
        )
    if gateway_id not in nodes:
        raise ScenarioError(f"gateway {gateway_id!r} is not a node")
    return World(
        name=name,
        gateway_id=gateway_id,
        nodes=nodes,
        range_m=float(data.get("range_m", 5.0)),
        default_ttl=integer(data.get("default_ttl", 5), "default_ttl"),
        lease_timeout_ms=integer(data.get("lease_timeout_ms", LEASE_TIMEOUT_MS),
                                 "lease_timeout_ms"),
        prepare_timeout_ms=integer(data.get("prepare_timeout_ms", PREPARE_TIMEOUT_MS),
                                   "prepare_timeout_ms"),
        mode_timeout_ms=integer(data.get("mode_timeout_ms", MODE_TIMEOUT_MS),
                                "mode_timeout_ms"),
    )


def run_event(world: World, event: dict[str, Any]) -> None:
    op = event.get("op")
    if not isinstance(op, str):
        raise ScenarioError("event op must be a string")
    world.tick_to(integer(event.get("at_ms", world.now_ms), f"{op}.at_ms"))

    if op == "assert_route":
        source = str(event["from"])
        target = str(event["to"])
        route = world.route(source, target, integer(event.get("ttl", world.default_ttl), "ttl"))
        expected = bool(event.get("reachable", True))
        world.assert_equal(route is not None, expected, f"route {source}->{target} reachable")
        if route is not None and "hops" in event:
            world.assert_equal(len(route) - 1, integer(event["hops"], "hops"),
                               f"route {source}->{target} hops")
        world.emit("ROUTE", source=source, target=target, route=route)
        return

    if op == "partition":
        node_id = str(event["node"])
        if node_id not in world.nodes:
            raise ScenarioError(f"unknown node {node_id!r}")
        enabled = bool(event.get("enabled", True))
        world.nodes[node_id].present = not enabled
        world.emit("PARTITION", node=node_id, enabled=enabled)
        return

    if op == "drop_once":
        target = str(event["target"])
        world.drop_once.append(DropRule(target, opcode(event["opcode"])))
        world.emit("DROP_ARMED", target=target, opcode=str(event["opcode"]))
        return

    if op in {"lease_open", "lease_renew", "lease_close"}:
        targets = [str(value) for value in event.get("targets", [])]
        if not targets:
            raise ScenarioError(f"{op} needs targets")
        world.lease(op.upper(), targets, integer(event["lease_id"], "lease_id"),
                    str(event.get("expect", "ok")))
        return

    if op == "transaction":
        targets = [str(value) for value in event.get("targets", [])]
        if not targets:
            raise ScenarioError("transaction needs targets")
        world.transaction(targets, integer(event["transaction_id"], "transaction_id"),
                          mode(event["mode"]), integer(event["lease_id"], "lease_id"),
                          str(event.get("expect", "complete")))
        return

    if op == "duplicate_last":
        target = str(event["target"])
        message = world.last_sent.get(target)
        if message is None:
            raise ScenarioError(f"no last message for {target}")
        reply = world.send(target, message, remember=False)
        actual = "duplicate" if reply is not None and reply.duplicate else (
            reply.status if reply is not None else "missing"
        )
        world.assert_equal(actual, str(event.get("expect", "duplicate")), f"duplicate {target}")
        return

    if op == "replay":
        target = str(event["target"])
        node = world.nodes[target]
        request = Message(
            opcode(event.get("opcode", "MODE_ABORT")),
            integer(event.get("sequence", max(1, node.highest_sequence - 1)), "sequence"),
            integer(event.get("transaction_id", 1), "transaction_id"),
            integer(event.get("lease_id", node.lease_id), "lease_id"),
        )
        reply = world.send(target, request, remember=False)
        actual = reply.status if reply is not None else "missing"
        world.assert_equal(actual, str(event.get("expect", STATUS_REPLAY)), f"replay {target}")
        return

    if op == "set_input":
        node_id = str(event["node"])
        node = world.nodes[node_id]
        for key in ("measurements_ready", "reserve_low", "real_short"):
            if key in event:
                setattr(node, key, bool(event[key]))
        world.emit("INPUT", node=node_id, measurements_ready=node.measurements_ready,
                   reserve_low=node.reserve_low, real_short=node.real_short)
        reason = node.tick(world.now_ms, world.lease_timeout_ms)
        if reason is not None:
            world.emit("FAILSAFE", node=node_id, reason=reason, mode="NORMAL")
        return

    if op == "set_apply_failure":
        node_id = str(event["node"])
        world.nodes[node_id].fail_apply = bool(event.get("enabled", True))
        world.emit("APPLY_FAILURE", node=node_id, enabled=world.nodes[node_id].fail_apply)
        return

    if op == "assert_modes":
        expected = event.get("modes")
        if not isinstance(expected, dict):
            raise ScenarioError("assert_modes needs modes object")
        for node_id, mode_name in expected.items():
            world.assert_equal(MODE_NAMES[world.nodes[str(node_id)].mode], str(mode_name),
                               f"mode {node_id}")
        return

    if op == "assert_all_normal":
        targets = [str(value) for value in event.get("targets", [])]
        if not targets:
            targets = [node.node_id for node in world.nodes.values() if node.device]
        for target in targets:
            world.assert_equal(MODE_NAMES[world.nodes[target].mode], "NORMAL", f"mode {target}")
        return

    if op == "advance":
        world.emit("ADVANCE")
        return

    raise ScenarioError(f"unknown event op {op!r}")


def run_scenario(data: dict[str, Any]) -> World:
    world = load_world(data)
    world.emit("READY", schema=SCHEMA, scenario=world.name,
               gateway=world.gateway_id, nodes=len(world.nodes))
    events = data.get("events")
    if not isinstance(events, list):
        raise ScenarioError("scenario events must be a list")
    for raw in events:
        if not isinstance(raw, dict):
            raise ScenarioError("each event must be an object")
        run_event(world, raw)
    world.emit("SUMMARY", passed=True, scenario=world.name, assertions=world.assertions)
    return world
