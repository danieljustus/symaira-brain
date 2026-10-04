#!/usr/bin/env bash
# Prepared only. Execute AFTER Root explicitly allocates this one archived target.
set -euo pipefail
umask 022
: "${SKILLS_RUNTIME_AUTHORIZED:?Root runtime allocation must be explicit}"
[[ "$SKILLS_RUNTIME_AUTHORIZED" == yes ]]
: "${SKILLS_ALLOCATED_TARGET:?Use only the exclusively handed-off Root target}"
: "${SKILLS_TARGET_ARCHIVE_RECEIPT:?Preserved original actual binaries receipt required}"
: "${SKILLS_SOURCE_HEAD:?Immutable clean successor source required}"
: "${SKILLS_PROOF_ROOT:?Owned absolute output root required}"
[[ -f "$SKILLS_TARGET_ARCHIVE_RECEIPT" ]]
repo=/workspace/symaira-skills794-wide-bootstrap
cd "$repo"
[[ "$(git rev-parse HEAD)" == "$SKILLS_SOURCE_HEAD" ]]
[[ -z "$(git status --porcelain)" ]]
[[ "$SKILLS_ALLOCATED_TARGET" == /* && "$SKILLS_PROOF_ROOT" == /* ]]
[[ ! -e "$SKILLS_PROOF_ROOT" ]]
mkdir -p "$SKILLS_PROOF_ROOT"
source /home/agent/.cargo/env
export PATH=/workspace/toolchains/go1.26.7/bin:$PATH
export CARGO_TARGET_DIR="$SKILLS_ALLOCATED_TARGET"
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
floor() {
  python3 - "$SKILLS_ALLOCATED_TARGET" <<'PY'
import shutil, sys
assert shutil.disk_usage(sys.argv[1]).free >= 700*1024*1024, 'STOP: below 700MiB floor'
PY
}
step() {
  floor
  python3 /tmp/symaira-subreaper.py "$@"
}
step cargo test --locked -p symbrain-audit -p symbrain-cli -p symbrain-skills --all-targets --all-features > "$SKILLS_PROOF_ROOT/affected-tests.log" 2>&1
step cargo test --locked -p symbrain-core --all-targets --all-features > "$SKILLS_PROOF_ROOT/core-tests.log" 2>&1
step cargo clippy --locked -p symbrain-audit -p symbrain-cli -p symbrain-skills -p symbrain-core --all-targets --all-features -- -D warnings > "$SKILLS_PROOF_ROOT/strict-clippy.log" 2>&1
step cargo fmt --all -- --check > "$SKILLS_PROOF_ROOT/fmt.log" 2>&1
step actionlint > "$SKILLS_PROOF_ROOT/actionlint.log" 2>&1
step cargo build --locked -p symbrain-cli --all-features --bin symbrain > "$SKILLS_PROOF_ROOT/cli-build.log" 2>&1
step bash scripts/run-go-oracle.sh dcddcef0df5789123c7c9a7ebe6e01f10e941f2c build -o "$SKILLS_PROOF_ROOT/symbrain-go" ./cmd/symbrain > "$SKILLS_PROOF_ROOT/go-build.log" 2>&1
cp -p "$CARGO_TARGET_DIR/debug/symbrain" "$SKILLS_PROOF_ROOT/symbrain"
step python3 scripts/skills-argv-oracle/compare.py --go "$SKILLS_PROOF_ROOT/symbrain-go" --rust "$SKILLS_PROOF_ROOT/symbrain" --out "$SKILLS_PROOF_ROOT/process.json" > "$SKILLS_PROOF_ROOT/process.log" 2>&1
for control in flag-input quoted-value normalization; do
  floor
  set +e
  python3 /tmp/symaira-subreaper.py python3 scripts/skills-argv-oracle/compare.py --go "$SKILLS_PROOF_ROOT/symbrain-go" --rust "$SKILLS_PROOF_ROOT/symbrain" --out "$SKILLS_PROOF_ROOT/control-$control.json" --negative-control "$control" > "$SKILLS_PROOF_ROOT/control-$control.log" 2>&1
  actual_status=$?
  set -e
  [[ "$actual_status" == 1 ]]
  python3 scripts/skills-argv-oracle/verify_control.py "$SKILLS_PROOF_ROOT/control-$control.json" "$control" >> "$SKILLS_PROOF_ROOT/control-$control.log"
done
# Counts/source hashes/actual ELF archive and no-active-process receipt must be
# finalized and independently reviewed. This script does NOT claim Windows proof.
# Genuine native Windows uses the existing tracked CI job: .exe builds, all312
# wide CreateProcessW cases, both wide tests+ASCII bootstrap,3 same controls and
# whole-workspace strict native CI. Native macOS/Linux protected CI remains due.
