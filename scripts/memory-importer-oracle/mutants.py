"""PREPARED real-process input controls; unchanged production oracle binaries."""
import argparse
import copy
import json
import os
from pathlib import Path


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("input", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    args.destination.mkdir()  # Originals are never rewritten.
    packets = json.loads(args.input.read_text())
    for name, family, key, value in [
        ("wrong-source-owner", "codex-memory", "RootHex", None),
        ("shell-filter-corruption", "shell-history", "FiltersHex", [b"owned-unmatched-filter".hex()]),
        ("obsidian-tag-corruption", "obsidian", "TagsHex", [b"owned-unmatched-tag".hex()]),
    ]:
        changed = copy.deepcopy(packets)
        packet = next(row for row in changed if row["Family"]==family and row["Id"].endswith("-positive"))
        if value is None:
            value = (bytes.fromhex(packet[key])+b"/absent-owned-root").hex()
        packet[key] = value
        (args.destination/f"{name}.json").write_text(json.dumps(changed,indent=2)+"\n")


if __name__ == "__main__": main()
