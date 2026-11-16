# Security Policy

## Supported Versions

Only the latest released version of `symbrain` receives security fixes.
There is no long-term support branch — upgrade to the latest release to
get a fix.

| Version | Supported |
| ------- | --------- |
| Latest release | :white_check_mark: |
| Older releases | :x: |

## Reporting a Vulnerability

Please report security vulnerabilities privately, not through a public
GitHub issue.

- Preferred: use [GitHub Security Advisories](https://github.com/danieljustus/symaira-brain/security/advisories/new)
  for this repository.
- Alternative: contact the maintainer directly.

Please include:

- A description of the vulnerability and its potential impact.
- Steps to reproduce, or a minimal proof of concept.
- The affected version/commit.

You should expect an initial response within a few days. Once a fix is
available, it will ship in a new release and the advisory will be
credited to you unless you prefer to remain anonymous.

## Scope

Reports are in scope for every module this repository ships. Each module
has its own security boundary:

### Gateway (`symbrain mcp`)

The gateway's job is least exposure at the profile/capability level. In
scope: a harness seeing or calling a tool, server or vault mode that its
profile does not expose; secret material escaping redaction into the audit
log. See the [Security notes](../README.md#security-notes) section of the
README for the full model.

### Browse (`symbrowse`)

The Browse module renders pages written by others, so page content is
untrusted. In scope:

- hostile page content reaching the agent outside the prompt-injection
  boundary ([browse/docs/injection.md](../browse/docs/injection.md));
- a bypass of the SSRF guard ([browse/docs/ssrf.md](../browse/docs/ssrf.md))
  or of the domain allowlist ([browse/docs/allowlist.md](../browse/docs/allowlist.md));
- secrets escaping redaction into the action journal, traces or command
  output;
- cookies or storage state leaking from one session into another;
- takeover of a session while it is handed over to a human (2FA, CAPTCHA,
  approval).

### Operate (`symoperate`)

In scope: any action performed outside the permission set granted with
`symoperate serve --grant` or the host's macOS privacy permissions.

### Explicit non-goals

- Brain performs no call-time approval, risk scoring or human confirmation
  for the tools a profile does expose. That is the guard module's job
  (`symbrain guard`, absorbed into this repository); report guard bypasses
  under the guard module.
- A remote `cdp_endpoint` configured by the operator is trusted by that
  operator. Behavior of the browser behind it is out of scope.
- A malicious or compromised harness process abusing tools its profile
  legitimately exposes.
