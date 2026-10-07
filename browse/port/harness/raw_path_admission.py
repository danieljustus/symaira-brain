"""Live owned kernel admission, distinct from daemon parity and SDK projections."""
import copy
import os
import stat
import sys
import tempfile

INVALID_ORIGINS = (b"\xe2\x82", b"\xc0\xaf")


def state(root):
    return [{"name_hex": name.hex(), "mode": os.lstat(root + b"/" + name).st_mode,
             "inode": os.lstat(root + b"/" + name).st_ino}
            for name in sorted(os.listdir(root))]


def probe_name(name, parent, record=None):
    """Record actual kernel admission for one exact raw filename component."""
    if not isinstance(name, bytes) or not name or b"/" in name or b"\0" in name:
        raise ValueError("filename must be one non-empty raw component")
    if record is None:
        record = {}
    record.update({"kind": "actual owned kernel mkdir, no Go/Rust child", "platform": sys.platform,
                   "operation": "os.mkdir", "path_component_bytes_hex": name.hex(),
                   "child_executed": False, "parity": False})
    with tempfile.TemporaryDirectory(prefix="bd-path-", dir=parent) as folder:
        root = os.fsencode(folder)
        path = root + b"/" + name
        metadata = os.lstat(root)
        record.update(root_bytes_hex=root.hex(), attempted_path_bytes_hex=path.hex(),
                      root_mode=stat.S_IMODE(metadata.st_mode), root_device=metadata.st_dev,
                      before=state(root))
        try:
            os.mkdir(path, 0o700)
        except OSError as error:
            record.update(admitted=False, error={"type": type(error).__name__, "errno": error.errno,
                          "strerror": error.strerror, "repr": repr(error),
                          "filename_bytes_hex": os.fsencode(error.filename).hex()})
        else:
            record.update(admitted=True, error=None)
        record["after_attempt"] = state(root)
        if record["admitted"]:
            expected = [{"name_hex": name.hex(), "mode": stat.S_IFDIR | 0o700,
                         "inode": os.lstat(path).st_ino}]
            if record["after_attempt"] != expected:
                raise AssertionError("kernel changed the admitted filename bytes/type/mode")
            os.rmdir(path)
        record["after_entry_cleanup"] = state(root)
    record["owned_root_removed"] = not os.path.lexists(root)
    return record


def probe(raw, parent, record=None):
    if record is None:
        record = {}
    record["raw_origin_hex"] = raw.hex()
    return probe_name(b"origin-" + raw, parent, record)


def unavailable_name_is_proven(record, name):
    root = bytes.fromhex(record["root_bytes_hex"])
    attempted = root + b"/" + name
    return (record["kind"] == "actual owned kernel mkdir, no Go/Rust child"
            and record["platform"] == "darwin" and record["operation"] == "os.mkdir"
            and record["path_component_bytes_hex"] == name.hex()
            and record["admitted"] is False and record["error"]["errno"] == 92
            and record["error"]["filename_bytes_hex"] == attempted.hex()
            and record["attempted_path_bytes_hex"] == attempted.hex()
            and record["root_mode"] == 0o700 and record["before"] == []
            and record["after_attempt"] == [] and record["after_entry_cleanup"] == []
            and record["owned_root_removed"] is True and record["child_executed"] is False
            and record["parity"] is False)


def unavailable_is_proven(record, raw):
    return (raw in INVALID_ORIGINS and record["raw_origin_hex"] == raw.hex()
            and unavailable_name_is_proven(record, b"origin-" + raw))


def validate_accounting(report):
    requested = {row["id"]: row for row in report["requested_cases"]}
    assert len(requested) == len(report["requested_cases"]), "duplicate requested case"
    settings = [{}, {"SYMBROWSE_ALLOW_PRIVATE": "true"}, {"SYMBROWSE_SSRF": "true"},
                {"XDG_CACHE_HOME": "relative-cache"}, {"LOCALAPPDATA": ""}]
    variants = [(value, None, False) for value in settings]
    if report["platform_system"] != "win32":
        variants.extend(({}, value.hex(), False) for value in (*INVALID_ORIGINS, b"\xef\xbf\xbd"))
        variants.append(({}, None, True))
    assert set(requested) == {f"process-{index}" for index in range(len(variants))}, "requested domain changed"
    for index, (value, raw, git) in enumerate(variants):
        row = requested[f"process-{index}"]
        assert (row["settings"], row["raw_origin_hex"], row["git_origin"]) == (value, raw, git)
    executed = {row["id"]: row for row in report["cases"]}
    unavailable = {row["id"]: row for row in report["unavailable_cases"]}
    assert len(executed) == len(report["cases"]), "duplicate executed case"
    assert len(unavailable) == len(report["unavailable_cases"]), "duplicate unavailable case"
    assert not executed.keys() & unavailable.keys(), "case both executed and unavailable"
    assert executed.keys() | unavailable.keys() == requested.keys(), "lost/unrecorded case"
    for owner, rows in (("executed", executed), ("unavailable", unavailable)):
        for case_id, row in rows.items():
            expected = requested[case_id]
            for key in ("session", "settings", "raw_origin_hex", "git_origin"):
                assert row[key] == expected[key], f"{owner} input changed: {case_id}/{key}"
            if owner == "unavailable":
                assert row["settings"] == {} and row["git_origin"] is False
                assert report["platform_system"] == "darwin"
                assert row["child_executed"] is False and row["parity"] is False
                assert unavailable_is_proven(row["kernel_probe"], bytes.fromhex(row["raw_origin_hex"]))
            else:
                assert row["matches"] is True, f"unmatched admitted case: {case_id}"
                for product in ("go", "rust"):
                    assert len(row[product]["observations"]) == 7
                if row["raw_origin_hex"] is not None:
                    assert row["kernel_probe"]["admitted"] is True
                    assert row["kernel_probe"]["owned_root_removed"] is True
    # Valid U+FFFD and raw Git stdout stay executed on every POSIX runner.
    if report["platform_system"] != "win32":
        assert any(row["raw_origin_hex"] == "efbfbd" for row in executed.values())
        assert any(row["git_origin"] is True for row in executed.values())
    assert report["total"] == len(executed) * 7
    return True


def accounting_controls(report):
    """Actual receipt mutations reject lost/admitted/unavailable-domain drift."""
    changes = []
    bad = copy.deepcopy(report); bad["cases"].pop(); changes.append(("dropped-executed-case", bad))
    bad = copy.deepcopy(report); bad["cases"].append(bad["cases"][0]); changes.append(("duplicate-case", bad))
    bad = copy.deepcopy(report); bad["total"] += 7; changes.append(("invented-total", bad))
    bad = copy.deepcopy(report); last = bad["cases"].pop()
    bad["requested_cases"] = [row for row in bad["requested_cases"] if row["id"] != last["id"]]
    bad["total"] -= 7; changes.append(("dropped-requested-domain", bad))
    if report["unavailable_cases"]:
        bad = copy.deepcopy(report); bad["unavailable_cases"].pop(); changes.append(("unrecorded-exclusion", bad))
        bad = copy.deepcopy(report); bad["unavailable_cases"][0]["kernel_probe"]["error"]["errno"] = 13
        changes.append(("other-kernel-errno", bad))
        bad = copy.deepcopy(report); bad["unavailable_cases"][0]["child_executed"] = True
        changes.append(("unavailable-child-parity", bad))
    rejected = []
    for name, bad in changes:
        try:
            validate_accounting(bad)
        except (AssertionError, KeyError, ValueError):
            rejected.append({"name": name, "rejected": True})
        else:
            raise AssertionError(f"domain accounting control accepted: {name}")
    return rejected
