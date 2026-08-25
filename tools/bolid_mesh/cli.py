"""Command-line entry point for the deterministic Bolid Mesh model."""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Optional, TextIO

from .core import ScenarioError, World
from .scenario import run_scenario


def write_trace(world: World, stream: TextIO) -> None:
    for record in world.trace:
        stream.write(json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n")


def parse_args(argv: Optional[list[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Deterministic Bluetooth Mesh access-model emulator for Bolid v2"
    )
    parser.add_argument("scenario", type=Path, help="firmverse.bolid-mesh/v1 JSON scenario")
    parser.add_argument("--trace", type=Path, help="also write JSONL trace to this file")
    return parser.parse_args(argv)


def main(argv: Optional[list[str]] = None) -> int:
    args = parse_args(argv)
    try:
        data = json.loads(args.scenario.read_text(encoding="utf-8"))
        if not isinstance(data, dict):
            raise ScenarioError("scenario root must be an object")
        world = run_scenario(data)
    except (OSError, json.JSONDecodeError, ScenarioError, KeyError, TypeError, ValueError) as exc:
        print(f"BOLID_MESH_FAIL {exc}", file=sys.stderr)
        return 2

    write_trace(world, sys.stdout)
    if args.trace is not None:
        args.trace.parent.mkdir(parents=True, exist_ok=True)
        with args.trace.open("w", encoding="utf-8") as stream:
            write_trace(world, stream)
    print(f"BOLID_MESH_PASS scenario={world.name} nodes={len(world.nodes)} "
          f"assertions={world.assertions}")
    return 0
