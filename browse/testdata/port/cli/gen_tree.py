#!/usr/bin/env python3
"""Enumerate the Go cobra tree of symbrowse recursively via --help.

Usage: gen_tree.py <go-binary> <go-source-sha> <out.json>
Every node records the raw `--help` bytes (base64-free, UTF-8 text), parsed
usage line, aliases, groups, sub-commands and flags. Run only against the real
Go oracle binary; never hand-edit the output.
"""
import base64, hashlib, json, os, platform, re, subprocess, sys, tempfile

binary, source_sha, out = sys.argv[1:4]
home = tempfile.mkdtemp(prefix="symbrowse-tree-")
env = {
    "HOME": home, "USERPROFILE": home, "XDG_CONFIG_HOME": home + "/c",
    "XDG_DATA_HOME": home + "/d", "XDG_CACHE_HOME": home + "/ca",
    "XDG_STATE_HOME": home + "/s", "TMPDIR": home, "LANG": "C", "LC_ALL": "C",
    "TZ": "UTC", "TERM": "dumb", "NO_COLOR": "1", "PATH": "/usr/bin:/bin",
}

def run(args):
    p = subprocess.run([binary, *args], env=env, cwd=home, capture_output=True, timeout=30)
    return p.returncode, p.stdout, p.stderr

def section(text, title):
    m = re.search(r"^" + re.escape(title) + r"\n((?:  .*\n|\n(?=  ))*)", text, re.M)
    return m.group(1) if m else ""

FLAG_RE = re.compile(r"^  (?:-(\w), )?(?:    )?--([\w-]+)(?: ([A-Za-z0-9]+))?(?:   +| ?$)(.*)$")

def parse_flags(block):
    flags = []
    for line in block.splitlines():
        if not line.strip():
            continue
        m = FLAG_RE.match(line)
        if not m:
            raise SystemExit("unparsed flag line: %r" % line)
        usage = m.group(4)
        default = None
        d = re.search(r" \(default (\"[^\"]*\"|-?[0-9.]+|true|false)\)$", usage)
        if d:
            default = d.group(1)
            usage = usage[: d.start()]
        flags.append({"short": m.group(1), "name": m.group(2), "type": m.group(3) or "bool",
                      "default": default, "usage": usage})
    return flags

nodes = []
def visit(path):
    rc, so, se = run([*path, "--help"])
    text = so.decode("utf-8")
    node = {
        "path": path, "help_exit": rc, "help_stderr_len": len(se),
        "help": text, "help_sha256": hashlib.sha256(so).hexdigest(),
    }
    usage = re.search(r"^Usage:\n  (.*)$", text, re.M)
    node["usage"] = usage.group(1) if usage else None
    al = re.search(r"^Aliases:\n  (.*)$", text, re.M)
    node["aliases"] = [a.strip() for a in al.group(1).split(",")][1:] if al else []
    node["flags"] = parse_flags(section(text, "Flags:"))
    node["global_flags"] = parse_flags(section(text, "Global Flags:"))
    children = []
    for hdr in re.finditer(r"^((?:[A-Z][A-Za-z ]*Commands|Available Commands):)\n((?:  .*\n)+)", text, re.M):
        for line in hdr.group(2).splitlines():
            m = re.match(r"^  (\S+)\s+(.*)$", line)
            if m:
                children.append({"name": m.group(1), "short": m.group(2), "group": hdr.group(1)})
    node["children"] = children
    nodes.append(node)
    for c in children:
        if c["name"] in ("help", "completion", "compat-sidecar") and path:
            pass
        visit(path + [c["name"]])
        if path == [] and c["name"] == "completion":
            pass

visit([])
doc = {
    "schema": 1,
    "provenance": {
        "go_source_sha": source_sha,
        "go_binary_sha256": hashlib.sha256(open(binary, "rb").read()).hexdigest(),
        "go_version": subprocess.run(["go", "version"], capture_output=True, text=True).stdout.strip(),
        "platform": platform.system().lower() + "/" + platform.machine(),
        "generator": "browse/testdata/port/cli/gen_tree.py",
        "env": "fresh HOME/XDG, LANG=C TZ=UTC TERM=dumb NO_COLOR=1",
    },
    "nodes": nodes,
}
with open(out, "w") as f:
    json.dump(doc, f, indent=1, ensure_ascii=False)
    f.write("\n")
print(len(nodes), "nodes")
