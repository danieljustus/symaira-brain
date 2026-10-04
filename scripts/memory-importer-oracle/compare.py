"""Compare actual constructor process reports; no expected-data normalization."""
import argparse
import json
from pathlib import Path


def reports(path):
    rows = [json.loads(line) for line in path.read_text().splitlines() if line]
    assert rows, "empty process output"
    assert len({row["id"] for row in rows}) == len(rows), "duplicate report ID"
    return rows


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("input", type=Path)
    parser.add_argument("go", type=Path)
    parser.add_argument("native", type=Path)
    args = parser.parse_args()
    packets = json.loads(args.input.read_text())
    ids = [packet["Id"] for packet in packets]
    go, native = reports(args.go), reports(args.native)
    assert [row["id"] for row in go] == ids, "Go process missing/reordered cases"
    assert [row["id"] for row in native] == ids, "native process missing/reordered cases"
    for family in {packet["Family"] for packet in packets}:
        positive = [row for row in go if row["family"] == family and row["id"].endswith("-positive")]
        assert len(positive) == 1 and positive[0]["sessions"], f"bootstrap failed for {family}"
        assert any(item["facts"] for item in positive[0]["imports"]), f"zero positive facts for {family}"
    for before, after in zip(go,native,strict=True):
        assert before == after, f"full constructor report mismatch: {before['id']}\nGo={before!r}\nNative={after!r}"
    print(json.dumps({"actual_reports":len(go),"status":"pass","normalizations":[]}))


if __name__ == "__main__": main()
