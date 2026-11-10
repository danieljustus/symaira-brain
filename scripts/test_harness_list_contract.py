import unittest

from harness_list_contract import legacy_harness_list_view

NOTE = "  note\tread-only: symbrain install/uninstall do not support Hermes\n"
GO_HUMAN = b"cursor\tCursor\n  global\t/h/.cursor/mcp.json\tmissing\tservers=(none)\n\n"
HERMES_HUMAN = (
    "hermes\tHermes\n"
    "  global\t/h/.hermes/config.yaml\tparsed\tservers=gh[stdio]\n"
    "  profile work\t/h/.hermes/profiles/work/config.yaml\tinvalid\tservers=(none)\n"
    "    error: harness: hermes config is not valid yaml; symbrain only reads it: parse yaml: x\n"
    + NOTE + "\n"
).encode()
GO_JSON = b'{"schema_version":2,"harnesses":[{"name":"cursor","display_name":"Cursor","global":{"path":"p","exists":false,"parsed":false,"servers":[]}}]}\n'
ENTRY = (
    b',{"name":"hermes","display_name":"Hermes","global":{"path":"/h/.hermes/config.yaml","exists":false,'
    b'"parsed":false,"servers":[]},"profiles":[{"name":"a}\\"b","config":{"path":"/h/.hermes/profiles/a/config.yaml",'
    b'"exists":true,"parsed":true,"servers":[]}}],"read_only":true}'
)


def with_entry(go: bytes) -> bytes:
    return go.replace(b"]}\n", ENTRY + b"]}\n")


class HarnessListContract(unittest.TestCase):
    def test_human_projection(self):
        self.assertEqual(legacy_harness_list_view(GO_HUMAN, GO_HUMAN + HERMES_HUMAN), GO_HUMAN)

    def test_human_rejects_drift(self):
        for bad in (
            HERMES_HUMAN.replace(NOTE.encode(), b""),
            HERMES_HUMAN.replace(b"config.yaml\tparsed", b"settings.yaml\tparsed"),
            HERMES_HUMAN + HERMES_HUMAN,
            HERMES_HUMAN.replace(b"  global", b"  extra"),
        ):
            with self.assertRaises(AssertionError):
                legacy_harness_list_view(GO_HUMAN, GO_HUMAN + bad)
        with self.assertRaises(AssertionError):  # hermes not last
            legacy_harness_list_view(GO_HUMAN, HERMES_HUMAN + GO_HUMAN)
        self.assertNotEqual(legacy_harness_list_view(GO_HUMAN, GO_HUMAN.replace(b"missing", b"parsed") + HERMES_HUMAN), GO_HUMAN)

    def test_json_projection(self):
        self.assertEqual(legacy_harness_list_view(GO_JSON, with_entry(GO_JSON)), GO_JSON)

    def test_json_rejects_drift(self):
        good = with_entry(GO_JSON)
        for bad in (
            good.replace(b'"read_only":true', b'"read_only":false'),
            good.replace(b'"read_only":true', b'"read_only":true,"extra":1'),
            good.replace(b"config.yaml", b"settings.yaml"),
            good.replace(b"]}\n", ENTRY + b"]}\n"),
        ):
            with self.assertRaises(AssertionError):
                legacy_harness_list_view(GO_JSON, bad)
        first = GO_JSON.replace(b'"harnesses":[', b'"harnesses":[' + ENTRY[1:] + b",")
        self.assertNotEqual(legacy_harness_list_view(GO_JSON, first), GO_JSON)  # not projected

    def test_unrelated_output_and_already_listed_go_are_untouched(self):
        self.assertEqual(legacy_harness_list_view(b"", b"x\n"), b"x\n")
        listed = with_entry(GO_JSON)
        self.assertEqual(legacy_harness_list_view(listed, listed), listed)


if __name__ == "__main__":
    unittest.main()
