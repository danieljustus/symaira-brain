"""Strict comparisons plus explicitly validated existing corrective contracts."""
import datetime
import hashlib
import json
import re


def digest(data):
    return hashlib.sha256(data).hexdigest()


def current_timestamp(value, start, end):
    parsed = datetime.datetime.fromisoformat(value.replace("Z", "+00:00")).timestamp()
    assert start - 1 <= parsed <= end + 1, "generated timestamp outside this actual process"
    return "<verified-current-process-time>"


def filesystem(root, start, end):
    rows, lock_rows, raw = {}, {}, {}
    for area in ("home", "config", "data", "cache", "state", "project"):
        for path in sorted((root / area).rglob("*")):
            relative = path.relative_to(root).as_posix()
            if path.is_symlink():
                rows[relative] = {"type": "link", "target": str(path.readlink())}
                continue
            if path.is_dir():
                rows[relative] = {"type": "directory", "mode": path.stat().st_mode & 0o777}
                continue
            data = path.read_bytes()
            raw[relative] = {"sha256": digest(data), "bytes": len(data)}
            mode = path.stat().st_mode & 0o777
            if re.fullmatch(r"\.symskills-lock-[0-9a-f]{64}", path.name):
                assert data == b"" and path.is_file(), "SKL004 lock must be a regular empty file"
                lock_rows[relative] = {"sha256": digest(data), "mode": mode}
                continue
            normalized = None
            if path.name == ".symskills.json":
                normalized = json.loads(data)
                installed = normalized.get("installed")
                if installed and installed != "2020-01-02T03:04:05Z":
                    normalized["installed"] = current_timestamp(installed, start, end)
            if path.name == "events.jsonl":
                normalized = []
                for line in data.splitlines():
                    event = json.loads(line)
                    if not event["ts"].startswith("2020-01-02T03:04:05"):
                        event["ts"] = current_timestamp(event["ts"], start, end)
                    normalized.append(event)
            rows[relative] = {"type": "file", "mode": mode,
                              "value": normalized if normalized is not None else digest(data)}
    return {"files": rows, "raw_file_hashes": raw, "validated_skl004_locks": lock_rows}


def cli_view(record, root, rust=False):
    out = bytes.fromhex(record["stdout_hex"])
    err = bytes.fromhex(record["stderr_hex"])
    if not out or record["exit"]:
        return record["exit"], out, err
    # Each status case starts without a render cache. SKL008 adds no fields
    # in that state; the unchanged dedicated twelve-case gate proves its
    # present-cache extensions separately. No report field is projected away.
    if rust:
        assert b"\tRENDER\n" not in out
        try:
            value = json.loads(out)
        except (ValueError, UnicodeError):
            value = None
        if isinstance(value, dict) and set(value) == {"installs", "summary"}:
            for row in value["installs"]:
                assert not any(key in row for key in ("render_status", "render_drift", "render_error"))
    return record["exit"], out, err


def mcp_view(record):
    out = bytes.fromhex(record["stdout_hex"])
    responses = [json.loads(line) for line in out.splitlines()]
    ids = [row.get("id") for row in responses]
    assert ids == [1, 2, 3], f"missing or duplicate actual responses: {ids}"
    catalog = responses[1]["result"]["tools"]
    skills = [row for row in catalog if row["name"].startswith("skills_")]
    assert len(skills) == 11 and len({row["name"] for row in skills}) == 11
    responses[1]["result"]["tools"] = skills
    result = responses[2]["result"]
    assert len(result["content"]) == 1 and result["content"][0]["type"] == "text"
    text = result["content"][0]["text"]
    if not result.get("isError", False):
        # Payload comparison is semantic JSON with ALL fields retained. Raw
        # transport/content bytes remain separately recorded, not called equal.
        result["content"][0]["text"] = json.loads(text)
    return record["exit"], responses, bytes.fromhex(record["stderr_hex"])


def matched(go, rust, root, mcp):
    view = mcp_view if mcp else lambda row: cli_view(row, root, row is rust)
    return view(go) == view(rust) and go["filesystem"]["files"] == rust["filesystem"]["files"]
