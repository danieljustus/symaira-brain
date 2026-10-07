#!/usr/bin/env bash
set -euo pipefail
umask 022
source /home/agent/.cargo/env
export PATH=/workspace/toolchains/go1.26.7/bin:$PATH
export CARGO_HOME=/home/agent/.cargo RUSTUP_HOME=/home/agent/.rustup
export GOMODCACHE=/home/agent/go/pkg/mod GOCACHE=/home/agent/.cache/go-build
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
python3 - <<'PY'
import shutil
free=shutil.disk_usage('/workspace').free
print('initial_available_bytes='+str(free))
assert free>=2*1024*1024*1024, 'stop before growing new Cargo stage below2GiB'
PY
ss -ltnp 'sport = :11434' > /tmp/symaira-memory649-runtime-25292fe-prep/11434-before.log
python3 scripts/memory-historical-oracle/validate.py \
 --go /workspace/oracles/symbrain-go-dcddcef0 \
 --go-sha256 a68dce5b6f34d10ed568d2a89fab880c889e5ff578735c7bf2ad0535285eda41 \
 --go-source /tmp/symaira-memory649-runtime-25292fe-prep/frozen-go \
 --target /workspace/symaira-memory649-integrated/target \
 --actionlint /workspace/toolchains/bin/actionlint \
 --output /tmp/symaira-memory649-runtime-ab561d5
