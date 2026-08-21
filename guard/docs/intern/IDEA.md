# Symaira Guard (`symguard`)

> Local-first security gateway for AI agents, MCP servers, and Symaira toolchains.

`symaira-guard` protects humans from over-trusting agentic systems. It sits between AI agents and the tools they want to call, inspects capability surfaces, applies local policies, asks for human approval when risk rises, and records tamper-evident audit trails.

The short version: **MCP gives agents tools. `symguard` decides which tool calls are safe enough to run.**

---

## 1. Idea

Modern AI agents are quickly becoming able to read files, call APIs, execute shell commands, browse the web, access secrets, write code, open PRs, and trigger production workflows. That is useful — and also a decent recipe for digital self-harm if the agent is compromised, overconfident, or tricked by prompt injection.

The Model Context Protocol (MCP) solves interoperability, but it does not automatically solve:

- tool poisoning,
- prompt injection,
- unsafe tool descriptions,
- missing authentication,
- unbounded shell/network/filesystem access,
- secret exfiltration,
- cross-agent delegation risk,
- human approval and auditability.

`symaira-guard` is the local control layer for that gap.

It is not an antivirus, not a cloud policy SaaS, and not another chat app. It is a composable, local-first security layer for agent tool execution.

---

## 2. Vision

Symaira's broader direction is **Human Control by Design**:

- humans own their data,
- agents can act usefully,
- autonomy is explicit and bounded,
- every risky action remains understandable, reversible where possible, and auditable.

`symaira-guard` turns that into infrastructure.

Long-term vision:

> A user can connect any AI client to any local or remote tool and still keep clear control over what the agent is allowed to see, decide, execute, remember, and share.

In practice, this means `symguard` becomes the trust boundary between:

```text
AI client / agent  ->  symguard  ->  MCP servers / CLIs / APIs / Symaira tools
```

The agent still gets useful tools. The human gets enforceable boundaries.

Remote access is part of that vision, but `symguard` should not become a VPN.
The right layer is: **agents can use local or remote MCP/tools through existing
secure transports, while `symguard` enforces identity, policy, approval,
redaction, and audit at the tool-call layer.**

---

## 3. Purpose

`symaira-guard` exists to answer five questions before an agent acts:

1. **What is the agent trying to do?**
   - Read-only lookup?
   - File write?
   - Shell execution?
   - Network request?
   - Secret access?
   - Production-impacting operation?

2. **Which tool is being used?**
   - Known and pinned?
   - Newly discovered?
   - Tool description changed since last approval?
   - Local binary, stdio MCP server, HTTP MCP server, or remote API?

3. **What data could leak?**
   - Secrets?
   - Personal data?
   - Repository source?
   - Customer data?
   - Memory context?

4. **Does policy allow it?**
   - Always allow?
   - Ask once?
   - Ask every time?
   - Deny?
   - Allow only in a specific project, repo, identity, or time window?

5. **How is it logged?**
   - What was requested?
   - Which policy matched?
   - Who approved it?
   - What was executed?
   - What tool output came back?
   - Which memory/context/secrets influenced the decision?

---

## 4. Context Position in the Symaira Ecosystem

`symaira-guard` is a **public self-hosted core** in the Symaira ecosystem.

It should follow existing Symaira conventions:

- binary name: `symguard`,
- standalone-first: works without any other Symaira tool installed,
- optional runtime integration with sibling tools via `exec.LookPath`, MCP discovery, HTTP APIs, or config paths,
- no compile-time dependency on sibling public tool repositories,
- no tenant, billing, or hosted-service code — the tool ships as one edition,
- local-first defaults,
- XDG paths:
  - config: `~/.config/symguard/config.toml`,
  - data: `~/.local/share/symguard/`,
  - cache: `~/.cache/symguard/`,
- zero stdio pollution for MCP mode,
- clear fallback behavior if optional tools are missing.

### Ecosystem Layer

