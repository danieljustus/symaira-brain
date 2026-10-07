# Remaining usage file contracts: reference baseline for #768

Status: baseline only. No additional production source, native routing or native
parity is accepted by this decision. The parent candidate remains immutable at
`5111d8b66246f1c397fc9f962cece73c10929ad2` during independent review.

## Decision and reason

Capture remaining constructors against the immutable Go implementation before
porting Copilot/Kimi file readers or relaxing configuration gates. This keeps
provider behavior, file precedence and request bytes reviewable independently of
a proposed Rust implementation. A single observed Go map iteration is not a
stable account-selection contract: distinct eligible Copilot tokens must remain
gated unless later evidence establishes deterministic selection. Automatic host
Keychain access also remains gated; synthetic providers cannot prove host ACLs.

The baseline uses 97 owned inputs: Copilot 34, Kimi 39, Nous 12, OpenRouter 7 and
OpenCode 5. It creates private temporary HOME/USERPROFILE roots, uses synthetic
credentials and a canned HTTP 401 transport, and records actual constructors,
complete reports, request methods/URLs/headers/body, raw header value bytes and
file hashes. It verifies all fixture files are unchanged. Supplemental tests are
untracked in a temporary checkout of frozen Go
`dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`; tracked Go source is unchanged.

## Observations that constrain a future port

Copilot's typed entry decoder retains scalar tokens across null duplicates,
ignores overflowing unknown metadata, skips invalid entries, and checks the
GitHub prefix pass before fallback entries. A root duplicate replaced with null
removes that entry. Both successful and failed environment resolution preempt
file selection. Multiple different prefix tokens remain nondeterministic; the
recorded selection documents one actual run only. Literal credential-file
reference schemes remain literal header bytes.

Kimi's typed access-token decoder accepts Go field folding and ignores unknown
metadata; a wrong known type before a later valid duplicate invalidates the
credential. An existing unusable current credential file suppresses the legacy
home. API resolution failure still permits a usable CLI strategy. With all three
strategies present, the canned rejection produces three actual requests, while
the reported authentication source prefers the CLI. Device IDs use Go Unicode
whitespace trimming and can contain non-UTF-8 bytes; raw header hex preserves the
latter without treating JSON replacement text as byte parity.

The Linux HOME/USERPROFILE mismatch selects HOME. This is a Linux observation,
not proof for Windows. Base URL examples preserve Go concatenation and escaping,
including its query/fragment behavior. Rejected HTTP/userinfo overrides use the
provider default. OpenCode workspace overrides also record the no-cookie state.
The five numeric JWT boundary examples are missing on Linux amd64 Go 1.26.7;
architecture-dependent conversion gates remain until native platform evidence
or an explicitly validated platform contract exists.

## Sequencing and validation boundary

Independent review has reproduced an inherited Hermes defect: the Go JSON depth
limit is missing both for the outer credential file and decoded JWT claims.
Finish that review, retain the original failed inputs/replays, repair the shared
parent first, and rerun the reference, Hermes and provider-file process gates.
Only then implement a bounded successor and evaluate its own complete native
constructor/request/CLI parity, read-only boundaries and real failure controls.

The Go-only runner is `scripts/usage-local-files-baseline/run.sh`. Its receipt
states that the native successor is unimplemented and unverified. No native
build, private CLI parity or native macOS/Windows proof is supplied by this
baseline. The existing frozen fixtures and historical failure receipts remain
unchanged.
