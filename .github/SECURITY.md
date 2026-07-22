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

See the [Security notes](../README.md#security-notes) section of the
README for what `symbrain` protects against by design (least exposure at
the profile/capability level) and what it explicitly does not protect
against (call-time policy enforcement, which is
[`symguard`](https://github.com/danieljustus/symaira-guard)'s job).
