"""PREPARED owned fixture materializer; does not invoke Go, Rust or providers."""
import argparse
import json
import os
import shutil
from pathlib import Path

STAMP = 1787911200  # 2026-08-28 10:00:00Z; runtime peers must use TZ=UTC.
FAMILIES = ("codex-memory", "curated-memory", "aider", "shell-history", "obsidian")


def recipes():
    result = []
    for family in FAMILIES:
        for variant in ("positive", "empty", "malformed", "duplicate", "unicode"):
            result.append((family, variant))
    extra = {
        "codex-memory": ("old-six-hour-parent", "cross-extension-coverage", "deny-wins",
                         "allow-no-match", "upper-extension", "nested-resources",
                         "raw-body", "one-mib-cut", "instructions", "outside-root",
                         "duplicate-frontmatter", "citations", "lone-surrogate-app"),
        "curated-memory": ("outside-memory-dir", "index-skip", "raw-body", "five-k-cut",
                           "link-order", "scanner-limit", "delimiter-suffix"),
        "aider": ("threshold-50", "threshold-51", "inline-role", "scanner-ignored",
                  "headings", "raw-body", "crlf"),
        "shell-history": ("bash-not-paired", "excluded", "filter", "empty-filter",
                          "duplicate-epoch", "zero-duration", "scanner-prefix", "raw-command"),
        "obsidian": ("unused-folder", "exclude-folder", "tag-filter", "exclude-tag",
                     "raw-body", "five-k-cut", "links-not-trimmed", "scanner-limit"),
    }
    for family, variants in extra.items():
        result.extend((family, variant) for variant in variants)
    result.extend((family, "frozen-testdata") for family in ("codex-memory", "curated-memory"))
    return result


