"""Deterministic Bolid Bluetooth Mesh access-model state and RF graph."""
from __future__ import annotations

import math
from collections import deque
from dataclasses import dataclass, field
from typing import Any, Iterable, Optional

SCHEMA = "firmverse.bolid-mesh/v1"
LEASE_TIMEOUT_MS = 10_000
PREPARE_TIMEOUT_MS = 3_000
MODE_TIMEOUT_MS = 300_000

MODES = {
    "NORMAL": 0,
    "OPEN_T": 1,
    "OPEN_MAIN": 2,
    "SHORT_1": 3,
    "SHORT_2": 4,
    "SHORT_T": 5,
}
MODE_NAMES = {value: name for name, value in MODES.items()}

OPCODES = {
    "LEASE_OPEN": 0x01,
    "LEASE_RENEW": 0x02,
    "LEASE_CLOSE": 0x03,
    "MODE_PREPARE": 0x10,
    "MODE_COMMIT": 0x11,
    "MODE_ABORT": 0x12,
    "STATE_GET": 0x20,
}
OPCODE_NAMES = {value: name for name, value in OPCODES.items()}

STATUS_OK = "OK"
STATUS_BAD_MESSAGE = "BAD_MESSAGE"
STATUS_UNAUTHENTICATED = "UNAUTHENTICATED"
STATUS_WRONG_GATEWAY = "WRONG_GATEWAY"
STATUS_LEASE_EXPIRED = "LEASE_EXPIRED"
STATUS_REPLAY = "REPLAY"
STATUS_SAFETY_REJECTED = "SAFETY_REJECTED"
STATUS_PREPARE_MISMATCH = "PREPARE_MISMATCH"
STATUS_APPLY_FAILED = "APPLY_FAILED"
STATUS_BUSY = "BUSY"
STATUS_TTL_EXHAUSTED = "TTL_EXHAUSTED"
STATUS_WRONG_DESTINATION = "WRONG_DESTINATION"


class ScenarioError(RuntimeError):
    pass


@dataclass(frozen=True)
class Message:
    opcode: int
    sequence: int
    transaction_id: int = 0
    lease_id: int = 0
    payload: tuple[int, ...] = ()


@dataclass
class Reply:
    status: str
    acknowledged_opcode: int
    detail: Optional[str] = None
    duplicate: bool = False


