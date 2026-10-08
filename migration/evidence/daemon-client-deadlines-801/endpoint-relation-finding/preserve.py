"""Preserve immutable source evidence; synthetic code-unit projection only."""
import hashlib
import io
import json
from pathlib import Path
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[4]
OUT = Path(__file__).resolve().parent
HEADS = ["591248a89f56c7b810daf9fa1a036ec20061aaec", "c91938d445593e66ca47d613ed1d2c60c15ba060"]
FILES = ["browse/crates/symbrowse-daemon/src/client/transport.rs", "browse/crates/symbrowse-daemon/src/client/errors.rs", "browse/crates/symbrowse-daemon/src/server/windows.rs", "browse/Cargo.lock"]
REG = Path("/home/agent/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f")
REFERENCES = [REG / "serde_core-1.0.229/src/ser/impls.rs", REG / "serde_json-1.0.151/src/macros.rs", REG / "interprocess-2.4.4/src/os/windows/path_conversion.rs"]


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    rows = []
    archive = OUT / "pre-correction-source.tar.gz"
    with tarfile.open(archive, "w:gz") as packed:
        items = [(head + "/" + file, subprocess.check_output(["git", "show", head + ":" + file], cwd=ROOT), {"git_head": head, "path": file}) for head in HEADS for file in FILES]
        items += [("reference/" + str(path.relative_to(REG)), path.read_bytes(), {"local_reference": str(path)}) for path in REFERENCES]
        for name, data, source in items:
            info = tarfile.TarInfo(name)
            info.size, info.mtime = len(data), 0
            packed.addfile(info, io.BytesIO(data))
            rows.append({"member": name, "bytes": len(data), "sha256": sha(data), **source})
    with tarfile.open(archive, "r:gz") as packed:
        for row in rows:
            data = packed.extractfile(row["member"]).read()
            assert sha(data) == row["sha256"] and len(data) == row["bytes"]
    # Explicit synthetic projection of source-defined replacement relation.
    # No Rust/Windows/Go execution, no kernel endpoint or executable involved.
    vectors = [{"id": name, "native_code_units": raw, "legacy_lossy_code_units": lossy,
                "OsStr_client_owner_matches_unchanged_server": raw == lossy,
                "restored_lossy_client_owner_matches_unchanged_server": True}
               for name, raw, lossy in [("ascii", [65], [65]), ("valid_non_bmp_pair", [0xD83D, 0xDE00], [0xD83D, 0xDE00]),
                                        ("lone_high", [0xD800], [0xFFFD]), ("lone_low", [0xDC00], [0xFFFD]),
                                        ("literal_replacement", [0xFFFD], [0xFFFD])]]
    result = {"kind": "SOURCE_FINDING_AND_SYNTHETIC_UTF16_PROJECTION_NOT_RUNTIME", "heads": HEADS,
              "archive": archive.name, "archive_sha256": sha(archive.read_bytes()), "members": rows,
              "roundtrips": len(rows), "vectors": vectors,
              "original_raw_archive_sha256": "07f40f5be16af310d851241fd1e25ae6a773860a3a848f75b662acdc87ca1b34",
              "finding": "Direct OsStr client alone splits endpoint ownership from unchanged lossy server/wake. Invalid Path serialization is an inherited separate error-path limitation, not fixed here.",
              "disposition": "Restore prior shared lossy relation for bounded deadline fix; whole-three-owner raw-WTF16 cutover remains open."}
    (OUT / "receipt.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({"members": len(rows), "sha256": result["archive_sha256"], "kind": result["kind"]}))


if __name__ == "__main__":
    main()