def create_case(destination, family, variant, number):
    case_id = f"I761-{number:03d}-{family}-{variant}"
    home = destination / case_id
    home.mkdir()
    root = home
    body = b"An owned fixture records a reproducible implementation decision without credentials.\n"
    if variant == "empty":
        body = b""
    if variant == "unicode":
        body = ("Entscheidung: Äpfel, 東京 und 🦀 bleiben im Quelldokument. " * 3).encode()
    if variant in ("raw-body", "raw-command"):
        body = b"invalid-byte-" + b"\xff\xfe" * 40 + b"\n"
    if variant == "five-k-cut":
        body = b"x" * 4999 + "東京".encode() + b" extra words"
    if variant == "one-mib-cut":
        body = b"x" * ((1 << 20) - 1) + "東京".encode()
    if family == "codex-memory":
        root = home / ".codex" / "memories"
        filename = b"MEMORY.md"
    elif family == "curated-memory":
        filename = b".claude/projects/owned/memory/note.md"
    elif family == "aider":
        filename = b"project/.aider.chat.history.md"
        body = b"**Assistant**:\n" + body
    elif family == "shell-history":
        root = home / ".zsh_history"
        filename = b".zsh_history"
        body = b": 1787911200:0;git status\n"
    else:
        root = home / "vault"
        filename = b"note.md"
    packet = {"Id": case_id, "Family": family, "RootHex": os.fsencode(root).hex(),
              "Since": "2026-08-28T10:00:00Z"}
    if variant == "frozen-testdata":
        origin = Path(__file__).resolve().parents[2] / "migration/evidence/memory-importers-761/source-preparation/original-testdata"
        folder = "codexmemory" if family == "codex-memory" else "curatedmemory"
        for old in sorted((origin/folder).rglob("*")):
            if not old.is_file(): continue
            relative = old.relative_to(origin/folder)
            if family == "curated-memory":
                first, *parts = relative.parts
                relative = Path(".claude" if first == "claude-code" else ".hermes", *parts)
            target = root / relative
            target.parent.mkdir(parents=True,exist_ok=True)
            shutil.copyfile(old,target)
            os.utime(target,(STAMP,STAMP))
        return packet
    records = {}
    if variant == "malformed":
        body = b"---\nname: unclosed\nnot a complete frontmatter document"
    if variant == "duplicate":
        body += body
    if family == "shell-history":
        changes = {
            "empty": b"", "malformed": b"not a timestamp\n", "unicode": ": 1787911200:0;git commit -m 東京\n".encode(),
            "duplicate": b": 1787911200:0;git status\n" * 2,
            "duplicate-epoch": b": 1787911200:0;git status\n: 1787911200:2;git diff\n",
            "bash-not-paired": b"#1787911200\ngit status\n",
            "excluded": b": 1787911200:2;ls -la\n: 1787911200:2;git status\n",
            "filter": b": 1787911200:2;git status\n: 1787911200:2;npm install\n",
            "scanner-prefix": b": 1787911200:0;git status\n" + b"z" * 65536,
            "raw-command": b": 1787911200:0;git status \xff\xfe\n",
        }
        body = changes.get(variant, body)
        if variant == "filter": packet["FiltersHex"] = [b"git".hex()]
        if variant == "empty-filter": packet["FiltersHex"] = [""]
    if variant == "outside-memory-dir": filename = b".claude/projects/owned/notes.md"
    if variant == "index-skip":
        filename = b".claude/projects/owned/MEMORY.md"
        records[b".claude/projects/owned/memory/notes.md"] = body
    if variant in ("link-order", "links-not-trimmed"): body += b"[[ b ]] [[a|label]] [[ b ]] [[  ]]"
    if variant == "delimiter-suffix": body = b"---\nname: owned\n---suffix\n" + body
    if variant == "duplicate-frontmatter": body = b"---\ntitle: first\ntitle: second\n---\n" + body
    if variant == "citations": body += b"## citations\nowned reference\n## next\nnot a citation"
    if variant == "scanner-limit": body = b"valid prefix\n" + b"x" * 65536
    if variant == "scanner-ignored": body = b"**Assistant**:\n" + b"x" * 51 + b"\n" + b"y" * 65536
    if variant.startswith("threshold-"): body = b"**Assistant**:\n" + b"x" * (int(variant[-2:])-1) + b"\n"
    if variant == "inline-role": body = b"**Assistant**: " + b"x" * 100 + b"\n"
    if variant == "headings": body += b"## reset\n**Assistant**:\n" + b"z" * 51 + b"\n"
    if variant == "crlf": body = body.replace(b"\n", b"\r\n")
    if variant in ("old-six-hour-parent", "cross-extension-coverage"):
        filename = b"extensions/second/resources/2026-08-28T10-05-00-id-10min-owned.md"
        parent = b"extensions/first/resources/2026-08-28T10-00-00-id-6h-owned.md"
        records[parent] = body
    if variant == "upper-extension": filename = b"rollout_summaries/upper.MD"
    if variant == "nested-resources": filename = b"extensions/owned/resources/deeper/note.md"
    if variant in ("deny-wins", "allow-no-match"):
        body = b"---\napplications: [\"owned.app\"]\n---\n" + body
        packet["AllowedHex"] = [(b"owned.app" if variant == "deny-wins" else b"other.app").hex()]
        if variant == "deny-wins": packet["DeniedHex"] = [b"owned.app".hex()]
    if variant == "lone-surrogate-app": body = b"---\napplications: [\"\\ud800\",null,\"owned\"]\n---\n" + body
    if variant == "instructions": filename = b"extensions/owned/instructions.md"
    if variant == "unused-folder": filename = b"different-folder/note.md"
    if variant == "exclude-folder": filename = b"Templates/note.md"
    if variant in ("tag-filter", "exclude-tag"):
        body += b" #owned"
        packet["TagsHex" if variant == "tag-filter" else "ExcludedTagsHex"] = [b"owned".hex()]
    records[filename] = body
    file_base = home if family in ("aider", "shell-history", "curated-memory") else root
    for relative, data in records.items():
        path = os.fsencode(file_base) + b"/" + relative
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "xb") as handle: handle.write(data)
        old = variant == "old-six-hour-parent" and b"-6h-" in relative
        os.utime(path, (STAMP-10 if old else STAMP, STAMP-10 if old else STAMP))
    if variant in ("instructions", "outside-root"):
        packet["Direct"] = True
        direct = os.fsencode(file_base) + b"/" + filename
        if variant == "outside-root":
            direct = os.fsencode(home) + b"/outside.md"
            with open(direct,"xb") as handle: handle.write(body)
            os.utime(direct,(STAMP,STAMP))
        packet["PathHex"] = direct.hex()
    return packet


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    destination = args.destination.resolve()
    destination.mkdir(mode=0o700)  # Refuse an existing root, including operator data.
    packets = [create_case(destination, family, variant, i+1) for i,(family,variant) in enumerate(recipes())]
    (destination/"input.json").write_text(json.dumps(packets,indent=2)+"\n")
    (destination/"PREPARED.json").write_text(json.dumps({"status":"prepared-not-executed", "cases":len(packets),"families":list(FAMILIES)},indent=2)+"\n")


if __name__ == "__main__": main()
