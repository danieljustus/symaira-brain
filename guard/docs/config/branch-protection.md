# Branch Protection Configuration

## Classic Branch Protection (main)

Enabled via GitHub API with the following settings:

- **Required pull request reviews**: 0 approvals (solo repo — author cannot self-approve)
- **Required conversation resolution**: enabled
- **Allow force pushes**: disabled
- **Allow deletions**: disabled
- **Enforce admins**: disabled (admin bypass available)

## Repository Ruleset (id: 18178325)

Enabled the existing "main protection" ruleset:

- **Enforcement**: active
- **Rules**: deletion prevention, non-fast-forward prevention
- **Target**: refs/heads/main

## Rationale

For a solo repository, requiring approving reviews would create a self-blocking
scenario (GitHub won't count the author's own approval). Instead, conversation
resolution and required PRs serve as merge gates.