```text
┌─────────────────────────────────────────────────────────────┐
│ AI clients / agents                                         │
│ Hermes, Claude Desktop/Code, Cursor, Cline, OpenCode, etc.  │
└───────────────────────┬─────────────────────────────────────┘
                        │ MCP / CLI / HTTP tool calls
                        ▼
┌─────────────────────────────────────────────────────────────┐
│ symguard                                                     │
│ Security gateway, policy engine, approval layer, audit       │
└──────┬───────────────┬───────────────┬───────────────┬──────┘
       │               │               │               │
       ▼               ▼               ▼               ▼
┌──────────┐     ┌───────────┐   ┌───────────┐   ┌───────────┐
│ symvault │     │ symmemory │   │ symscope  │   │ symseek   │
│ secrets  │     │ context   │   │ inventory │   │ retrieval │
└──────────┘     └───────────┘   └───────────┘   └───────────┘
       │               │               │               │
       ▼               ▼               ▼               ▼
┌──────────┐     ┌───────────┐   ┌───────────┐   ┌───────────┐
│ symfetch │     │ symingest │   │ symvibe   │   │ symdesk   │
│ web data │     │ OCR/docs  │   │ workflows │   │ console   │
└──────────┘     └───────────┘   └───────────┘   └───────────┘
```

### Strategic Role

`symaira-guard` is not another data store. It is the **agentic trust boundary**.

- `symvault` protects secrets.
- `symmemory` stores durable context.
- `symseek` retrieves documents.
- `symfetch` pulls web content.
- `symingest` turns files into usable text.
- `symscope` inventories local AI/dev surfaces.
- `symvibe` orchestrates coding workflows.
- `symdesk` becomes the user-facing composition shell.
- **`symguard` decides what agentic actions are safe enough to pass through.**

---

## 5. Core Use Cases

### 5.1 MCP Security Audit

Scan configured MCP servers across local AI clients:

```bash
symguard scan
symguard scan --client hermes
symguard scan --client claude-desktop
symguard scan --format json
```

Findings should include:

- MCP server name, command, args, env exposure,
- local vs remote transport,
- declared tools/resources/prompts,
- risky capabilities,
- suspicious tool descriptions,
- changed tool descriptions since last scan,
- unauthenticated remote endpoints,
- access to shell, filesystem, browser, network, secrets, or production systems.

### 5.2 MCP Proxy / Gateway

Run as a proxy between AI client and tool servers:

```bash
symguard proxy --config ~/.config/symguard/config.toml
```

The proxy should:

- accept MCP requests from an AI client,
- route calls to the configured upstream MCP server,
- classify each tool call,
- apply policy,
- optionally ask for approval,
- redact sensitive data where configured,
- log decisions and outcomes.

### 5.3 Human Approval Before Risky Actions

Examples:

- allow `memory_search` silently,
- ask before `run_command`,
- deny shell commands containing `rm -rf`, `curl | sh`, credential dumping, or uncontrolled network exfiltration,
- ask before reading `.env`, SSH keys, browser profiles, password stores, or customer exports,
- allow Git status/diff but ask before commit/push/deploy.

### 5.4 Tool Pinning

`symaira-guard` should store hashes of MCP tool descriptions and schemas.

If a tool's description changes, especially if it gains hidden instructions or expands scope, `symguard` should flag it:

```text
WARNING: Tool schema changed for server "filesystem" tool "read_file".
Previous hash: ...
Current hash:  ...
Policy: require re-approval
```

This directly targets MCP tool poisoning and rug-pull attacks.

### 5.5 Secret Mediation

Agents should not receive broad secret access by default.

With optional `symvault` integration, `symguard` can mediate scoped secret access:

```text
Agent requests: vault://stripe/live-key
Policy says: deny direct read; allow only delegated call to approved Stripe endpoint
```

The agent gets the capability it needs, not the raw credential unless explicitly approved.

### 5.6 Remote MCP / Agent Access Gateway

Research around a "Tailscale alternative for AI agents" points to a useful
feature, but not to a new VPN product. The useful part belongs here:

> `symguard` should let agents safely reach remote MCP servers and private tools
> over already trusted transports such as Tailscale, SSH tunnels, or LAN/mDNS.

Initial commands could look like:

```bash
symguard remote add macmini --via ssh --host macmini
symguard remote add studio --via tailscale --host studio.tailnet-name.ts.net
symguard remote list --format json

symguard mcp discover --remote macmini
symguard proxy --remote macmini --upstream symmemory
```

The goal is not "all traffic can reach all devices". The goal is:

```text
agent -> symguard -> approved remote MCP tool -> audited result
```

`symguard` should enforce the same local policy model for remote tools as for
local tools:

- read-only discovery by default,
- explicit opt-in for writes,
- per-agent identity and TTL,
- tool/resource-level permissions,
- approval for high-risk calls,
- local audit trail even when the upstream tool runs remotely.

### 5.7 Agent-First Identity

Classic VPNs model mostly users and devices. Agentic workflows need first-class
agent identities.

`symguard` should eventually distinguish:

- human user (`daniel`),
- AI client (`hermes`, `claude-code`, `opencode`, `cursor`),
- agent/run identity (`build-bot`, `paperless-curator`, `release-agent`),
- upstream MCP server (`symmemory@macmini`, `symvault@local`),
- tool/resource/action (`memory_search`, `memory_set`, `vault_get`, `run_command`).

Policies should be expressible at the semantic tool layer, not only at IP/port
level:

```toml
[[rules]]
match.agent = "release-agent"
match.remote = "macmini"
match.server = "symmemory"
match.tool = "memory_search"
decision = "allow"

[[rules]]
match.agent = "release-agent"
match.remote = "macmini"
match.server = "symmemory"
match.tool = "memory_set"
decision = "ask"
ttl = "30m"

[[rules]]
match.client = "remote_mcp"
match.capability = "read_secret"
decision = "deny"
```

### 5.8 Ephemeral Agent Sandboxes

Many useful agents are short-lived: CI jobs, one-off coding agents, importers,
bulk migration workers, or remote maintenance sessions. `symguard` should avoid
treating every one of them like a permanent device.

Useful concepts:

- temporary agent identities with expiration,
- scoped join/authorization tokens stored or mediated via `symvault`,
- automatic policy attachment based on role/job type,
- sandboxed network/tool views for risky runs,
- automatic revoke on TTL expiry or failed policy check.

Example:

```bash
symguard agent register --name paperless-curator --role document_worker --ttl 2h
symguard sandbox create --agent paperless-curator --allow symmemory:memory_search --allow symseek:search
symguard sandbox revoke --expired
```

### 5.9 Explainable Agent Networking

Existing network tools explain packets, routes, and devices. Agents need a more
semantic explanation layer:

- Why can agent X not reach tool Y?
- Was the failure transport, auth, MCP handshake, policy, approval, or upstream error?
- Which rule allowed or denied the call?
- What would happen if this agent tried a write action instead of read-only?

`symguard` should expose diagnostics in human-readable and JSON forms:

```bash
symguard diag path --from agent:paperless-curator --to mcp:symmemory@macmini
symguard diag explain --event evt_123 --format json
symguard policy simulate --agent paperless-curator --remote macmini --server symmemory --tool memory_set
```

The output should be compact enough for LLMs and precise enough for humans:

```json
{
  "decision": "deny",
  "reason": "remote client cannot write confidential memory without approval",
  "matched_rule": "remote-confidential-write-approval",
  "transport": "ssh",
  "upstream_reachable": true,
  "suggested_next_step": "request approval or use memory_search instead"
}
```

### 5.10 Dynamic MCP Discovery Without Blind Trust

Tailscale Aperture and similar products aggregate MCP tools behind one endpoint.
`symguard` can borrow the useful pattern, but should keep stricter local control.

Remote discovery should:

- list remote MCP servers and tools,
- prefix/canonicalize names to avoid collisions,
- classify tool risk before exposing tools to clients,
- pin schemas/descriptions on first approval,
- hide tools that no policy grants,
- re-check drift before invocation,
- keep dynamic registration opt-in, never ambient.

Example naming:

```text
symmemory@macmini.memory_search
symmemory@macmini.memory_set
paperless@macmini.document_search
```

This is the safer Symaira version of "MCP aggregation": discovery is useful,
but `symguard` never turns discovery into automatic trust.

---

## 6. Grundarchitektur

### 6.1 Components

```text
cmd/symguard
  CLI entrypoint

internal/config
  TOML config loader, XDG paths, env overrides

internal/discovery
  Finds AI client MCP configs and upstream MCP servers

internal/mcp
  MCP stdio/HTTP proxy, JSON-RPC frame handling, upstream routing

internal/remote
  Transport providers for existing secure links: ssh, tailscale, lan/mdns,
  and local; no custom VPN daemon in the core

internal/identity
  Human/client/agent/run/upstream identity model, TTLs, scoped grants

internal/policy
  Policy model, matcher, risk classifier, decision engine

internal/approval
  TUI/CLI/browser approval prompts, approval cache

internal/audit
  Append-only event log, hash chaining, optional age encryption

internal/pinning
  Tool schema hashing, drift detection, trust-on-first-use registry

internal/diag
  Explainable access diagnostics, policy simulation, failure classification

internal/redact
  PII/secret redaction before logs and memory handoff

internal/integrations
  Optional runtime integrations: symvault, symmemory, symscope, symdesk
```

### 6.2 Request Flow