@dataclass
class Node:
    node_id: str
    address: int
    x: float
    y: float
    relay: bool
    device: bool
    present: bool = True
    mode: int = MODES["NORMAL"]
    physical_mode: int = MODES["NORMAL"]
    lease_active: bool = False
    lease_id: int = 0
    last_lease_ms: int = 0
    prepared_transaction_id: int = 0
    prepared_mode: int = MODES["NORMAL"]
    prepare_deadline_ms: int = 0
    active_transaction_id: int = 0
    mode_deadline_ms: int = 0
    highest_sequence: int = 0
    last_request: Optional[Message] = None
    last_reply: Optional[Reply] = None
    measurements_ready: bool = True
    reserve_low: bool = False
    real_short: bool = False
    fail_apply: bool = False

    def lease_valid(self, lease_id: int, now_ms: int, lease_timeout_ms: int) -> bool:
        return (
            self.lease_active
            and lease_id != 0
            and lease_id == self.lease_id
            and now_ms < self.last_lease_ms + lease_timeout_ms
        )

    def force_normal(self) -> None:
        self.mode = MODES["NORMAL"]
        self.physical_mode = MODES["NORMAL"]
        self.active_transaction_id = 0

    def safety_reason(self, requested_mode: int, now_ms: int, lease_timeout_ms: int) -> Optional[str]:
        if requested_mode == MODES["NORMAL"]:
            return None
        if not self.measurements_ready:
            return "MEASUREMENT_LOST"
        if self.real_short:
            return "REAL_SHORT"
        if self.reserve_low:
            return "LOW_RESERVE"
        if not self.lease_active or now_ms >= self.last_lease_ms + lease_timeout_ms:
            return "SESSION_TIMEOUT"
        return None

    def receive(
        self,
        message: Message,
        *,
        source: int,
        destination: int,
        gateway_address: int,
        ttl: int,
        authenticated: bool,
        now_ms: int,
        lease_timeout_ms: int,
        prepare_timeout_ms: int,
        mode_timeout_ms: int,
    ) -> Reply:
        if not authenticated:
            return Reply(STATUS_UNAUTHENTICATED, message.opcode)
        if ttl <= 0:
            return Reply(STATUS_TTL_EXHAUSTED, message.opcode)
        if destination != self.address:
            return Reply(STATUS_WRONG_DESTINATION, message.opcode)
        if source != gateway_address:
            return Reply(STATUS_WRONG_GATEWAY, message.opcode)
        if message.sequence <= 0:
            return Reply(STATUS_BAD_MESSAGE, message.opcode)
        if self.last_request == message and self.last_reply is not None:
            return Reply(
                self.last_reply.status,
                self.last_reply.acknowledged_opcode,
                self.last_reply.detail,
                True,
            )
        if message.sequence <= self.highest_sequence:
            return Reply(STATUS_REPLAY, message.opcode)

        reply = self._process(
            message,
            now_ms=now_ms,
            lease_timeout_ms=lease_timeout_ms,
            prepare_timeout_ms=prepare_timeout_ms,
            mode_timeout_ms=mode_timeout_ms,
        )
        self.highest_sequence = message.sequence
        self.last_request = message
        self.last_reply = reply
        return reply

    def _process(
        self,
        message: Message,
        *,
        now_ms: int,
        lease_timeout_ms: int,
        prepare_timeout_ms: int,
        mode_timeout_ms: int,
    ) -> Reply:
        opcode = message.opcode
        if opcode == OPCODES["LEASE_OPEN"]:
            if message.lease_id == 0 or message.payload:
                return Reply(STATUS_BAD_MESSAGE, opcode)
            if (
                self.lease_active
                and self.lease_id != message.lease_id
                and now_ms < self.last_lease_ms + lease_timeout_ms
            ):
                return Reply(STATUS_BUSY, opcode)
            self.lease_id = message.lease_id
            self.lease_active = True
            self.last_lease_ms = now_ms
            return Reply(STATUS_OK, opcode)

        if opcode == OPCODES["LEASE_RENEW"]:
            if message.payload:
                return Reply(STATUS_BAD_MESSAGE, opcode)
            if not self.lease_valid(message.lease_id, now_ms, lease_timeout_ms):
                return Reply(STATUS_LEASE_EXPIRED, opcode)
            self.last_lease_ms = now_ms
            return Reply(STATUS_OK, opcode)

        if opcode == OPCODES["LEASE_CLOSE"]:
            if message.payload or not self.lease_valid(message.lease_id, now_ms, lease_timeout_ms):
                return Reply(STATUS_LEASE_EXPIRED, opcode)
            self.lease_active = False
            self.prepared_transaction_id = 0
            self.force_normal()
            return Reply(STATUS_OK, opcode)

        if opcode == OPCODES["MODE_PREPARE"]:
            if (
                len(message.payload) != 1
                or message.transaction_id == 0
                or message.payload[0] not in MODE_NAMES
            ):
                return Reply(STATUS_BAD_MESSAGE, opcode)
            requested_mode = message.payload[0]
            if requested_mode != MODES["NORMAL"] and not self.lease_valid(
                message.lease_id, now_ms, lease_timeout_ms
            ):
                return Reply(STATUS_LEASE_EXPIRED, opcode)
            if (
                self.prepared_transaction_id
                and self.prepared_transaction_id != message.transaction_id
                and now_ms < self.prepare_deadline_ms
            ):
                return Reply(STATUS_BUSY, opcode)
            reason = self.safety_reason(requested_mode, now_ms, lease_timeout_ms)
            if reason is not None:
                return Reply(STATUS_SAFETY_REJECTED, opcode, reason)
            self.prepared_transaction_id = message.transaction_id
            self.prepared_mode = requested_mode
            self.prepare_deadline_ms = now_ms + prepare_timeout_ms
            return Reply(STATUS_OK, opcode, MODE_NAMES[requested_mode])

        if opcode == OPCODES["MODE_COMMIT"]:
            if (
                message.payload
                or message.transaction_id == 0
                or message.transaction_id != self.prepared_transaction_id
                or now_ms >= self.prepare_deadline_ms
            ):
                self.prepared_transaction_id = 0
                return Reply(STATUS_PREPARE_MISMATCH, opcode)
            requested_mode = self.prepared_mode
            if requested_mode != MODES["NORMAL"] and not self.lease_valid(
                message.lease_id, now_ms, lease_timeout_ms
            ):
                self.prepared_transaction_id = 0
                return Reply(STATUS_LEASE_EXPIRED, opcode)
            reason = self.safety_reason(requested_mode, now_ms, lease_timeout_ms)
            if reason is not None:
                self.prepared_transaction_id = 0
                return Reply(STATUS_SAFETY_REJECTED, opcode, reason)
            if requested_mode == MODES["NORMAL"]:
                self.force_normal()
            elif self.fail_apply:
                self.force_normal()
                self.prepared_transaction_id = 0
                return Reply(STATUS_APPLY_FAILED, opcode)
            else:
                self.mode = requested_mode
                self.physical_mode = requested_mode
                self.active_transaction_id = message.transaction_id
                self.mode_deadline_ms = now_ms + mode_timeout_ms
            self.prepared_transaction_id = 0
            return Reply(STATUS_OK, opcode, MODE_NAMES[self.mode])

        if opcode == OPCODES["MODE_ABORT"]:
            if message.payload or message.transaction_id == 0:
                return Reply(STATUS_BAD_MESSAGE, opcode)
            matched = (
                self.prepared_transaction_id == message.transaction_id
                or self.active_transaction_id == message.transaction_id
            )
            if self.prepared_transaction_id == message.transaction_id:
                self.prepared_transaction_id = 0
            # Rollback is fail-safe: a node that prepared a group transaction
            # returns to NORMAL even when the new COMMIT never reached it.
            if matched:
                self.force_normal()
            return Reply(STATUS_OK, opcode)

        if opcode == OPCODES["STATE_GET"]:
            if message.payload:
                return Reply(STATUS_BAD_MESSAGE, opcode)
            return Reply(STATUS_OK, opcode, MODE_NAMES[self.mode])

        return Reply(STATUS_BAD_MESSAGE, opcode)

    def tick(self, now_ms: int, lease_timeout_ms: int) -> Optional[str]:
        if self.lease_active and now_ms >= self.last_lease_ms + lease_timeout_ms:
            self.lease_active = False
        if self.prepared_transaction_id and now_ms >= self.prepare_deadline_ms:
            self.prepared_transaction_id = 0
        if self.mode == MODES["NORMAL"]:
            return None
        if not self.measurements_ready:
            reason = "MEASUREMENT_LOST"
        elif self.real_short:
            reason = "REAL_SHORT"
        elif self.reserve_low:
            reason = "LOW_RESERVE"
        elif now_ms >= self.mode_deadline_ms:
            reason = "MODE_TIMEOUT"
        elif not self.lease_active:
            reason = "SESSION_TIMEOUT"
        else:
            return None
        self.force_normal()
        return reason


