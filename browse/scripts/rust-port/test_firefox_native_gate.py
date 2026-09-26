import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from firefox_native_gate import (
    TARGETS,
    Links,
    checksum_for,
    elf_machine,
    exact_artifact,
    latest_artifact,
    pe_machine,
    runner_matches,
    safe_relative,
    version_key,
    verify_metadata,
    verify_executable_version,
)


class FirefoxNativeGateTests(unittest.TestCase):
    def test_directory_index_paths_are_reduced_to_artifact_names(self):
        links = Links()
        links.feed('<a href="/pub/firefox/nightly/latest-mozilla-central/firefox-10.0a1.en-US.mac.pkg">')
        self.assertEqual(links.hrefs, {"firefox-10.0a1.en-US.mac.pkg"})

    def test_all_six_targets_bind_to_native_runner_and_official_artifact(self):
        self.assertEqual(len(TARGETS), 6)
        for target, (runner_os, runner_arch, platform, extension, _, _) in TARGETS.items():
            with self.subTest(target=target):
                runner_matches(target, runner_os, runner_arch)
                version, name = latest_artifact(
                    {
                        f"firefox-9.0a1.en-US.{platform}.{extension}",
                        f"firefox-9.0a1.en-US.{platform}.checksums",
                        f"firefox-9.0a1.en-US.{platform}.buildhub.json",
                        f"firefox-10.0a1.en-US.{platform}.{extension}",
                        f"firefox-10.0a1.en-US.{platform}.checksums",
                        f"firefox-10.0a1.en-US.{platform}.buildhub.json",
                    },
                    platform,
                    extension,
                )
                self.assertEqual((version, name), ("10.0a1", f"firefox-10.0a1.en-US.{platform}.{extension}"))

    def test_latest_artifact_requires_checksum_and_buildhub_identity(self):
        with self.assertRaisesRegex(ValueError, "no checksummed Nightly"):
            latest_artifact({"firefox-10.0a1.en-US.mac.pkg"}, "mac", "pkg")

    def test_prepare_selects_only_the_planned_version(self):
        names = {
            f"firefox-{version}.en-US.mac.{suffix}"
            for version in ("9.0a1", "10.0a1")
            for suffix in ("pkg", "checksums", "buildhub.json")
        }
        self.assertEqual(exact_artifact(names, "mac", "pkg", "9.0a1"), "firefox-9.0a1.en-US.mac.pkg")
        with self.assertRaisesRegex(ValueError, "expected one"):
            exact_artifact(names, "mac", "pkg", "11.0a1")

    def test_checksum_matches_only_exact_sha256_filename(self):
        content = "a" * 64 + " sha256 100 firefox.zip\n" + "b" * 64 + " sha512 100 firefox.zip\n"
        self.assertEqual(checksum_for(content, "firefox.zip"), "a" * 64)
        with self.assertRaisesRegex(ValueError, "checksum missing"):
            checksum_for(content, "other.zip")

    def test_archive_paths_reject_parent_escape(self):
        self.assertEqual(str(safe_relative("firefox/firefox")), "firefox/firefox")
        with self.assertRaisesRegex(ValueError, "unsafe path"):
            safe_relative("firefox/../../outside")

    def test_pe_architecture_is_read_from_binary_header(self):
        executable = bytearray(128)
        executable[:2] = b"MZ"
        executable[0x3C:0x40] = (64).to_bytes(4, "little")
        executable[64:68] = b"PE\0\0"
        executable[68:70] = (0xAA64).to_bytes(2, "little")
        with TemporaryDirectory() as temp:
            path = Path(temp) / "firefox.exe"
            path.write_bytes(executable)
            self.assertEqual(pe_machine(path), 0xAA64)

    def test_elf_architecture_is_read_from_binary_header(self):
        executable = bytearray(20)
        executable[:6] = b"\x7fELF\x02\x01"
        executable[18:20] = (183).to_bytes(2, "little")
        with TemporaryDirectory() as temp:
            path = Path(temp) / "firefox-bin"
            path.write_bytes(executable)
            self.assertEqual(elf_machine(path), 183)

    def test_buildhub_must_identify_matching_official_nightly(self):
        metadata = {
            "source": {"product": "firefox", "revision": "a" * 40},
            "target": {
                "channel": "nightly",
                "locale": "en-US",
                "platform": "linux-aarch64",
                "version": "10.0a1",
            },
            "download": {"url": "https://archive.mozilla.org/pub/firefox/nightly/firefox.tar.xz"},
        }
        metadata["source"]["repository"] = "https://hg.mozilla.org/mozilla-central"
        metadata["source"]["tree"] = "mozilla-central"
        metadata["target"]["os"] = "linux"
        self.assertEqual(verify_metadata(metadata, "linux-aarch64", "10.0a1", "linux"), "a" * 40)
        metadata["target"]["channel"] = "release"
        with self.assertRaisesRegex(ValueError, "Buildhub identity mismatch"):
            verify_metadata(metadata, "linux-aarch64", "10.0a1", "linux")

    def test_nightly_version_order_is_numeric(self):
        self.assertGreater(version_key("10.0a1"), version_key("9.0a99"))

    def test_version_identity_ignores_runtime_warning_stream(self):
        verify_executable_version("Mozilla Firefox 158.0a1\n", "158.0a1")
        with self.assertRaisesRegex(ValueError, "identity mismatch"):
            verify_executable_version("warning\nMozilla Firefox 158.0a1\n", "158.0a1")


if __name__ == "__main__":
    unittest.main()
