import json
import pathlib
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
import bolid_mesh as model  # noqa: E402


class BolidMeshModelTest(unittest.TestCase):
    def load_example(self):
        return json.loads((ROOT / "examples" / "bolid-mesh-v2.json").read_text())

    def test_full_regression_scenario(self):
        world = model.run_scenario(self.load_example())
        self.assertGreaterEqual(world.assertions, 15)
        self.assertEqual(world.nodes["d1"].mode, model.MODES["NORMAL"])
        self.assertEqual(world.nodes["d2"].mode, model.MODES["NORMAL"])
        self.assertTrue(any(row["event"] == "FAILSAFE" for row in world.trace))
        self.assertTrue(
            any(row["event"] == "TXN" and row["state"] == "ABORTED" for row in world.trace)
        )

    def test_non_relay_end_device_does_not_forward(self):
        data = self.load_example()
        data["nodes"][2]["relay"] = False
        world = model.load_world(data)
        self.assertIsNone(world.route("gw", "d2"))
        self.assertIsNotNone(world.route("gw", "d1"))

    def test_same_sequence_different_payload_is_replay(self):
        world = model.load_world(self.load_example())
        world.tick_to(0)
        node = world.nodes["d1"]
        first = model.Message(model.OPCODES["LEASE_OPEN"], 1, lease_id=7)
        reply = world.send("d1", first)
        self.assertEqual(reply.status, model.STATUS_OK)
        changed = model.Message(model.OPCODES["LEASE_OPEN"], 1, lease_id=8)
        reply = world.send("d1", changed, remember=False)
        self.assertEqual(reply.status, model.STATUS_REPLAY)
        self.assertEqual(node.lease_id, 7)


if __name__ == "__main__":
    unittest.main()
