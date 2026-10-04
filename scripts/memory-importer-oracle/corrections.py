"""Additional PREPARED constructor cases; the original 70 recipes stay exact."""
import argparse
import json
import os
from pathlib import Path
import fixtures


def additions():
    cases = [(family, variant) for family in ("codex-memory", "curated-memory", "obsidian")
             for variant in ("invalid-frontmatter", "literal-replacement", "js-frontmatter", "invalid-key")]
    cases += [(family, variant) for family in ("curated-memory", "obsidian")
              for variant in ("invalid-link", "literal-link")]
    cases += [("obsidian", variant) for variant in ("dot-root", "parent-root", "trailing-root", "raw-root")]
    cases += [("shell-history", variant) for variant in ("dot-command", "parent-command", "trailing-command")]
    return cases


def create(destination, family, variant, number):
    packet = fixtures.create_case(destination, family, "positive", 70+number)
    packet["Id"] = f"I761-C{number:02d}-{family}-{variant}"
    root = bytes.fromhex(packet["RootHex"])
    path = root
    if family == "codex-memory": path += b"/MEMORY.md"
    elif family == "curated-memory": path += b"/.claude/projects/owned/memory/note.md"
    elif family == "obsidian": path += b"/note.md"
    if family == "shell-history":
        command = {"dot-command": b"npm/. install", "parent-command": b"npm/.. install",
                   "trailing-command": b"npm/ install"}[variant]
        body = b": 1787911200:0;"+command+b"\n"
    else:
        prefix = {"invalid-frontmatter": b"---\nname: \xff\xfe\n---\n",
                  "literal-replacement": b"---\nname: \xef\xbf\xbd\n---\n",
                  "js-frontmatter": "---\nname: <>&\u2028\u2029\n---\n".encode(),
                  "invalid-key": b"---\n\xff: owned\n---\n"}.get(variant, b"")
        body = prefix+b"Owned context preserves a documented decision without any credential or provider operation.\n"
        if variant == "invalid-link": body += b"[[\xff\xfe]]"
        if variant == "literal-link": body += b"[[\xef\xbf\xbd]]"
    with open(path, "wb") as handle: handle.write(body)
    os.utime(path, (fixtures.STAMP, fixtures.STAMP))
    if variant in ("dot-root", "parent-root", "trailing-root"):
        suffix = {"dot-root": b"/.", "parent-root": b"/..", "trailing-root": b"///"}[variant]
        packet["RootHex"] = (root+suffix).hex()
    if variant == "raw-root":
        renamed = root+b"-\xff"
        os.rename(root, renamed)
        packet["RootHex"] = renamed.hex()
    return packet


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    destination = args.destination.resolve()
    destination.mkdir(mode=0o700)
    original = [fixtures.create_case(destination, family, variant, index+1)
                for index, (family, variant) in enumerate(fixtures.recipes())]
    assert len(original) == 70
    expanded = [create(destination, family, variant, index+1)
                for index, (family, variant) in enumerate(additions())]
    assert len(expanded) == 23
    (destination/"input.json").write_text(json.dumps(original+expanded, indent=2)+"\n")
    (destination/"PREPARED.json").write_text(json.dumps({"status":"prepared-not-executed",
        "unchanged_original_cases":70,"additional_cases":23,"total":93},indent=2)+"\n")


if __name__ == "__main__": main()
