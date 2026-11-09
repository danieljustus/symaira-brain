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

    def test_unrelated_output_is_untouched(self):
        self.assertEqual(legacy_doctor_view(b"x\n", b"y\n"), b"y\n")


if __name__ == "__main__":
    unittest.main()