@dataclass
class DropRule:
    target: str
    opcode: int


@dataclass
class World:
    name: str
    gateway_id: str
    nodes: dict[str, Node]
    range_m: float
    default_ttl: int
    lease_timeout_ms: int
    prepare_timeout_ms: int
    mode_timeout_ms: int
    now_ms: int = 0
    sequence: int = 0
    drop_once: list[DropRule] = field(default_factory=list)
    last_sent: dict[str, Message] = field(default_factory=dict)
    assertions: int = 0
    trace: list[dict[str, Any]] = field(default_factory=list)

    @property
    def gateway(self) -> Node:
        return self.nodes[self.gateway_id]

    def emit(self, event: str, **payload: Any) -> None:
        self.trace.append({"event": event, "time_ms": self.now_ms, **payload})

    def tick_to(self, now_ms: int) -> None:
        if now_ms < self.now_ms:
            raise ScenarioError(f"event time moved backwards: {now_ms} < {self.now_ms}")
        self.now_ms = now_ms
        for node in self.nodes.values():
            if node.device:
                reason = node.tick(now_ms, self.lease_timeout_ms)
                if reason is not None:
                    self.emit("FAILSAFE", node=node.node_id, reason=reason, mode="NORMAL")

    def neighbors(self, node_id: str) -> list[str]:
        source = self.nodes[node_id]
        if not source.present:
            return []
        result = []
        for candidate_id, candidate in self.nodes.items():
            if candidate_id == node_id or not candidate.present:
                continue
            if math.hypot(source.x - candidate.x, source.y - candidate.y) <= self.range_m:
                result.append(candidate_id)
        return sorted(result)

    def route(self, source_id: str, target_id: str, ttl: Optional[int] = None) -> Optional[list[str]]:
        if source_id not in self.nodes or target_id not in self.nodes:
            return None
        if not self.nodes[source_id].present or not self.nodes[target_id].present:
            return None
        ttl_value = self.default_ttl if ttl is None else ttl
        queue: deque[list[str]] = deque([[source_id]])
        seen = {source_id}
        while queue:
            path = queue.popleft()
            current = path[-1]
            if current == target_id:
                return path
            if len(path) - 1 >= ttl_value:
                continue
            if current != source_id and not self.nodes[current].relay:
                continue
            for neighbor in self.neighbors(current):
                if neighbor not in seen:
                    seen.add(neighbor)
                    queue.append([*path, neighbor])
        return None

    def next_sequence(self) -> int:
        self.sequence += 1
        return self.sequence

    def should_drop(self, target: str, opcode: int) -> bool:
        for index, rule in enumerate(self.drop_once):
            if rule.target == target and rule.opcode == opcode:
                del self.drop_once[index]
                return True
        return False

    def send(
        self,
        target_id: str,
        message: Message,
        *,
        authenticated: bool = True,
        remember: bool = True,
    ) -> Optional[Reply]:
        if target_id not in self.nodes:
            raise ScenarioError(f"unknown target {target_id!r}")
        route = self.route(self.gateway_id, target_id)
        opcode_name = OPCODE_NAMES.get(message.opcode, f"0x{message.opcode:02X}")
        base = {
            "source": self.gateway_id,
            "target": target_id,
            "opcode": opcode_name,
            "sequence": message.sequence,
            "transaction_id": message.transaction_id,
        }
        if route is None:
            self.emit("TX", **base, delivered=False, reason="NO_ROUTE")
            return None
        if self.should_drop(target_id, message.opcode):
            self.emit("TX", **base, route=route, delivered=False, reason="DROP_ONCE")
            return None
        node = self.nodes[target_id]
        reply = node.receive(
            message,
            source=self.gateway.address,
            destination=node.address,
            gateway_address=self.gateway.address,
            ttl=self.default_ttl - (len(route) - 1) + 1,
            authenticated=authenticated,
            now_ms=self.now_ms,
            lease_timeout_ms=self.lease_timeout_ms,
            prepare_timeout_ms=self.prepare_timeout_ms,
            mode_timeout_ms=self.mode_timeout_ms,
        )
        reverse_route = self.route(target_id, self.gateway_id)
        delivered = reverse_route is not None
        self.emit(
            "TX",
            **base,
            route=route,
            response_route=reverse_route,
            delivered=delivered,
            status=reply.status if delivered else "RESPONSE_NO_ROUTE",
            duplicate=reply.duplicate,
            detail=reply.detail,
        )
        if remember:
            self.last_sent[target_id] = message
        return reply if delivered else None

    def lease(self, op: str, targets: Iterable[str], lease_id: int, expect: str) -> None:
        replies = [
            self.send(target, Message(OPCODES[op], self.next_sequence(), lease_id=lease_id))
            for target in targets
        ]
        ok_count = sum(reply is not None and reply.status == STATUS_OK for reply in replies)
        actual = "ok" if ok_count == len(replies) else "none" if ok_count == 0 else "partial"
        self.assert_equal(actual, expect, f"{op} outcome")

    def transaction(
        self,
        targets: list[str],
        transaction_id: int,
        mode: int,
        lease_id: int,
        expect: str,
    ) -> None:
        prepared: list[str] = []
        committed: list[str] = []
        for target in targets:
            reply = self.send(
                target,
                Message(
                    OPCODES["MODE_PREPARE"],
                    self.next_sequence(),
                    transaction_id,
                    lease_id,
                    (mode,),
                ),
            )
            if reply is None or reply.status != STATUS_OK:
                self._abort_and_report(targets, transaction_id, lease_id, mode, expect,
                                       "PREPARE", prepared, committed)
                return
            prepared.append(target)
        for target in targets:
            reply = self.send(
                target,
                Message(OPCODES["MODE_COMMIT"], self.next_sequence(), transaction_id, lease_id),
            )
            if reply is None or reply.status != STATUS_OK:
                self._abort_and_report(targets, transaction_id, lease_id, mode, expect,
                                       "COMMIT", prepared, committed)
                return
            committed.append(target)
        self.emit("TXN", transaction_id=transaction_id, mode=MODE_NAMES[mode],
                  state="COMPLETE", prepared=prepared, committed=committed)
        self.assert_equal("complete", expect, f"transaction {transaction_id} outcome")

    def _abort_and_report(
        self,
        targets: Iterable[str],
        transaction_id: int,
        lease_id: int,
        mode: int,
        expect: str,
        phase: str,
        prepared: list[str],
        committed: list[str],
    ) -> None:
        for target in targets:
            self.send(
                target,
                Message(OPCODES["MODE_ABORT"], self.next_sequence(), transaction_id, lease_id),
            )
        self.emit("TXN", transaction_id=transaction_id, mode=MODE_NAMES[mode],
                  state="ABORTED", phase=phase, prepared=prepared, committed=committed)
        self.assert_equal("abort", expect, f"transaction {transaction_id} outcome")

    def assert_equal(self, actual: Any, expected: Any, label: str) -> None:
        self.assertions += 1
        passed = actual == expected
        self.emit("ASSERT", label=label, passed=passed, expected=expected, actual=actual)
        if not passed:
            raise ScenarioError(f"{label}: expected {expected!r}, got {actual!r}")
