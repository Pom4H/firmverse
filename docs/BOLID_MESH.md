# Bolid Bluetooth Mesh model

Firmverse has two complementary levels of testing for PHY6252 projects:

1. the instruction-level emulator boots a real HEX and checks the MCU/ROM/HAL path;
2. `tools/bolid_mesh_model.py` deterministically checks the multi-node Bolid
   Bluetooth Mesh **Access-model contract** before a vendor radio stack is bound.

The second level does not claim to emulate RF timing, Bluetooth Mesh encryption,
provisioning or the PHY6252 controller. Those belong to the vendor stack and the
instruction/hardware gates. It tests the product states that are easy to lose in
packet-level experiments:

- relay reachability and TTL;
- one authenticated gateway;
- strict source sequence and idempotent duplicate handling;
- a 10-second authenticated control lease;
- group `PREPARE -> COMMIT`, with `ABORT -> NORMAL` on any partial result;
- local return to `NORMAL` after lease loss, mode timeout, low reserve, real
  short or measurement loss.

## Run

```sh
python3 tools/bolid_mesh_model.py examples/bolid-mesh-v2.json \
  --trace tmp/bolid-mesh-v2.jsonl
```

The command writes deterministic JSON Lines and finishes with:

```text
BOLID_MESH_PASS scenario=... nodes=... assertions=...
```

A failed assertion or malformed scenario returns non-zero and prints
`BOLID_MESH_FAIL`.

## Scenario schema

The current schema identifier is `firmverse.bolid-mesh/v1`.

```json
{
  "schema": "firmverse.bolid-mesh/v1",
  "gateway": "gw",
  "range_m": 3.25,
  "default_ttl": 5,
  "nodes": [
    {"id": "gw", "address": "0x0001", "x": 0, "y": 0, "relay": true, "device": false},
    {"id": "d1", "address": "0x0101", "x": 3, "y": 0, "relay": false, "device": true}
  ],
  "events": [
    {"at_ms": 0, "op": "lease_open", "targets": ["d1"], "lease_id": "0xA11CE"},
    {"at_ms": 10, "op": "transaction", "targets": ["d1"], "transaction_id": 1,
     "lease_id": "0xA11CE", "mode": "SHORT_1", "expect": "complete"},
    {"at_ms": 10010, "op": "assert_all_normal", "targets": ["d1"]}
  ]
}
```

Supported event operations:

| Operation | Purpose |
| --- | --- |
| `assert_route` | Validate relay path, TTL and hop count |
| `partition` | Remove/restore one node from the RF graph |
| `drop_once` | Lose one selected Access opcode to a node |
| `lease_open`, `lease_renew`, `lease_close` | Control the authenticated lease |
| `transaction` | Execute group prepare/commit/abort |
| `duplicate_last` | Repeat the exact last request |
| `replay` | Inject an older source sequence |
| `set_input` | Change measurement/reserve/short facts |
| `set_apply_failure` | Inject output-actuator failure |
| `assert_modes`, `assert_all_normal` | Product-state assertions |
| `advance` | Advance time without another action |

## Composite GitHub Action

```yaml
- id: mesh
  uses: Pom4H/firmverse/actions/bolid-mesh@<pinned-commit>
  with:
    scenario: firmware/experiments/mesh_v2/firmverse/bolid-mesh-v2.json
    trace: tmp/bolid-mesh-v2.jsonl
```

Pin a commit while the model is experimental. The action exposes the trace path
as `${{ steps.mesh.outputs.trace }}`.

## Boundary with real Bluetooth Mesh

A passing model proves the Bolid access/session/rollback invariants for the
scenario. It does **not** prove:

- Bluetooth SIG qualification or interoperability;
- PHY6252 BLE Proxy + Mesh coexistence;
- actual relay timing, packet loss or segmentation behavior;
- current consumption on PB-03F or the target board;
- correct binding to a particular vendor Mesh SDK.

Those claims require the real vendor libraries, a real compiled image in the
instruction emulator and hardware acceptance measurements.