```text
1. AI client sends MCP request
2. symguard parses JSON-RPC request
3. symguard identifies upstream server and tool
4. tool schema + call args are classified by risk
5. policy engine decides: allow / ask / deny / transform
6. approval layer asks human if needed
7. upstream MCP call executes if allowed
8. output is optionally redacted/classified
9. audit event is written
10. response is returned to AI client
```

### 6.2b Remote Request Flow

Remote MCP/tool access follows the same trust-boundary model, with transport
setup before upstream invocation:

```text
1. AI client sends MCP request to local symguard
2. symguard resolves client + agent/run identity
3. symguard resolves remote target and transport provider
4. symscope/symguard discover or load pinned upstream MCP capabilities
5. policy engine checks agent, remote, server, tool, action, sensitivity, TTL
6. approval layer asks human if needed
7. remote tunnel/session is opened through existing provider (ssh/tailscale/lan/relay)
8. upstream MCP call executes if allowed
9. output is classified/redacted
10. local audit event records identity, transport, policy, and upstream result
11. response is returned to AI client
```

Important boundary: `symguard` owns policy and audit. It does **not** need to own
packet routing, DNS, NAT traversal, or WireGuard key exchange.

### 6.2c Approval Data Contract

The approval layer pauses a tool call and asks the human for a decision.
The request embeds the original tool call so no separate correlation store
is needed — the frontend (TUI, CLI, or browser) never needs its own lookup
state to know what it is approving.

**ApprovalRequest** — sent to the human for a decision:

| Field | Type | Description |
|---|---|---|
| `id` | string | Stable, unique request ID (echoed in the decision) |
| `hint` | string | Human-readable summary of what is being approved |
| `original_call` | ActionEvent | The full event with tool call, args (redacted), agent identity, and risk context |
| `payload` | any | Optional frontend-specific data (UI hint, metadata) |
| `created_at` | timestamp | RFC 3339 timestamp |
| `ttl` | duration | How long the request is valid before it expires |

**ApprovalDecision** — the human's response:

| Field | Type | Description |
|---|---|---|
| `id` | string | Echoes the ApprovalRequest ID (correlation) |
| `approved` | boolean | True = allow, false = deny |
| `payload` | any | Optional frontend-specific response data |
| `reason` | string | Optional human-written justification |
| `ttl` | duration | How long this decision should be cached |
| `decided_at` | timestamp | RFC 3339 timestamp |

**Correlation mechanism:** The `ApprovalRequest.id` is a stable, unique ID
generated by the policy engine when it decides `ask`. The frontend presents
the request to the human. Any response must echo the exact same `id` in the
`ApprovalDecision.id` field. If the ID does not match an active pending
request, the decision is rejected as stale or forged.

This embedded-correlation design avoids a separate pending-request database
or lookup table — the request IS the state, and matching on the echoed ID
is sufficient for single-process, local-first operation.

### 6.3 Policy Decision Model

Minimum decision states:

```text
allow       execute without interruption
deny        block and return policy error
ask         require explicit user approval
redact      allow but remove sensitive fields
readonly    allow only non-mutating sub-capabilities
sandbox     execute in constrained environment where possible
```

### 6.4 Risk Classes

Initial risk taxonomy:

| Risk | Examples | Default |
|---|---|---|
| `read_public` | docs, README, public web | allow |
| `read_private` | repo files, notes, local docs | allow or ask per project |
| `read_secret` | `.env`, SSH keys, vault entries | ask/deny |
| `write_file` | patch, overwrite, create file | ask |
| `shell` | command execution | ask |
| `network` | outbound API/web requests | allow/ask depending target |
| `browser` | cookies, sessions, web automation | ask |
| `credential_use` | using secrets without revealing them | ask once / scoped allow |
| `deploy` | release, push, infra mutation | ask every time |
| `destructive` | delete, wipe, reset, revoke | ask every time or deny |

### 6.5 Marginal-Capability Risk Capping

The static risk table above (6.4) classifies a tool by its **name** alone. That
is a necessary starting point, but it is not sufficient once `scan` has to
classify real tool inventories: a tool whose name maps to a high-risk class
can still be safe to `allow` if it grants the calling agent no capability
beyond one it already effectively holds — and flagging it anyway trains users
to reflexively approve `ask` prompts, which defeats the point of asking.

**Rule:** after the static capability-name lookup, cap the resulting risk
level at `read_public` (i.e. effectively `allow`) whenever the tool grants
**zero marginal capability** — the calling agent/client could already achieve
the same effect through another tool this session already resolved to
`allow`, or through a trust boundary it already sits inside. Do not raise the
risk of a tool that already scored higher via 6.4; this only ever caps
*downward*, never up.

