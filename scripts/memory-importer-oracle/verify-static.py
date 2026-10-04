"""Source/proof binding only. No compiler, SDK probe, target or product run."""
import argparse
import ast
import hashlib
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FROZEN = Path("/workspace/oracles/daemon772-go-source")
SDK = Path("/workspace/toolchains/go1.26.7")
FROZEN_SHA = "dcddcef0df5789123c7c9a7ebe6e01f10e941f2c"
PARENT = "a30bba8bbd52323fa5e000207f1c391d576ac86c"
OUTPUT = ROOT / "migration/evidence/memory-importers-761/source-preparation/source-map.json"


def git(root, *arguments):
    return subprocess.check_output(["git", "-C", str(root), *arguments]).decode().strip()


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def files(root, names):
    return {name:digest(root/name) for name in sorted(names)}


def build():
    assert git(FROZEN,"rev-parse","HEAD")==FROZEN_SHA
    assert not git(FROZEN,"status","--porcelain"), "frozen source modified"
    subprocess.run(["git","-C",str(ROOT),"merge-base","--is-ancestor",PARENT,"HEAD"],check=True)
    assert not (ROOT/"target").exists(), "source-only checkpoint acquired target"
    names = git(ROOT,"ls-files").splitlines()
    native = [name for name in names if name.endswith(".rs") or name.endswith("Cargo.toml") or name=="Cargo.lock"]
    inputs = [name for name in names if name.startswith("scripts/memory-importer-oracle/") or
              name.startswith("migration/evidence/memory-importers-761/source-preparation/original-testdata/") or
              name in ["AGENTS.md","docs/adr/0002-product-boundaries.md","migration/contract-matrix.csv",
                       "migration/implementation-plan.md","MIGRATION-RUST.md","docs/adr/memory-importer-library-checkpoint-761.md"]]
    inputs.extend(name for name in names if name in [
        "migration/evidence/memory-importers-761/source-preparation/issue-761.json",
        "migration/evidence/memory-importers-761/source-preparation/contract-inventory.json",
        "migration/evidence/memory-importers-761/source-preparation/case-plan.json",
        "migration/evidence/memory-importers-761/source-preparation/edit-correction.json",
        "migration/evidence/memory-importers-761/source-preparation/edit-correction-original.patch.gz"])
    frozen_names = git(FROZEN,"ls-files").splitlines()
    frozen = [name for name in frozen_names if name.endswith(".go") or name in ["go.mod","go.sum"] or
              name.startswith("internal/memory/importer/")]
    sdk_names = ["src/"+name for name in [
        "bufio/scan.go","bufio/bufio.go","unicode/utf8/utf8.go","strings/strings.go",
        "encoding/json/decode.go","encoding/json/encode.go","encoding/json/scanner.go","encoding/json/fold.go",
        "path/filepath/path.go","path/filepath/path_unix.go","path/filepath/path_windows.go",
        "internal/filepathlite/path.go","internal/filepathlite/path_unix.go","internal/filepathlite/path_windows.go",
        "time/time.go","time/format.go","time/format_rfc3339.go","time/zoneinfo.go","time/zoneinfo_unix.go",
        "database/sql/sql.go","database/sql/convert.go","os/stat_linux.go","os/stat_darwin.go","os/types_unix.go","os/env.go",
    ]]
    modules = {
        "modernc":Path("/home/agent/go/pkg/mod/modernc.org/sqlite@v1.59.0"),
        "sqlitekit":Path("/home/agent/go/pkg/mod/github.com/danieljustus/symaira-corekit@v0.17.0"),
    }
    module_map = {"modernc":files(modules["modernc"],["go.mod","rows.go","conn.go","driver.go"]),
                  "sqlitekit":files(modules["sqlitekit"],["go.mod","sqlitekit/sqlitekit.go"])}
    old = git(ROOT,"ls-tree","-r",PARENT,"--","migration/evidence").splitlines()
    current = dict(line.split("\t",1)[::-1] for line in git(ROOT,"ls-tree","-r","HEAD","--","migration/evidence").splitlines())
    retained = {}
    for line in old:
        object_info, path = line.split("\t",1)
        if path.startswith("migration/evidence/memory"):
            assert current.get(path)==object_info, f"ancestor original changed: {path}"
            retained[path]=digest(ROOT/path)
    for family in ("codexmemory","curatedmemory"):
        origin = FROZEN/"internal/memory/importer"/family/"testdata"
        retained_root = OUTPUT.parent/"original-testdata"/family
        for original in origin.rglob("*"):
            if original.is_file(): assert original.read_bytes()==(retained_root/original.relative_to(origin)).read_bytes()
    for path in (ROOT/"scripts/memory-importer-oracle").glob("*.py"):
        ast.parse(path.read_text(),filename=str(path))
    for path in (ROOT/"rust/symbrain-memory/src/importer").glob("*.rs"):
        assert len(path.read_text().splitlines())<400, f"production module exceeds bound: {path}"
    for relative in ["rust/symbrain-memory/src/activity_retention.rs","rust/symbrain-memory/src/activity_retention_tests.rs","rust/symbrain-memory/examples/importers761_oracle.rs"]:
        assert len((ROOT/relative).read_text().splitlines())<400
    for relative in ["Cargo.lock","Cargo.toml","rust/symbrain-memory/Cargo.toml"]:
        original=subprocess.check_output(["git","-C",str(ROOT),"show",f"f7a761d3ef77ad55c1727293be50abf301c6e2a2:{relative}"])
        assert original==(ROOT/relative).read_bytes(),"unexpected dependency delta"
    return {"status":"source-only-not-compiled-or-executed","source_revision":git(ROOT,"rev-parse","HEAD"),
            "frozen_revision":FROZEN_SHA,"store_parent":PARENT,"native_source":files(ROOT,native),
            "prepared_inputs":files(ROOT,inputs),"frozen_source":files(FROZEN,frozen),
            "sdk_source":files(SDK,sdk_names),"module_source":module_map,"retained_parent_proofs":retained,
            "runtime_claims":[],"routing_admission":False,"new_dependencies":[],"allocated_target":None}


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument("--write",action="store_true")
    args=parser.parse_args()
    actual=build()
    if args.write:
        OUTPUT.write_text(json.dumps(actual,indent=2,sort_keys=True)+"\n")
    else:
        original=json.loads(OUTPUT.read_text())
        source=original.pop("source_revision")
        actual.pop("source_revision")
        subprocess.run(["git","-C",str(ROOT),"merge-base","--is-ancestor",source,"HEAD"],check=True)
        assert actual==original,"source/proof binding mismatch"
    print(json.dumps({"status":"static-bindings-pass","native_files":len(actual["native_source"]),
                      "frozen_files":len(actual["frozen_source"]),"sdk_files":len(actual["sdk_source"]),
                      "parent_proofs":len(actual["retained_parent_proofs"]),"runtime_tests_executed":0}))


if __name__=="__main__":main()
