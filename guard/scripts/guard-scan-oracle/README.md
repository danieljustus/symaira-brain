# Guard scan Go oracle

run.sh check rebuilds symbrain guard scan from the pinned Go revision and
compares exact stdout/stderr bytes for table, JSON, help, error, default TTY,
pipe, and single-finding cases. The fixture also records JSONC, YAML,
command/URL, OpenCode local/remote/unknown, environment merge/redaction,
missing-path, and Darwin/Linux XDG source-path coverage.

The oracle fails closed if the pinned revision is unavailable, the fixture
source hashes drift, or the production Go scan graph differs from the pin.
Use only the wrapper so Go/Python temporary files and caches stay on external
storage:

bash guard/scripts/guard-scan-oracle/run.sh check
bash guard/scripts/guard-scan-oracle/run.sh test
bash guard/scripts/guard-scan-oracle/run.sh parity target/debug/symbrain

write is intentionally explicit; changing the pinned source requires a new
reviewed commit hash in oracle.py before regenerating the fixture.
