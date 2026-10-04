"""Source identities for actual Doctor output processes and public callers."""
from pathlib import Path
import hashlib,platform,subprocess
ROOT=Path(__file__).resolve().parents[2]
ORACLE='dcddcef0df5789123c7c9a7ebe6e01f10e941f2c'
def identity():
 names=subprocess.check_output(['git','ls-files','--','Cargo.toml','Cargo.lock','rust/symbrain-cli','rust/symbrain-managed','rust/symbrain-core','scripts/doctor-stdout-oracle','scripts/doctor-repair-oracle','scripts/setup-output-oracle','scripts/run-go-oracle.sh','.github/workflows/ci.yml','docs/adr/doctor-stdout-failure-boundary.md'],cwd=ROOT,text=True).splitlines()
 frozen=subprocess.check_output(['git','ls-tree','-r','--name-only',ORACLE,'--','cmd/symbrain','internal/managed','internal/config','internal/xdg','go.mod','go.sum'],cwd=ROOT,text=True).splitlines()
 return {'candidate_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'candidate_dirty':bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)),'go_oracle_ref':ORACLE,'go_sdk':subprocess.check_output(['go','version'],text=True).strip(),'platform':platform.platform(),'candidate_source_sha256':{name:hashlib.sha256((ROOT/name).read_bytes()).hexdigest()for name in names if (ROOT/name).is_file()},'go_source_sha256':{name:hashlib.sha256(subprocess.check_output(['git','show',f'{ORACLE}:{name}'],cwd=ROOT)).hexdigest()for name in frozen if name.endswith('.go')or name in ['go.mod','go.sum']}}
