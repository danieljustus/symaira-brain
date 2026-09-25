#!/usr/bin/env python3
"""Compare primp loopback wire captures to the same-commit Go capture.

This is a diagnostic candidate spike only. A successful run means the raw
observations were captured and compared; it is never FETCH-002 acceptance.
"""
from __future__ import annotations

import argparse
import base64
import importlib.util
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
VALIDATOR = ROOT / "browse/port/harness/fetch_fingerprints_validate.py"
SPEC = importlib.util.spec_from_file_location("fetch002_validator", VALIDATOR)
assert SPEC and SPEC.loader
validator = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = validator
SPEC.loader.exec_module(validator)

PROFILES = ("chrome", "edge", "firefox", "ios", "opera", "safari")


def parsed_vector(row: dict) -> dict:
    hello = validator.reparse_client_hello(
        base64.b64decode(row["raw_client_hello_record_b64"], validate=True)
    )
    settings = validator.reparse_settings_frame(
        base64.b64decode(row["raw_settings_frame_b64"], validate=True)
    )
    raw_headers = base64.b64decode(row["raw_header_block_b64"], validate=True)
    try:
        names = validator.hpack_decode_names(raw_headers)
        header_names_source = "independent_raw_hpack_decode"
    except validator.CaptureError:
        # The existing validator deliberately bounds name decoding to literal
        # non-Huffman names. The Go capture was already checked by that gate;
        # primp's full Rust HPACK decoder records names while decoding the raw
        # block. Preserve that order and disclose the fallback in the report.
        names = row.get("header_names")
        if not isinstance(names, list) or not all(isinstance(name, str) for name in names):
            raise
        header_names_source = "capture_decoder_fallback"
    return {
        "protocol": row["protocol"],
        "tls_client_version": hello["tls_client_version"],
        "cipher_suites_no_grease": validator._filter_grease(hello["cipher_suites"]),
        "extensions_no_grease": validator._filter_grease(hello["extensions"]),
        "supported_groups_no_grease": validator._filter_grease(hello["supported_groups"]),
        "ec_point_formats": hello["ec_point_formats"],
        "h2_settings": settings,
        "header_names": names,
        "header_names_source": header_names_source,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--go-capture", type=Path, required=True)
    parser.add_argument("--primp-capture", type=Path, required=True)
    parser.add_argument("--sha", required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()

    go = json.loads(args.go_capture.read_text())
    primp = json.loads(args.primp_capture.read_text())
    if go.get("worktree_head") != args.sha:
        raise SystemExit("Go capture SHA differs from the checked-out commit")
    if go.get("capture_kind") != "loopback_hermetic_production_client":
        raise SystemExit("input is not the source-bound production Go capture")
    if primp.get("worktree_sha") != args.sha:
        raise SystemExit("primp capture SHA differs from the checked-out commit")
    if primp.get("parity_acceptance") is not False:
        raise SystemExit("primp spike must remain diagnostic-only")

    go_rows = {row["profile"]: row for row in go.get("profiles", [])}
    primp_rows = {row["profile"]: row for row in primp.get("profiles", [])}
    if set(go_rows) != set(PROFILES):
        raise SystemExit("source-bound Go capture does not contain the exact six profiles")
    if set(primp_rows) != {*PROFILES, "honest-default"}:
        raise SystemExit("primp capture must contain six candidates and one honest default")

    profiles = []
    for name in PROFILES:
        go_vector = parsed_vector(go_rows[name])
        primp_vector = parsed_vector(primp_rows[name])
        fields = {
            key: {"equal": go_vector[key] == primp_vector[key], "go": go_vector[key], "primp": primp_vector[key]}
            for key in go_vector
            if key != "header_names_source"
        }
        profiles.append({
            "profile": name,
            "fields": fields,
            "raw_client_hello_bytes_equal": (
                go_rows[name]["raw_client_hello_record_b64"]
                == primp_rows[name]["raw_client_hello_record_b64"]
            ),
            "header_name_decoder": {
                "go": go_vector["header_names_source"],
                "primp": primp_vector["header_names_source"],
            },
            "candidate_equal": all(value["equal"] for value in fields.values()),
        })

    chrome = parsed_vector(primp_rows["chrome"])
    honest = parsed_vector(primp_rows["honest-default"])
    negative_control_distinct = chrome != honest
    if not negative_control_distinct:
        raise SystemExit("honest default negative control was indistinguishable from primp Chrome")

    result = {
        "schema_version": 1,
        "worktree_sha": args.sha,
        "comparison_mode": "raw-capture-derived-ordered-fields",
        "parity_acceptance": False,
        "note": "Candidate diagnostic only. A match on this spike does not satisfy FETCH-002; six native source/oracle and integrated transport gates remain required.",
        "negative_control": {
            "profile": "honest-default",
            "compared_with": "chrome",
            "distinct": negative_control_distinct,
        },
        "profiles": profiles,
        "all_candidate_profiles_equal": all(row["candidate_equal"] for row in profiles),
    }
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print(f"Wrote diagnostic FETCH-002 primp report for {args.sha}")
    print("Parity acceptance remains false.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