**Example:** a `read_file` tool on a filesystem MCP server is classified
`read_private` → `ask` by the static table. If the same client already holds
an unrestricted `shell` tool resolved to `allow` (e.g. because the operator
explicitly allow-listed shell for that client), `read_file` grants that
client no capability it did not already have via `shell` — cat is a strict
subset of what a shell can already do. `scan` should cap `read_file`'s risk
down to `allow` in that case and say why (`"no marginal capability over
already-allowed tool: shell"`), rather than prompting the user on every call.
Conversely, if `shell` itself is `ask` or `deny`, `read_file` keeps its
`read_private` classification unchanged — there is nothing to cap it against.

This is a downstream refinement of 6.4, not a replacement: `scan` always
computes the static class first, then applies this cap as a second pass once
it knows what else the same client/session already resolved to `allow`.

### 6.6 Evidence References and Portable Security Case Bundles

A case bundle packages an action chain — events, policy decisions, and audit
records — into a portable, redaction-safe directory that can be exported,
shared, or verified without exposing raw tool arguments by default.

**Directory layout:**

```text
case.symguard/
  manifest.json           # schema version, case ID, record counts, SHA-256 digests
  events.ndjson           # normalized ActionEvent records (from internal/model)
  decisions.ndjson        # ApprovalDecision records (from internal/approval)
  audit.ndjson            # append-only audit log entries
```

The manifest contains:

| Field | Type | Description |
|---|---|---|
| `schema_version` | int | Case bundle schema version |
| `case_id` | string | Stable, unique case identifier |
| `created_at` | timestamp | RFC 3339 |
| `record_counts` | object | Per-stream record counts (events, decisions, audit) |
| `digests` | object | SHA-256 digests of each stream file |
| `source_repo` | string | Repository and HEAD SHA that produced the case |
| `unsigned` | boolean | Always true for local-first bundles; authenticity requires external signatures |

**Evidence references** point to supporting data without copying it:

```text
ref://file/path:line         local file and line number
ref://event/<event-id>       reference to another event in the case
ref://hash/<sha256>          content-addressed reference
```

Evidence references are optional — live events may honestly omit artifact
provenance if the data was ephemeral or unavailable at export time.

**Redaction rule:** The default export is metadata-only and must never include
credentials, full tool output, or raw prompts. Explicit export modes may
include bounded argument/output excerpts with a `redacted: false` annotation.

**Verification contract:** `case verify` checks that:
1. All referenced stream files exist and match their manifest digests.
2. Every event ID referenced by another record resolves to an existing event.
3. No stream references records with a schema version it cannot parse.
What it does **not** verify: that the events are authentic. An unsigned
manifest is not a signature — authenticity requires external signatures or
an audit-log hash-chain checkpoint from the source machine.

---

## 7. Functions / Feature Set

### Phase 0: Repo Foundation

- README and scope definition.
- AGENTS.md with Symaira conventions.
- Go module skeleton.
- CLI command shape.
- Config file schema draft.

### Phase 1: Scanner MVP

- Discover MCP configs for common clients:
  - Hermes,
  - Claude Desktop,
  - Cursor,
  - VS Code/Cline/Roo/Continue,
  - OpenCode where applicable.
- Parse server commands, args, env.
- Run MCP initialize/tools/list smoke checks.
- Classify tools by capability.
- Emit human-readable and JSON reports.

Example:

```bash
symguard scan --all --format table
symguard scan --all --format json > mcp-security-report.json
```

### Phase 2: Policy Engine

- TOML policy config.
- Capability labels.
- Project-level rules.
- Server/tool allowlists and denylists.
- First version of `allow`, `ask`, `deny`.

Example policy sketch:

```toml
[defaults]
shell = "ask"
read_secret = "deny"
write_file = "ask"
network = "ask"

[[rules]]
match.server = "symmemory"
match.tool = "memory_search"
decision = "allow"

[[rules]]
match.server = "symvault"
match.capability = "read_secret"
decision = "ask"

[[rules]]
match.command_contains = ["rm -rf", "curl", "| sh"]
decision = "deny"
```

### Phase 3: MCP Proxy

- stdio proxy mode.
- upstream routing.
- policy enforcement per tool call.
- CLI approval prompt.
- approval cache.
- structured policy errors back to client.

#### Capability/scope probing at connect time

