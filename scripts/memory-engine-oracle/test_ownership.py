"""Owned portable input/receipt controls; no SDK/compiler/product execution."""
import hashlib
import io
import json
import os
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile
import binding
import build
import dependencies
import owner


class Ownership(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def file(self, name, value=b"owned"):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(value)
        return path

    def test_extra_build_script_and_ignored_source_are_rejected(self):
        self.file("rust/package/src/lib.rs")
        selected = {"rust/package/src/lib.rs": "owned"}
        for name in ("rust/package/build.rs", "rust/package/ignored.rs", "rust/package/data.txt"):
            path = self.file(name)
            with self.assertRaisesRegex(ValueError, "unexpected physical"):
                owner.physical(self.root, selected, "rust")
            path.unlink()
        owner.physical(self.root, selected, "rust")

    def test_linked_git_metadata_is_not_a_compilation_source(self):
        self.file(".git", b"gitdir: owned-git-metadata\n")
        self.file("owned.go", b"package owned\n")
        owner.physical(self.root, {"owned.go": "owned"}, "go")
        self.file("unexpected.go", b"package foreign\n")
        with self.assertRaisesRegex(ValueError, "unexpected physical"):
            owner.physical(self.root, {"owned.go": "owned"}, "go")

    def test_current_and_ancestor_cargo_config_are_rejected(self):
        checkout = self.root / "checkout"
        checkout.mkdir()
        for prefix in (self.root, checkout):
            for name in ("config", "config.toml"):
                path = prefix / ".cargo" / name
                path.parent.mkdir(exist_ok=True)
                path.write_text("[build]\nrustc-wrapper = 'foreign'\n")
                with self.assertRaisesRegex(ValueError, "Cargo ancestor"):
                    owner.configurations(checkout)
                path.unlink()
        owner.configurations(checkout)

    def test_broken_config_and_source_symlinks_are_rejected(self):
        self.file("rust/package/src/lib.rs")
        path = self.root / "rust/package/build.rs"
        path.symlink_to("missing.rs")
        with self.assertRaisesRegex(ValueError, "symlink"):
            owner.physical(self.root, {"rust/package/src/lib.rs": "owned"}, "rust")
        cfg = self.root / ".cargo/config"
        cfg.parent.mkdir()
        cfg.symlink_to("missing.toml")
        with self.assertRaisesRegex(ValueError, "Cargo ancestor"):
            owner.configurations(self.root)

    def test_tracked_assets_stage_exactly_and_extra_assets_do_not(self):
        path = self.file("rust/package/schema.sql", b"CREATE TABLE exact(a);")
        self.file("untracked.txt", b"never staged")
        selected = {path.relative_to(self.root).as_posix(): owner.sha(path)}
        destination = self.root / "owned-copy"
        owner.stage(self.root, selected, destination)
        self.assertEqual((destination / "rust/package/schema.sql").read_bytes(), path.read_bytes())
        self.assertFalse((destination / "untracked.txt").exists())
        path.write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "source changed"):
            owner.stage(self.root, selected, self.root / "second")

    def test_operator_flags_wrappers_and_workspace_are_not_inherited(self):
        inherited = {"PATH": "/foreign", "CARGO_HOME": "/foreign", "CARGO_BUILD_RUSTC_WRAPPER": "foreign", "RUSTFLAGS": "--cfg foreign", "GOWORK": "/foreign/go.work", "GOENV": "/foreign/env", "GOTOOLCHAIN": "auto", "GIT_CONFIG_COUNT": "9", "CC": "/foreign/cc", "CPATH": "/foreign/include"}
        with patch.dict(os.environ, inherited):
            env = owner.environment(self.root, Path("/pinned-go"), Path("/pinned-rust"), Path("/owned-target"))
        self.assertEqual(env["GOWORK"], "off")
        self.assertEqual(env["GOENV"], "off")
        self.assertEqual(env["GOTOOLCHAIN"], "local")
        self.assertEqual(env["CARGO_HOME"], str(self.root / "cargo-home"))
        self.assertNotIn("/foreign", env["PATH"])
        for key in ("RUSTFLAGS", "CARGO_BUILD_RUSTC_WRAPPER", "GIT_CONFIG_COUNT", "CC", "CPATH"):
            self.assertNotIn(key, env)

    def artifact_event(self, path, fresh=False, manifest=None):
        return json.dumps({"reason": "compiler-artifact", "target": {"name": "engine_probe", "kind": ["example"]}, "manifest_path": str(manifest or self.root / "Cargo.toml"), "fresh": fresh, "executable": str(path)}).encode()

    def test_only_observed_current_owned_cargo_artifact_is_accepted(self):
        path = self.file("target/debug/examples/engine_probe")
        event = self.artifact_event(path)
        self.assertEqual(owner.artifact(event, self.root / "target", self.root / "Cargo.toml"), path)
        for wrong in (self.artifact_event(path, fresh=True), self.artifact_event(path, manifest=self.root / "foreign.toml"), event + b"\n" + event, self.artifact_event(self.file("outside"))):
            with self.assertRaises(ValueError):
                owner.artifact(wrong, self.root / "target", self.root / "Cargo.toml")

    def go_fixture(self):
        module, version = "example.test/owned", "v1.0.0"
        prefix = module + "@" + version + "/"
        data = {prefix + "go.mod": b"module example.test/owned\n", prefix + "owned.go": b"package owned\n"}
        base = "cache/download/" + module + "/@v/" + version
        path = self.file(base + ".zip", b"")
        with zipfile.ZipFile(path, "w") as archive:
            for name, body in data.items():
                archive.writestr(name, body)
        self.file(base + ".mod", data[prefix + "go.mod"])
        sums = module + " " + version + " " + dependencies.hash1(data) + "\n"
        sums += module + " " + version + "/go.mod " + dependencies.hash1({"go.mod": data[prefix + "go.mod"]}) + "\n"
        self.file("source/go.sum", sums.encode())
        return path, base

    def test_go_archive_source_and_mod_body_are_checked(self):
        path, base = self.go_fixture()
        observed = dependencies.go_inputs(self.root / "source", self.root, {})
        self.assertEqual(len(observed["files"]), 2)
        with zipfile.ZipFile(path, "w") as archive:
            archive.writestr("example.test/owned@v1.0.0/owned.go", "altered source")
        with self.assertRaisesRegex(ValueError, "source archive"):
            dependencies.go_inputs(self.root / "source", self.root, {})
        self.go_fixture()
        self.file(base + ".mod", b"module different\n")
        with self.assertRaisesRegex(ValueError, "declaration"):
            dependencies.go_inputs(self.root / "source", self.root, {})

    def test_duplicate_and_traversal_go_archive_are_rejected(self):
        path, _ = self.go_fixture()
        with zipfile.ZipFile(path, "w") as archive:
            archive.writestr("example.test/owned@v1.0.0/../escape", "bad")
        with self.assertRaisesRegex(ValueError, "invalid Go archive"):
            dependencies.go_inputs(self.root / "source", self.root, {})

    def test_cargo_archive_git_database_and_metadata_have_body_checks(self):
        path = self.file("registry/cache/owned/example-1.0.0.crate", b"")
        with tarfile.open(path, "w:gz") as archive:
            row = tarfile.TarInfo("example-1.0.0/src/lib.rs")
            row.size = 12
            archive.addfile(row, io.BytesIO(b"pub fn f(){}"))
        lock = 'version = 4\n[[package]]\nname = "example"\nversion = "1.0.0"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "' + dependencies.sha(path) + '"\n'
        self.file("source/Cargo.lock", lock.encode())
        metadata = self.file("registry/index/owned/.cache/example")
        database = self.file("git/db/owned/objects/pack/pinned.pack")
        pinned = {"registry": "owned", "cargo_metadata": {str(metadata.relative_to(self.root)): dependencies.sha(metadata)}, "cargo_git_database": {str(database.relative_to(self.root)): dependencies.sha(database)}, "git_owners": []}
        observed = dependencies.cargo_inputs(self.root / "source", self.root, pinned)
        self.assertEqual(len(observed["files"]), 3)
        for changed in (path, metadata, database):
            original = changed.read_bytes()
            changed.write_bytes(b"altered")
            with self.assertRaises(ValueError):
                dependencies.cargo_inputs(self.root / "source", self.root, pinned)
            changed.write_bytes(original)
        path.unlink()
        with self.assertRaises(FileNotFoundError):
            dependencies.cargo_inputs(self.root / "source", self.root, pinned)

    def test_missing_dependency_admission_prevents_every_sdk_child(self):
        arguments = ["build.py"]
        for name in ("go-source", "rust-source", "go-sdk", "rust-sdk", "go-modcache", "cargo-cache", "target"):
            directory = self.root / name
            directory.mkdir()
            arguments += ["--" + name, str(directory)]
        arguments += ["--output", str(self.root / "output")]
        with patch("sys.argv", arguments), patch.object(binding, "sources", return_value={}), patch.object(binding, "sdks", return_value={}), patch.object(dependencies, "admit", side_effect=FileNotFoundError("missing pinned source archive")), patch.object(build.subprocess, "run") as child:
            with self.assertRaisesRegex(FileNotFoundError, "pinned source archive"):
                build.main()
            child.assert_not_called()
        receipt = json.loads((self.root / "output/build-receipt.json").read_bytes())
        self.assertEqual(receipt["steps"], [])
        self.assertIn("missing pinned source archive", receipt["admission_error"])

    def test_private_cache_never_copies_config_or_extracted_foreign_source(self):
        original = self.file("registry/cache/owned.crate")
        self.file("config.toml", b"[build]\nrustc-wrapper='foreign'\n")
        self.file("registry/src/foreign/lib.rs", b"altered")
        selected = {"files": {"registry/cache/owned.crate": dependencies.sha(original)}}
        output = self.root / "private"
        dependencies.materialize(self.root, selected, output)
        self.assertFalse((output / "config.toml").exists())
        self.assertFalse((output / "registry/src").exists())
        dependencies.staged(output, selected)
        (output / "registry/cache/owned.crate").write_bytes(b"altered")
        with self.assertRaises(ValueError):
            dependencies.staged(output, selected)


if __name__ == "__main__":
    unittest.main()
