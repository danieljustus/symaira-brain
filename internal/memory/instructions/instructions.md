# Symaira Memory — Agent Integration Guide

Symaira Memory (`symmemory`) is a persistent semantic database for developer preferences, codebase guidelines, and session context. It exposes four MCP tools over stdio JSON-RPC 2.0.

## Tools

| Tool | Purpose |
|---|---|
| `memory_search` | Semantic vector similarity search across stored memories |
| `memory_set` | Store a new persistent memory or fact (requires `kind`; `staged: true` holds the fact for review) |
| `memory_candidates` | List staged candidate memories awaiting review |
| `memory_promote` | Approve a staged candidate so it becomes retrievable |
| `memory_reject` | Discard a staged candidate |
| `memory_get` | Retrieve a specific memory by its UUID |
| `memory_list` | List all memories, optionally filtered by scope |

## When to Use `memory_search`

- **Session start**: At the beginning of every session or task, search for relevant context using key terms (e.g., "code style", "database settings", "language preference").
- **Before decisions**: When about to make an architectural or design choice, search for prior decisions to avoid contradicting established patterns.
- **Fact lookup**: When the user references something that may have been discussed before, search to retrieve the original context.

## Search Budget

Retrieval is bounded: `memory_search`, `memory_get`, and `memory_list` combined may be called **at most three times per turn**.

- After a miss, retry **once** with different keywords or a different tool — still inside the budget.
- Once the budget is spent, **stop searching** and answer from the context you already have; do not rephrase the same query repeatedly.
- A store that does not contain a fact will not yield it no matter how the question is phrased.

## When to Use `memory_set`

Autonomously store memories when the user expresses persistent information. Do not ask for permission — but pass `staged: true` when a fact was derived autonomously without explicit user confirmation, so it is held as a candidate for review instead of silently entering the live store.

**Always classify the fact with `kind`** — `memory_set` requires it:
- `user` — preferences and personal facts: "User prefers TypeScript for scripting tasks."
- `feedback` — corrections and evaluations: "User corrected the JSON schema: enums must be lowercase."
- `project` — rules, constraints, architectural decisions: "API daemon runs on port 8787.", "Shared services must remain free of private commercial logic.", "No CGO dependencies in the build."
- `reference` — external facts and documentation: "PostgreSQL 16 default port is 5432."

**Store these:**
- User preferences: "User prefers TypeScript for scripting tasks."
- Project rules: "API daemon runs on port 8787."
- Architectural decisions: "Shared services must remain free of private commercial logic."
- Constraints: "No CGO dependencies in the build."

**Do NOT store:**
- Temporary debug states ("Fixed bug on line 42").
- Large raw logs, code blocks, or error tracebacks.
- Conversational filler.

## Scope Selection

| Scope | When to use |
|---|---|
| `project` | **Default choice.** Any fact specific to the current codebase or repository. Auto-resolves project name from `.symmemory.toml` or `.git` in parent directories. |
| `global` | General user preferences that apply across all projects (e.g., "User prefers tabs over spaces"). |
| `agent` | Facts specific to a particular AI agent or tool. |
| `user` | Facts tied to a specific human user identity. |
| `session` | Ephemeral facts relevant only to the current conversation. |

## Entity Tagging

Mention relevant entity names (people, services, projects) directly in the memory content so they become searchable. For example: "The **auth-service** uses JWT tokens signed by **symmemory**."

## Provenance Tracking

Pass `session_id` when calling `memory_set` to record which conversation session created the memory. This enables auditing and traceability.

## Fact Formatting

Write facts as objective, third-person, declarative statements:
- **Good**: "User prefers tabs over spaces in Go source files."
- **Bad**: "I need to use tabs."
- **Good**: "Project backend uses SQLite with WAL mode."
- **Bad**: "We enabled WAL mode."

## Profile & Multi-Agent Setup

Symaira Memory supports role-based access control through **profiles**. Each profile has a name, role (`read`, `readwrite`, `admin`), and type (`agent` or `human`). When a profile is active, the server enforces its role on all operations — read-only profiles cannot write memories.

### Why Profiles Matter

When multiple AI agents share the same Symaira Memory instance, profiles ensure:
- **Least-privilege access**: A monitoring agent can search without modifying data.
- **Audit trails**: Each memory can be traced to the agent profile that created it.
- **Conflict prevention**: Multiple agents writing simultaneously is safe when scoped correctly.

### Creating Profiles

```bash
# Create a readwrite profile for your primary coding agent
symmemory profile add claude-code --role readwrite --type agent --description "Claude Code coding agent"

# Create a read-only profile for a monitoring or search-only agent
symmemory profile add opencode --role read --type agent --description "OpenCode read-only access"
```

### Using Profiles at Serve Time

Pass the `--profile` flag when starting the server, or set the `SYMMEMORY_PROFILE` environment variable:

```bash
# Start the MCP server with a specific profile
symmemory serve --profile claude-code

# Or via environment variable
SYMMEMORY_PROFILE=opencode symmemory serve
```

### Complete Examples

**Claude Desktop** — full readwrite access:

```json
{
  "mcpServers": {
    "symaira-memory": {
      "command": "symmemory",
      "args": ["serve", "--profile", "claude-code"]
    }
  }
}
```

**OpenCode** — read-only access (cannot write memories):

```json
{
  "mcpServers": {
    "symaira-memory": {
      "command": "symmemory",
      "args": ["serve", "--profile", "opencode"]
    }
  }
}
```

### Role Summary

| Role | `memory_search` | `memory_set` | `memory_get` | `memory_list` |
|------|-----------------|--------------|--------------|---------------|
| `read` | ✅ | ❌ | ✅ | ✅ |
| `readwrite` | ✅ | ✅ | ✅ | ✅ |
| `admin` | ✅ | ✅ | ✅ | ✅ |

### Profile Management

```bash
# List all profiles
symmemory profile list

# Change a profile's role
symmemory profile set-role opencode readwrite

# Remove a profile
symmemory profile remove opencode
```

## Memory Consolidation

When the user updates a prior decision (e.g., "Switch from SQLite to PostgreSQL"):
1. Run `memory_search` to find the stale memory.
2. Note the old memory ID so it can be deleted or updated.