The proxy must not rely only on static, pre-authored allow/deny rules that would
surface as call-time denials. At connect time, the proxy probes what a
downstream MCP server's credential can actually do and hides tools the caller
cannot reach, instead of advertising them and denying on use. The reference
pattern is `github/github-mcp-server`'s startup `HEAD` request reading
`X-OAuth-Scopes` to filter the exposed toolset.

Design notes:

- Probe once per server connect, cache the result for the session, re-probe on
  reconnect or explicit invalidation.
- A tool whose scope is unknown (probe failed, opaque server) is surfaced as
  `unknown` in the tool list — hidden only when the probe positively proves
  unreachability, denied at call time when it stays unknown.
- The probe result is policy input, not policy: allow/deny rules still apply to
  every call; probing only reduces the surface the caller sees.
- The probe itself runs with the least privilege the transport supports and
  must never send credentials to a server other than the one being probed.

#### MCP interception is not confinement

MCP-level interception constrains what an agent may *ask* through the proxy —
it does not constrain the agent's direct process or network egress. An agent
running locally can exec a binary, open a socket, or read a file without ever
talking to the proxy. This is the MCP-interception bypass: proxy deny rules are
a control on the mediated channel only.

Consequences:

- Policy decisions that depend on interception must be stated as *proxy-path*
  controls, never as host-level guarantees.
- Anything that must hold even when the agent does not use the proxy needs a
  confinement mechanism at the OS or container boundary, not at the MCP layer.
- The `sandbox` decision is a real mechanism, not an aspiration: when a call
  resolves to `sandbox`, the runtime must execute it inside an actual
  confinement boundary (see below). If no confinement backend is available,
  `sandbox` fails closed to `deny` rather than degrading to an unconfined run.

Sandbox confinement mechanism (decision):

1. **Preferred backend:** OS-level sandbox profiles — macOS Seatbelt
   (`sandbox-exec` with a generated profile), Linux bubblewrap/seccomp. These
   restrict process, filesystem, and network access without a container
   runtime.
2. **Alternative backend:** container runtime (Docker/Podman/Colima) when
   installed and the invocation can be containerised.
3. **Fallback:** if neither backend is available, the `sandbox` decision
   resolves to `deny` with a reason naming the missing backend. Never run the
   call unconfined under a `sandbox` decision.
4. Profile contents are generated from the policy match (allowed servers,
   commands, network targets) and logged to the audit sink with the profile
   hash for later review.

### Phase 4: Audit & Pinning

- Append-only local audit log.
- Hash chain for tamper evidence.
  - Hash chaining detects modification of retained entries only — it does
    not detect truncation (deleting the tail). Decide the truncation-detection
    mechanism (OS append-only enforcement, and/or a signed external
    checkpoint of the chain head) before implementing `internal/audit`; see
    README.md § "5. Audit" for the tracked version of this note.
- Optional age encryption.
- Tool schema pinning.
- Diff display for changed tools.

### Phase 5: Symaira Integration

- `symscope`: import local MCP/server inventory.
- `symvault`: mediated secret access.
- `symmemory`: store durable security decisions, trusted tools, rejected actions, and provenance summaries.
- `symdesk`: visual dashboard for approvals, policies, and audit trails.
- `symvibe`: approval gates for autonomous coding workflows.

### Phase 6: Agent-Aware Remote Access

Remote access is post-MVP. It should only start after scanner, policy, proxy,
audit, and pinning work locally.

Build order:

1. **Remote target registry**
   - Store known targets in `~/.config/symguard/remotes.toml`.
   - Fields: name, provider, host, allowed_servers, trust_level, labels.
   - Never store raw secrets; use `symvault` references or OS keychain.

2. **Transport provider abstraction**
   - Providers: `local`, `ssh`, `tailscale`, `lan_mdns`.
   - Future provider: `terminal_pro_relay`.
   - Each provider answers: reachable, open session/tunnel, close session, health.
   - No custom WireGuard control plane in the public core.

3. **Remote MCP discovery**
   - Discover remote MCP servers through `symscope` when installed.
   - Fallback to explicit config if `symscope` is missing.
   - Run MCP initialize/tools/list smoke checks through the transport.
   - Prefix remote tools as `<server>@<remote>.<tool>`.

4. **Agent identity + TTL grants**
   - Add agent/run IDs.
   - Add temporary grants with expiration.
   - Support read-only grants by default; writes require explicit policy.

5. **Remote policy enforcement**
   - Match on `client`, `agent`, `remote`, `server`, `tool`, `capability`,
     `sensitivity`, `operation`, `time`, and `project`.
   - Default-deny remote writes and remote secret reads.

