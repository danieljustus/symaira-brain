#!/usr/bin/env python3
"""Record or check Go `normalizeSyncTime` results for the native `symbrain_sync_time`.

The Go function (`internal/memory/db/sync_time.go`, #980) is extracted verbatim
from an immutable Git revision, compiled in a scratch module and run over the
vector list below. `--check` fails when the committed fixture differs from the
fresh Go output (no silent regeneration); the Rust test
`rust/symbrain-memory/tests/sync_time_contract.rs` replays every vector.
"""
import argparse
import json
from pathlib import Path
import re
import subprocess
import tempfile

REVISION = "fc5252d2e88c0ac2f8fe945113f9494084916558"
FIXTURE = Path("rust/symbrain-memory/tests/fixtures/sync_time_go.json")

VECTORS = [
    "2026-01-01T00:00:00.123Z", "2026-01-01 00:00:00.123 +0000 UTC",
    "2026-01-01 01:00:00.123 +0100 CET", "2026-01-01T01:00:00.123+01:00",
    "2026-01-01 00:00:00.123", "2026-01-01 00:00:00.123 +0000 UTC m=+1.234",
    "2026-01-01T00:00:00.100Z", "2026-01-01 00:00:00.100000001 +0000 UTC",
    "2026-01-01T01:00:00.100000001+01:00", "2026-01-01 00:00:00.100000002",
    "2026-01-01 00:00:00.2 +0000 UTC", "0001-01-01 00:00:00 +0000 UTC",
    "0000-01-01 00:00:00", "9999-12-31 23:59:59.999999999 +0000 UTC",
    "9999-12-31 23:59:59 -0100", "0000-01-01 00:00:00 +0100 +0100",
    "2026-10-26 03:02:03.456 +0200 +0200", "2026-10-26 01:02:03 +0000 UTC",
    "2026-10-26 01:02:03", "2026-10-26T01:02:03", "2026-10-26T01:02:03Z",
    "2026-10-26 01:02:03Z", "2026-10-26 01:02", "2026-10-26T01:02",
    "2026-10-26 01:02Z", "2026-10-26", "2026-10-26Z", "2026-10-26 01:02:03.5",
    "2026-10-26 01:02:03,5", "2026-10-26 01:02:03.123456789012",
    "2026-10-26 01:02:03.1234567890", "2026-10-26 01:02:03.", "2026-10-26 01:02:03.x",
    "2026-10-26 1:02:03", "2026-10-26 1:2:03", "2026-10-26 01:2:03", "2026-1-26 01:02:03",
    "2026-10-6 01:02:03", "26-10-26 01:02:03", "2026-02-29", "2024-02-29", "2026-04-31",
    "2026-13-01", "2026-00-10", "2026-10-00", "2026-10-26 24:00:00", "2026-10-26 23:60:00",
    "2026-10-26 23:59:60", "2026-10-26T24:00", "2026-10-26 01:02:03 +0100",
    "2026-10-26 01:02:03 -0000", "2026-10-26 01:02:03 +2400", "2026-10-26 01:02:03 +2500",
    "2026-10-26 01:02:03 +0160", "2026-10-26 01:02:03 +0161", "2026-10-26 01:02:03 0100",
    "2026-10-26T01:02:03+01:00", "2026-10-26T01:02:03-01:30", "2026-10-26T01:02:03+25:00",
    "2026-10-26T01:02:03+01:61", "2026-10-26T01:02:03+0100", "2026-10-26T01:02:03+01",
    "2026-10-26T01:02:03.5+01:00", "2026-10-26T01:02:03.5-00:00",
    "2026-10-26 01:02:03+01:00", "2026-10-26 01:02:03.5-07:00", "2026-10-26 01:02:03.5Z+01:00",
    "2026-10-26 01:02:03 +01:00", "2026-10-26 01:02:03z", "2026-10-26t01:02:03",
    " 2026-10-26 01:02:03 ", "\t2026-10-26 01:02:03\n", "2026-10-26  01:02:03",
    "2026-10-26 01:02:03 +0000  UTC", "2026-10-26 01:02:03 +0000 UTC m=+0.5",
    "2026-10-26 01:02:03 +0000 UTC m=", "2026-10-26 01:02:03 +0000 UTC\tm=+1",
    "2026-10-26 01:02:03 +0000 UTC extra", "2026-10-26 01:02:03 UTC",
    "2026-10-26 01:02:03.5 +0200 CEST", "2026-10-26 01:02:03 +0200 XYZ1",
    "", " ", "Z", "not a timestamp", "2026", "2026-10", "2026-10-26 01", "2026-10-26 01:",
    "2026-10-26 01:02:", "2026-10-26 01:02:3", "2026-10-26 01:02:03 +", "2026-10-26T",
    "+026-10-26", "2026-10-26T01:02:03.5Z", "2026-10-26T01:02:03.5ZZ", "2026-10-26 01:02:03.5 Z",
    "2026-10-26 01:02:03 +0000 UTC m=+1.234 m=+2", "2026-10-26 01:02:03.000000000Z",
    "2026-10-26 01:02:03.000000001Z", "2026-12-31 23:30:00 -0100 -0100",
    "2026-01-01 00:30:00 +0100 +0100", "2026-03-01T00:00:00+14:00", "2026-03-01T00:00:00-12:00",
    "٢٠٢٦-10-26", "2026-10-26 01:02:03 ＋0100",
]

GENERATOR = r'''package main

import (
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"time"
)

%FUNCTION%

type result struct {
	Input  string `json:"input"`
	Output string `json:"output,omitempty"`
	Error  bool   `json:"error"`
}

func main() {
	var vectors []string
	if err := json.NewDecoder(os.Stdin).Decode(&vectors); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(2)
	}
	out := make([]result, 0, len(vectors))
	for _, v := range vectors {
		got, err := normalizeSyncTime(v)
		out = append(out, result{Input: v, Output: got, Error: err != nil})
	}
	encoder := json.NewEncoder(os.Stdout)
	encoder.SetIndent("", "  ")
	_ = encoder.Encode(out)
}
'''


def run(args, cwd, stdin=None):
    return subprocess.run(args, cwd=cwd, input=stdin, check=True, capture_output=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--revision", default=REVISION)
    args = parser.parse_args()
    repo = Path(run(["git", "rev-parse", "--show-toplevel"], ".").stdout.decode().strip())
    source = run(["git", "show", f"{args.revision}:internal/memory/db/sync_time.go"], repo).stdout.decode()
    match = re.search(r"(?ms)^// normalizeSyncTime.*?^}\n", source)
    assert match, "normalizeSyncTime not found at the pinned revision"
    with tempfile.TemporaryDirectory(prefix="sync-time-oracle-") as scratch:
        root = Path(scratch)
        (root / "go.mod").write_text("module syncoracle\n\ngo 1.22\n")
        (root / "main.go").write_text(GENERATOR.replace("%FUNCTION%", match.group(0)))
        raw = run(["go", "run", "."], root, json.dumps(VECTORS).encode()).stdout.decode()
    records = json.loads(raw)
    assert [record["input"] for record in records] == VECTORS
    document = json.dumps(dict(go_oracle_revision=args.revision, vectors=records), indent=2,
                          ensure_ascii=True) + "\n"
    path = repo / FIXTURE
    if args.check:
        assert path.read_text() == document, "sync_time_go.json differs from the Go oracle"
        print(f"{len(records)} vectors match the Go oracle at {args.revision}")
    else:
        path.write_text(document)
        print(f"wrote {len(records)} vectors to {FIXTURE}")


if __name__ == "__main__":
    main()
