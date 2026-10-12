"""Non-product check of the actual variant timestamp loop, without filesystem I/O."""
import ast
from pathlib import Path
from types import SimpleNamespace
import unittest


class ByteMetadataInputs(unittest.TestCase):
    def setUp(self):
        source = Path(__file__).with_name("byte_cases.py")
        module = ast.parse(source.read_text(), filename=str(source))
        variant = next(node for node in module.body
                       if isinstance(node, ast.FunctionDef) and node.name == "variant")
        loops = [node for node in variant.body if isinstance(node, ast.For)
                 and isinstance(node.target, ast.Name) and node.target.id == "path"]
        self.assertEqual(len(loops), 1)
        # Execute only the production loop AST. No setup, symlink, process,
        # product, cleanup or mocked result/output comparator is invoked.
        statements: list[ast.stmt] = [loops[0]]
        self.code = compile(ast.Module(body=statements, type_ignores=[]), str(source), "exec")

    def run_loop(self, supports_nofollow, link_names=()):
        calls = []
        self.calls = calls

        class Entry:
            def __init__(self, name, link=False):
                self.name, self.link = name, link

            def is_symlink(self):
                return self.link

            def rglob(self, pattern):
                if pattern != "*":
                    raise AssertionError("unexpected fixture traversal")
                return entries

        root = Entry("root")
        entries = [Entry("file"), Entry("directory"),
                   *(Entry(name, True) for name in link_names)]

        def utime(path, **options):
            if not supports_nofollow and options.get("follow_symlinks") is False:
                raise NotImplementedError("utime: follow_symlinks unavailable on this platform")
            calls.append((path.name, options))

        env = {"root": root, "os": SimpleNamespace(
            utime=utime, supports_follow_symlinks={utime} if supports_nofollow else set())}
        exec(self.code, env)
        return calls

    def test_timestamp_loop_pins_link_inode_without_following(self):
        calls = self.run_loop(True, ("file-link", "directory-link", "dangling-link"))
        self.assertEqual([name for name, _ in calls],
                         ["root", "file", "directory", "file-link", "directory-link", "dangling-link"])
        for name, options in calls:
            self.assertEqual(options, {
                "ns": (1893456000000000123,
                       1577934246000000000 if name.endswith("link") else 1577934245000000000),
                "follow_symlinks": False,
            })

    def test_timestamp_loop_without_nofollow_pins_nonlinks(self):
        self.assertEqual(self.run_loop(False), [
            (name, {"ns": (1893456000000000123, 1577934245000000000)})
            for name in ("root", "file", "directory")
        ])

    def test_timestamp_loop_without_nofollow_rejects_links(self):
        for name in ("file-link", "directory-link", "dangling-link"):
            with self.subTest(name=name):
                with self.assertRaisesRegex(
                        NotImplementedError, "^fixture link timestamps require nofollow os.utime$"):
                    self.run_loop(False, (name,))
                self.assertEqual(self.calls, [
                    (entry, {"ns": (1893456000000000123, 1577934245000000000)})
                    for entry in ("root", "file", "directory")
                ])


if __name__ == "__main__":
    unittest.main()
