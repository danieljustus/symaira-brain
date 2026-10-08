"""Synthetic source predicate checks; live kernel fixture is separately named."""
import os
import sys
import unittest

import raw_path_admission as admission


def synthetic_report(platform="darwin", excluded=True):
    # These records are explicitly fabricated to test the accounting predicate.
    # They are never daemon/SDK/native/kernel observation receipts.
    report = {"kind": "synthetic predicate projection", "platform_system": platform,
              "requested_cases": [], "cases": [], "unavailable_cases": []}
    settings = [{}, {"SYMBROWSE_ALLOW_PRIVATE": "true"}, {"SYMBROWSE_SSRF": "true"},
                {"XDG_CACHE_HOME": "relative-cache"}, {"LOCALAPPDATA": ""}]
    variants = [(row, None, False) for row in settings]
    if platform != "win32":
        variants += [({}, raw.hex(), False) for raw in (*admission.INVALID_ORIGINS, b"\xef\xbf\xbd")]
        variants.append(({}, None, True))
    for index, (setting, raw, git) in enumerate(variants):
        requested = {"id": f"process-{index}", "session": f"owned-{index}", "settings": setting,
                     "raw_origin_hex": raw, "git_origin": git}
        report["requested_cases"].append(requested)
        case = dict(requested)
        if excluded and raw in ("e282", "c0af"):
            root = b"/owned/synthetic-not-created"
            attempted = root + b"/origin-" + bytes.fromhex(raw)
            case.update(child_executed=False, parity=False, kernel_probe={
                "kind": "actual owned kernel mkdir, no Go/Rust child", "platform": platform,
                "operation": "os.mkdir", "raw_origin_hex": raw,
                "path_component_bytes_hex": (b"origin-" + bytes.fromhex(raw)).hex(),
                "admitted": False, "root_bytes_hex": root.hex(),
                "attempted_path_bytes_hex": attempted.hex(), "root_mode": 0o700,
                "error": {"errno": 92, "filename_bytes_hex": attempted.hex()}, "before": [],
                "after_attempt": [], "after_entry_cleanup": [], "owned_root_removed": True,
                "child_executed": False, "parity": False})
            report["unavailable_cases"].append(case)
        else:
            case.update(matches=True, go={"observations": [{}] * 7}, rust={"observations": [{}] * 7})
            if raw is not None:
                case["kernel_probe"] = {"admitted": True, "owned_root_removed": True}
            report["cases"].append(case)
    report["total"] = len(report["cases"]) * 7
    return report


class AccountingTests(unittest.TestCase):
    def test_exact_original_domains_and_counts(self):
        for platform, excluded, count in (("darwin", True, 49), ("darwin", False, 63),
                                         ("linux", False, 63), ("win32", False, 35)):
            record = synthetic_report(platform, excluded)
            self.assertTrue(admission.validate_accounting(record))
            self.assertEqual(record["total"], count)
            self.assertTrue(all(row["rejected"] for row in admission.accounting_controls(record)))

    def test_exclusion_cannot_be_moved_to_linux(self):
        with self.assertRaises(AssertionError):
            admission.validate_accounting(synthetic_report("linux", True))

    def test_other_errno_raw_bytes_cleanup_and_parity_reject(self):
        for mutation in ("errno", "bytes", "cleanup", "parity"):
            record = synthetic_report()
            row = record["unavailable_cases"][0]
            if mutation == "errno": row["kernel_probe"]["error"]["errno"] = 13
            if mutation == "bytes": row["kernel_probe"]["attempted_path_bytes_hex"] = "ff"
            if mutation == "cleanup": row["kernel_probe"]["owned_root_removed"] = False
            if mutation == "parity": row["parity"] = True
            with self.assertRaises(AssertionError): admission.validate_accounting(record)

    def test_removing_both_requested_and_excluded_cannot_certify_domain(self):
        record = synthetic_report()
        removed = record["unavailable_cases"].pop()
        record["requested_cases"] = [row for row in record["requested_cases"] if row["id"] != removed["id"]]
        with self.assertRaises(AssertionError): admission.validate_accounting(record)


@unittest.skipUnless(os.name == "posix", "POSIX raw-byte filename input domain")
class LiveKernelTests(unittest.TestCase):
    def test_real_owned_kernel_probe_keeps_valid_replacement_required(self):
        for raw in (*admission.INVALID_ORIGINS, b"\xef\xbf\xbd"):
            record = admission.probe(raw, "/tmp" if sys.platform == "darwin" else None)
            self.assertTrue(record["owned_root_removed"])
            self.assertEqual(record["after_entry_cleanup"], [])
            if raw == b"\xef\xbf\xbd" or sys.platform != "darwin":
                self.assertTrue(record["admitted"])
            elif not record["admitted"]:
                self.assertTrue(admission.unavailable_is_proven(record, raw))


    def test_exact_startup_provider_name_is_preflighted_on_the_native_kernel(self):
        name = b"raw-\xff"
        record = admission.probe_name(name, "/tmp" if sys.platform == "darwin" else None)
        self.assertEqual(record["operation"], "os.mkdir")
        self.assertEqual(record["path_component_bytes_hex"], name.hex())
        self.assertEqual(record["attempted_path_bytes_hex"],
                         (bytes.fromhex(record["root_bytes_hex"]) + b"/" + name).hex())
        self.assertTrue(record["owned_root_removed"])
        self.assertEqual(record["after_entry_cleanup"], [])
        if not record["admitted"]:
            self.assertTrue(admission.unavailable_name_is_proven(record, name))
            self.assertFalse(record["child_executed"])
            self.assertFalse(record["parity"])


if __name__ == "__main__":
    unittest.main()