6. **Explainable diagnostics**
   - `diag path`: transport + MCP + policy explanation.
   - `policy simulate`: dry-run a remote tool call.
   - `diag explain`: classify a failed event and suggest next step.

7. **Remote audit trail**
   - Log remote identity, transport, policy decision, approval, upstream server,
     tool, redaction summary, and result status.
   - Keep logs local-first; no aggregation off the machine.

Exit criteria for this phase:

- A local agent can call `symmemory@macmini.memory_search` through `symguard`.
- `memory_set` over the same remote path requires approval.
- A failed call explains whether transport, MCP, auth, or policy blocked it.
- No raw secret leaves `symvault` or the local machine unless explicitly approved.

---

## 8. Potential Synergies

### With `symvault`

- Use Vault as the secure backend for policy secrets.
- Avoid raw secret exposure to agents.
- Issue scoped secret-use approvals.
- Log which secret path was used without logging the secret value.

### With `symmemory`

- Store high-level security memories:
  - trusted MCP servers,
  - rejected unsafe patterns,
  - user preferences for approval behavior,
  - project-specific safety rules.
- Feed audit summaries into memory, not raw noisy logs.
- Attach provenance to memories: which tool call, which project, which agent, which approval.

### With `symscope`

- Use local inventory to discover exposed services, ports, MCP servers, and dev tools.
- Detect when an agent could reach local services that should not be exposed.
- Correlate MCP tool risk with actual local attack surface.
- Use `symscope` as the remote inventory probe when `symguard` connects to another
  machine through SSH/Tailscale/LAN.
- Reuse `symscope`'s port/MCP/container snapshot instead of duplicating inventory logic.

### With `symseek`

- Index audit reports, policy docs, and security findings.
- Search previous incidents and approvals.
- Retrieve project-specific security documentation before allowing risky actions.

### With `symfetch`

- Fetch security advisories, CVEs, MCP registry metadata, and tool documentation.
- Verify remote MCP server docs before trusting them.

### With `symingest`

- Ingest PDF/security policy documents and turn them into local policy context.
- Extract compliance requirements from customer documents.

### With `symvibe`

- Insert approval checkpoints into autonomous coding cycles.
- Gate risky steps: push, release, deploy, destructive cleanup, dependency upgrade, secret rotation.
- Surface blocked actions as workflow status instead of letting agents hang.

### With `symdesk`

- `symdesk` can become the GUI for `symguard`:
  - approval inbox,
  - policy editor,
  - tool trust registry,
  - audit timeline,
  - risk dashboard.

### Scope

`symguard` stays local-first and self-hosted, and that is the whole product —
there is no Pro edition to hand features to.

Deliberately out of scope: team policy management, centralized audit evidence
retention, SSO/RBAC, tenant-scoped policies, compliance exports, and a managed
MCP gateway for organizations.

---

## 9. Non-Goals

`symaira-guard` should **not** become:

- a chat frontend,
- a replacement for `symvault`,
- a replacement for `symmemory`,
- a Tailscale/NetBird/Octelium/WireGuard clone,
- a custom VPN daemon,
- a DNS/MagicDNS/routing platform,
- a NAT traversal or DERP/STUN/TURN infrastructure project,
- a full SIEM,
- a cloud-only policy SaaS,
- a generic endpoint protection platform,
- a monolithic orchestration engine.

Keep it narrow:

> classify agent tool calls, enforce local policy, ask the human when needed, and record what happened.

For remote access, keep it equally narrow:

> reuse existing secure transports, then enforce agent/tool policy above them.

---

## 10. Remote Access Research Handoff

This section absorbs the "Tailscale alternative for AI agents" research so the
separate research folder can be deleted without losing the useful product
decisions.

### Market Reality

The research found that a full mesh-VPN is not a clean Symaira opportunity right now:

| Product / pattern | What to learn | Why not copy directly |
|---|---|---|
| Tailscale Aperture | Aggregates remote MCP servers, grants, dynamic registration, one MCP endpoint | Centralized gateway, alpha, still built around Tailscale identity/control plane |
| NetBird | Solid FOSS WireGuard control plane, UI, SSO, AI workload positioning | Device/network-centric; MCP is add-on ecosystem, not local agent policy core |
| Octelium | ZTNA, MCP gateway, AI gateway, L7-aware policy, secretless access | Enterprise/security-platform scope; too heavy for Symaira local-first MVP |
| Wire / MeshPOP / mpop-style tools | AI-managed WireGuard topology and orchestration | Still mostly infrastructure orchestration; high maintenance and unclear Symaira moat |
| Tailscale/NetBird MCP servers | Map network APIs to AI tools | Useful for management, but they do not solve local human approval/trust boundary |

