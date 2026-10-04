"""Registry parity assertions and negative controls; independent of progress logging."""
import base64
import copy
from datetime import datetime
import json
import os
import re
import daemon_registry_cli as cli_edges


def normalize(record: dict, *, rust: bool) -> dict:
    value = copy.deepcopy(record)
    value.pop("cli_edges")  # compare() verifies literal bytes before this projection.
    root, begin, end = value.pop("root"), value.pop("begin"), value.pop("end")
    pids = {value["pid"], value["restart_pid"], value["owner"]["data"]["pid"]}
    assert all(isinstance(pid, int) and pid > 0 for pid in pids)
    assert len(pids) == 3, "restart/autostart did not have independent process owners"
    owner_pid = value["owner"]["data"]["pid"]
    first_pid, restart_pid = value["pid"], value["restart_pid"]
    for record in value["registry"]:
        data = record["response"].get("data", {})
        infos = data.get("sessions", [data])
        for info in infos:
            if "pid" in info:
                assert info["pid"] == first_pid, "registry reported a different process owner"
    for info in value["restart"]["data"]["sessions"]:
        assert info["pid"] == restart_pid, "restart retained the previous process owner"
    for key in ("owner", "owner_info"):
        assert value[key]["data"]["pid"] == owner_pid
    value.pop("pid")
    value.pop("restart_pid")
    for key in ("first_stop", "restart_stop"):
        stop_result = value.pop(key)
        assert stop_result["exit"] == 0 and stop_result["stdout"] == ""
        if rust:
            assert stop_result["stderr"] == "symbrowse shutdown: begin\nsymbrowse shutdown: no browser session\n"
        else:
            line = stop_result["stderr"]
            match = re.fullmatch(r'time=([^ ]+) level=WARN msg="no external risk decider available \(symbrain not found or disabled\); risk decisions fall back to the built-in policy"\n', line)
            assert match, f"unrecognized Go diagnostic: {line!r}"
            timestamp = datetime.fromisoformat(match[1].replace("Z", "+00:00")).timestamp()
            assert begin - 1 <= timestamp <= end + 1
    if rust:
        status = value["owner"]["data"]
        assert status.pop("session") == value["session"] + "a"
        assert status.pop("mode") == "browser"

    def walk(item, key=""):
        if isinstance(item, dict):
            return {k: walk(v, k) for k, v in item.items()}
        if isinstance(item, list):
            return [walk(v, key) for v in item]
        if key == "pid":
            assert item in pids, f"foreign PID {item}"
            return "<validated-owner-pid>"
        if key in ("started_at", "last_activity"):
            assert item.endswith("Z"), f"activity timestamp is not UTC: {item}"
            timestamp = datetime.fromisoformat(item.replace("Z", "+00:00")).timestamp()
            assert begin - 1 <= timestamp <= end + 1, f"timestamp outside observation: {item}"
            return "<validated-time>"
        if isinstance(item, str):
            return item.replace(root, "<owned-root>").replace(root.replace("\\", "/"), "<owned-root>")
        return item

    def text_fields(text, expected_pid=None):
        def timestamp(match):
            raw = match[2].strip("\"'")
            parsed = datetime.fromisoformat(raw.replace("Z", "+00:00")).timestamp()
            assert begin - 1 <= parsed <= end + 1, f"invalid text timestamp: {raw}"
            return match[1] + "<validated-time>"
        def pid(match):
            numeric = float(match[2])
            assert numeric.is_integer() and int(numeric) in pids, f"foreign text PID: {match[2]}"
            if expected_pid is not None:
                assert int(numeric) == expected_pid, "inspection text reported a different owner"
            return match[1] + "<validated-owner-pid>"
        text = re.sub(r"(?m)^(\s*(?:started_at|last_activity):\s*)([^\n]+)$", timestamp, text)
        return re.sub(r"(?m)^(\s*pid:\s*)([0-9]+(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?)$", pid, text)

    for group in ("missing", "invalid_cli", "inspection", "autostart", "state_commands"):
        for observed in value[group]:
            failed = group in ("missing", "invalid_cli") or group == "state_commands" and "missing" in observed["arguments"]
            assert observed["exit"] == (1 if failed else 0), observed
            if not failed:
                assert not observed["stderr"], observed
            if observed["stdout"].lstrip().startswith("{"):
                observed["stdout"] = json.loads(observed["stdout"])
                if group == "inspection":
                    data = observed["stdout"].get("data", observed["stdout"])
                    for info in data.get("sessions", [data]):
                        assert info["pid"] == first_pid, "inspection reported a different process owner"
            else:
                observed["stdout"] = text_fields(observed["stdout"], first_pid if group == "inspection" else None)
    return walk(value)


def compare(case: dict) -> bool:
    # These generic validation failures have no paths/PIDs/timestamps: preserve
    # literal arguments, exit, stdout and stderr without any projection.
    assert case["go"]["invalid_cli"] == case["rust"]["invalid_cli"], "invalid-session CLI bytes differ"
    cli_edges.compare(case["go"]["cli_edges"], case["rust"]["cli_edges"])
    return normalize(case["go"], rust=False) == normalize(case["rust"], rust=True)


def controls(case: dict) -> list[dict]:
    rejected = []
    for name in ("foreign-owner", "previous-owner-after-restart", "missing-error-detail", "invalid-timestamp", "invalid-cli-exit", "invalid-cli-protocol-code", "raw-cli-byte-loss", "help-description"):
        bad = copy.deepcopy(case)
        if name == "foreign-owner":
            bad["rust"]["owner_info"]["data"]["pid"] += 1
        elif name == "previous-owner-after-restart":
            bad["rust"]["restart"]["data"]["sessions"][0]["pid"] = bad["rust"]["pid"]
        elif name == "missing-error-detail":
            bad["rust"]["missing"][1]["stdout"] = '{"success":false,"error":{"code":"daemon_unavailable"}}'
        elif name == "invalid-cli-exit":
            bad["rust"]["invalid_cli"][0]["exit"] = 7
        elif name == "invalid-cli-protocol-code":
            response = json.loads(bad["rust"]["invalid_cli"][1]["stdout"])
            response["error"]["code"] = "invalid_session"
            bad["rust"]["invalid_cli"][1]["stdout"] = json.dumps(response)
        elif name in ("raw-cli-byte-loss", "help-description"):
            group = "invalid" if name == "raw-cli-byte-loss" else "help"
            record = bad["rust"]["cli_edges"][group][3 if group == "invalid" else 0]
            field = "stderr_base64" if group == "invalid" else "stdout_base64"
            raw = base64.b64decode(record[field])
            raw = raw.replace(b"\\xff", "\ufffd".encode()) if group == "invalid" and os.name == "posix" else raw + b" changed diagnostic"
            if group == "help":
                raw = base64.b64decode(record[field]).replace(b"Inspect browser sessions", b"Inspect daemon-owned sessions")
            record[field] = base64.b64encode(raw).decode()
        else:
            bad["rust"]["registry"][0]["response"]["data"]["started_at"] = "1970-01-01T00:00:00Z"
        try:
            matched = compare(bad)
        except AssertionError:
            matched = False
        assert not matched, f"negative control escaped comparator: {name}"
        rejected.append({"name": name, "rejected": True})
    return rejected

