"""Owned Windows-only polling/drop experiment; no production fix or Linux pass."""
import argparse
import base64
import csv
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time


def encoded(raw):
    return base64.b64encode(raw).decode("ascii")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", type=Path)
    parser.add_argument("report", type=Path)
    args = parser.parse_args()
    assert os.name == "nt", "actual native Windows required; no cross-target acceptance"
    binary = args.binary.resolve()
    report = args.report.resolve()
    evidence = Path(str(report) + ".evidence")
    evidence.mkdir(parents=True, exist_ok=False)
    data = dict(status="running", actual_windows=True, binary=str(binary),
                binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                observations=[], acceptance=False, production_acceptance=False,
                original_cli_timeout_cause_proved=False,
                scope="Owned test-only historical JobObject polling/drop and bounded comparison. "
                "This does not establish the phase or cause of the original CI timeout.")
    source = Path(__file__).resolve().parents[2]
    data["candidate_head"] = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=source, text=True).strip()
    data["candidate_dirty"] = bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=source))
    inputs = ["scripts/setup-source-oracle/windows_job_run.sh", "scripts/setup-source-oracle/windows_job_probe.py",
              "rust/symbrain-cli/tests/source_job_notifications.rs", "rust/symbrain-cli/src/setup_source_process.rs", "Cargo.lock"]
    data["candidate_source_sha256"] = {name:hashlib.sha256((source / name).read_bytes()).hexdigest() for name in inputs}

    def checkpoint():
        report.write_bytes((json.dumps(data, indent=2) + "\n").encode("utf-8"))

    def system(label, executable, arguments):
        request=dict(label=label,executable=executable,arguments=arguments,started=time.time())
        data.setdefault("system_requests",[]).append(request)
        checkpoint()
        result = subprocess.run([str(Path(os.environ["SystemRoot"]) / "System32" / executable),
                                 *arguments], capture_output=True, timeout=5)
        (evidence / (label + ".stdout")).write_bytes(result.stdout)
        (evidence / (label + ".stderr")).write_bytes(result.stderr)
        return dict(exit=result.returncode, stdout_base64=encoded(result.stdout),
                    stderr_base64=encoded(result.stderr))

    def active(pid, label):
        result = system(label, "tasklist.exe", ["/FI", f"PID eq {pid}", "/FO", "CSV", "/NH"])
        rows = csv.reader(base64.b64decode(result["stdout_base64"]).decode(errors="replace").splitlines())
        result["pid"] = pid
        matching = [row for row in rows if len(row) > 1 and row[1] == str(pid)]
        result["active"] = bool(matching)
        result["owned_image_name_matches"] = all(row[0].casefold() == binary.name.casefold() for row in matching)
        assert result["exit"] == 0, result
        assert result["owned_image_name_matches"], "recorded descendant PID names an unrelated image; refuse cleanup"
        return result

    checkpoint()
    try:
        for mode in ["historical", "inner-wait", "descendant"]:
            root = evidence / mode
            root.mkdir()
            observation = dict(mode=mode, started=time.time(), status="starting", forced_cleanup=[])
            data["observations"].append(observation)
            checkpoint()
            with (root / "parent.stdout").open("wb") as stdout, (root / "parent.stderr").open("wb") as stderr:
                child = subprocess.Popen([str(binary), "owned_job_diagnostic_helper", "--exact", "--ignored", "--nocapture"],
                                         env={**os.environ, "SOURCE_JOB_DIAGNOSTIC_ROOT":str(root),
                                              "SOURCE_JOB_DIAGNOSTIC_MODE":mode}, stdout=stdout, stderr=stderr)
                observation.update(pid=child.pid, status="running")
                checkpoint()
                try:
                    observation["exit"] = child.wait(timeout=12)
                    observation["timed_out"] = False
                except subprocess.TimeoutExpired:
                    observation["timed_out"] = True
                    observation["forced_cleanup"].append(dict(pid=child.pid,
                        result=system(mode + "-parent-taskkill", "taskkill.exe", ["/PID", str(child.pid), "/T", "/F"])))
                    observation["exit"] = child.wait(timeout=5)
                finally:
                    if child.poll() is None:
                        observation["forced_cleanup"].append(dict(pid=child.pid,
                            result=system(mode + "-finally-taskkill", "taskkill.exe", ["/PID", str(child.pid), "/T", "/F"])))
                        child.wait(timeout=5)
            observation.update(ended=time.time(), status="returned")
            phases = [json.loads(line) for line in (root / "phases.jsonl").read_bytes().splitlines()]
            observation["phases"] = phases
            observation["streams"] = {str(path.relative_to(root)):dict(bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest(),
                                          raw_base64=encoded(raw)) for path in root.iterdir() if path.is_file()
                                      for raw in [path.read_bytes()]}
            descendants = root / "descendant.pid"
            observation["descendant_checks"] = []
            if descendants.exists():
                pid = int(descendants.read_text())
                deadline = time.monotonic() + 2
                while True:
                    check = active(pid, mode + "-descendant-tasklist-" + str(len(observation["descendant_checks"])))
                    observation["descendant_checks"].append(check)
                    checkpoint()
                    if not check["active"] or time.monotonic() >= deadline:
                        break
                    time.sleep(0.02)
            observation["descendant"] = observation["descendant_checks"][-1] if observation["descendant_checks"] else None
            if observation["descendant"] and observation["descendant"]["active"]:
                pid = observation["descendant"]["pid"]
                observation["forced_cleanup"].append(dict(pid=pid,
                    result=system(mode + "-descendant-taskkill", "taskkill.exe", ["/PID", str(pid), "/T", "/F"])))
            observed = {row["phase"] for row in phases}
            if mode == "historical":
                matched = observation["timed_out"] and {"parent-exited", "post-exit-notifications-polled", "historical-drop-enter", "historical-kill-enter"} <= observed and "wrapper-drop-returned" not in observed
            else:
                matched = not observation["timed_out"] and observation["exit"] == 0 and "inner-wait-returned" in observed and "wrapper-drop-returned" in observed and not observation["forced_cleanup"] and (not observation["descendant"] or not observation["descendant"]["active"])
            observation["intended_observation_matched"] = matched
            checkpoint()
        data.update(status="complete", acceptance=all(row["intended_observation_matched"] for row in data["observations"]))
        checkpoint()
        return int(not data["acceptance"])
    except BaseException as error:
        data.update(status="failed", exception_type=type(error).__name__, exception=str(error))
        checkpoint()
        raise


if __name__ == "__main__":
    raise SystemExit(main())
