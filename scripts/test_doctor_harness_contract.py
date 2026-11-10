import unittest

from doctor_harness_contract import legacy_doctor_view

ARROW = "\u2192".encode()


class DoctorHarnessContract(unittest.TestCase):
    def test_human_projection(self):
        go = (ARROW.join([b"  ", b"  agents         config not found: \n"])
              + ARROW.join([b"  ", b"  hermes         config not found: \n"])
              + ARROW.join([b"  ", b"  openclaw       config not found: \n"]))
        rust = (ARROW.join([b"  ", b"  agents         unsupported (no MCP config path)\n"])
                + ARROW.join([b"  ", b"  hermes         config not found: /h/.hermes/config.yaml\n"])
                + ARROW.join([b"  ", b"  openclaw       unsupported (no MCP config path)\n"]))
        self.assertEqual(legacy_doctor_view(go, rust), go)
        with self.assertRaises(AssertionError):
            legacy_doctor_view(go, go.replace(b"hermes         config not found: ", b"hermes         x "))

    def test_json_projection(self):
        go = (b'{"harnesses":[{"name":"hermes","config_path":"","config_found":false,"config_parsed":false,'
              b'"config_error":"harness does not support MCP installation","supports_mcp_install":false}]}')
        rust = (b'{"harnesses":[{"name":"hermes","config_path":"/h/.hermes/config.yaml","config_found":false,'
                b'"config_parsed":false,"supports_mcp_install":false}]}')
        self.assertEqual(legacy_doctor_view(go, rust), go)
        with self.assertRaises(AssertionError):
            legacy_doctor_view(go, go.replace(b'"config_path":""', b'"config_path":"x"'))

    def test_new_sections_human(self):
        go = b"  x\n"
        profiles = ("\n  hermes profiles:\n"
                    "    \u2192  hermes:work    config found, symbrain not registered (read-only): /h/.hermes/profiles/work/config.yaml\n").encode()
        backup = (b"\n  pending first-run backup:\n"
                  b"    !  symbrain   /s/.symbrain-resume.partial (paused)\n"
                  b"       finish: run any `symbrain` command\n")
        self.assertEqual(legacy_doctor_view(go, go + profiles + backup), go)
        for bad in (backup.replace(b"(paused)", b"(stuck)"), backup + backup,
                    backup.replace(b"-resume.partial", b".partial"), profiles.replace(b"hermes:work", b"claude")):
            with self.assertRaises(AssertionError):
                legacy_doctor_view(go, go + bad)

    def test_new_sections_json(self):
        go = b'{"harnesses":[],"links":[]}'
        prof = (b',"hermes_profiles":[{"name":"hermes:work","config_path":"/h/profiles/work/config.yaml",'
                b'"config_found":true,"config_parsed":true,"supports_mcp_install":false,"superseded":["a]b"]}]')
        back = (b',"pending_backups":[{"app":"symbrain","partial_path":"/s/.symbrain-resume.partial",'
                b'"running":false,"finish":"run \\"x\\" ]"}]')
        rust = go[:-1] + prof + back + b"}"
        self.assertEqual(legacy_doctor_view(go, go[:-1] + prof + back + b"}"), go)
        self.assertEqual(rust.count(b"pending_backups"), 1)
        for bad in (back.replace(b'"running":false,', b""), back.replace(b"-resume.partial", b".x"),
                    back.replace(b'"finish":', b'"extra":1,"finish":'), prof.replace(b"hermes:work", b"work")):
            with self.assertRaises(AssertionError):
                legacy_doctor_view(go, go[:-1] + bad + b"}")

    def test_unrelated_output_is_untouched(self):
        self.assertEqual(legacy_doctor_view(b"x\n", b"y\n"), b"y\n")


if __name__ == "__main__":
    unittest.main()