Conclusion:

```text
Do not build "Symaira Network" as a Tailscale alternative now.
Build agent-aware remote MCP access inside symguard, reusing existing transports.
Only split into a dedicated symconnect/symnet tool if dogfooding proves the layer
is valuable and too large for symguard.
```

### What `symguard` Should Take From The Research

1. **Agent-first principals**
   - Treat agents/runs as identities, not just users/devices.
   - Support TTL and scoped grants.

2. **MCP as the controlled interface**
   - Remote access should expose approved tools/resources, not broad network reachability.
   - Tool lists are filtered by policy before the agent sees them.

3. **Semantic policies**
   - Match on tool/action/sensitivity/project, not just host/port.
   - Default deny for remote writes, secrets, destructive actions, and production changes.

4. **Provider-based transport reuse**
   - Use Tailscale, SSH, or LAN/mDNS as providers.
   - Avoid owning WireGuard/NAT/DNS until there is overwhelming evidence.

5. **Dynamic discovery with pinning**
   - Discover remote MCP tools dynamically.
   - Prefix names, classify risk, pin schemas, detect drift.
   - Discovery never equals trust.

6. **Explainable networking for agents**
   - Explain policy/transport/MCP failures in a structure an LLM can consume.
   - Provide dry-run simulation before risky actions.

7. **Audit and provenance**
   - Every remote call gets a local audit event.
   - Store high-level summaries in `symmemory` where useful.
   - Keep raw secrets and sensitive output out of logs.

8. **Developer/Homelab-first UX**
   - One local binary.
   - TOML config.
   - JSON-first output.
   - No mandatory account.
   - Works with Daniel-style MacBook ↔ Mac Mini setups before pretending to be enterprise.

### Naming Guidance

Avoid promising a network replacement.

Bad early names:

```text
symaira-network
symnet
Symaira Mesh VPN
```

Better feature language inside `symguard`:

```text
remote MCP access
agent access gateway
remote tool policy
explainable agent networking
```

Only if it later becomes a separate tool, prefer:

```text
symconnect
```

That name says "connect controlled things" rather than "we rebuilt Tailscale".
Less ego, less maintenance debt. A rare combo.

---

## 11. Initial CLI Shape

```bash
symguard version
symguard doctor

symguard scan
symguard scan --client hermes
symguard scan --client claude-desktop
symguard scan --format json

symguard pin list
symguard pin approve <server> <tool>
symguard pin diff <server> <tool>

symguard policy init
symguard policy check --server <name> --tool <tool> --args args.json

symguard proxy --upstream <server-name>
symguard proxy --config ~/.config/symguard/config.toml

symguard remote list
symguard remote add macmini --via ssh --host macmini
symguard remote add studio --via tailscale --host studio.tailnet-name.ts.net
symguard remote doctor macmini

symguard mcp discover --remote macmini
symguard proxy --remote macmini --upstream symmemory

symguard agent register --name build-bot --role ci_runner --ttl 2h
symguard sandbox create --agent build-bot --allow symmemory:memory_search

symguard diag path --from agent:build-bot --to mcp:symmemory@macmini
symguard diag explain --event <event-id>

symguard audit list
symguard audit show <event-id>
symguard audit export --format jsonl
```

---

## 12. MVP Definition

A useful MVP is **not** a perfect AI security framework.

A useful MVP is:

1. finds local MCP servers,
2. lists tools and risky capabilities,
3. detects schema/tool-description drift,
4. enforces simple allow/ask/deny policies,
5. proxies at least one stdio MCP server,
6. writes a clean audit trail,
7. integrates optionally with `symvault` and `symmemory`.

If it does those seven things, it earns its repo.

Remote access is **not** required for the first MVP. It becomes worth building
after local policy/proxy/audit works. Otherwise remote access just multiplies
the blast radius of an immature policy engine. Charming in a demo, terrible at
2 a.m.

---

## 13. Product Positioning

Possible tagline:

> **Agent security without giving up local control.**

Alternative:

> **A firewall for AI tool calls.**

More Symaira-native:

> **Human control for agent autonomy.**

The strongest positioning is probably:

> `symguard` is the local policy and approval layer for MCP and agentic tools.

With the remote-access roadmap included:

> `symguard` lets agents use local and remote tools without giving up local human control.

That is boring in the right way. Boring security sells better than sci-fi security.
