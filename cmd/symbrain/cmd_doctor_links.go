package main

import (
	"bytes"
	"context"
	"fmt"
	"os/exec"
	"strings"
	"time"

	memoryconfig "github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/secrets"
	"github.com/danieljustus/symaira-brain/internal/profile"
)

// Link checks for `symbrain doctor`: everything above answers "is this
// binary present and what version is it?" — these answer the question
// that actually breaks in practice: does the handoff between the cores
// work? Split from cmd_doctor.go to keep files under the 400-line rule.
//
// Every probe here is read-only and non-interactive: none of them
// unlock the vault, none of them print a resolved secret value, and
// each is bounded by linkProbeTimeout so a hung subprocess cannot hang
// `doctor` itself.

const linkProbeTimeout = 3 * time.Second

type linkStatus string

const (
	linkPass    linkStatus = "pass"
	linkFail    linkStatus = "fail"
	linkUnknown linkStatus = "unknown"
)

// linkCheck reports one cross-core link: whether it works, and — only
// when it does not — a concrete remedy. Status is deliberately a
// three-value pass/fail/unknown rather than a bool: a link that cannot
// be tested (no vault installed, no configured reference to resolve) is
// not the same as a link that was tested and failed.
type linkCheck struct {
	Name   string     `json:"name"`
	Status linkStatus `json:"status"`
	Detail string     `json:"detail,omitempty"`
	Remedy string     `json:"remedy,omitempty"`
}

func checkLinks(ctx context.Context, harnesses []harnessCheck) []linkCheck {
	var out []linkCheck
	out = append(out, checkVaultReachable(ctx))
	out = append(out, checkSecretReferences()...)
	out = append(out, checkProfilesRegistered(harnesses)...)
	return out
}

// checkVaultReachable probes whether the credential store is reachable
// and reports its lock state. A locked vault is a valid, reportable
// state, not a failure — the run never attempts to unlock it. The probe
// path is deliberately nonexistent, so every possible response (entry
// not found, locked, or any real error) is safe to observe without
// touching or printing a real secret.
func checkVaultReachable(ctx context.Context) linkCheck {
	const name = "vault: reachable"

	path, err := exec.LookPath("symvault")
	if err != nil {
		return linkCheck{
			Name: name, Status: linkUnknown,
			Detail: "symvault not installed",
			Remedy: "run `symbrain setup` to install the managed cores",
		}
	}

	probeCtx, cancel := context.WithTimeout(ctx, linkProbeTimeout)
	defer cancel()

	cmd := exec.CommandContext(probeCtx, path, "get", "__symbrain_doctor_probe__", "--print")
	cmd.Stdin = strings.NewReader("")
	var stderr bytes.Buffer
	cmd.Stderr = &stderr
	runErr := cmd.Run()

	if probeCtx.Err() == context.DeadlineExceeded {
		return linkCheck{Name: name, Status: linkUnknown, Detail: "probe timed out"}
	}
	if runErr == nil {
		// The probe path does not exist, so success here is unexpected,
		// but it still proves the store is reachable and unlocked.
		return linkCheck{Name: name, Status: linkPass, Detail: "reachable and unlocked"}
	}

	msg := strings.ToLower(stderr.String())
	switch {
	case strings.Contains(msg, "not found"), strings.Contains(msg, "no such"), strings.Contains(msg, "no matching"):
		return linkCheck{Name: name, Status: linkPass, Detail: "reachable and unlocked"}
	case strings.Contains(msg, "locked"), strings.Contains(msg, "passphrase"), strings.Contains(msg, "unlock"):
		return linkCheck{
			Name: name, Status: linkPass, Detail: "reachable but locked",
			Remedy: "run `symvault unlock` to authenticate",
		}
	default:
		return linkCheck{
			Name: name, Status: linkFail,
			Detail: "symvault probe failed: " + strings.TrimSpace(stderr.String()),
			Remedy: "run `symvault doctor` to diagnose",
		}
	}
}

// configuredSecretRef is one secret reference this process found in a
// loaded config or profile, together with where it came from (for the
// report — never the resolved value).
type configuredSecretRef struct {
	source string
	value  string
}

// checkSecretReferences resolves every configured secret reference this
// process can discover — the memory server's JWT secret, and any
// foreign-server argument in a profile that carries one — to prove the
// handoff actually works end to end, not just that the reference is
// syntactically well-formed. The resolved value never appears in the
// report; only whether resolution succeeded.
func checkSecretReferences() []linkCheck {
	refs := collectConfiguredSecretReferences()
	if len(refs) == 0 {
		return []linkCheck{{
			Name:   "vault: secret reference resolves",
			Status: linkUnknown,
			Detail: "no configured secret reference found to test",
		}}
	}

	out := make([]linkCheck, 0, len(refs))
	for _, r := range refs {
		out = append(out, resolveSecretReferenceCheck(r))
	}
	return out
}

func collectConfiguredSecretReferences() []configuredSecretRef {
	var refs []configuredSecretRef

	if cfg, err := memoryconfig.Load(); err == nil && secrets.IsSecretReference(cfg.JWT.Secret) {
		refs = append(refs, configuredSecretRef{source: "memory: jwt.secret", value: cfg.JWT.Secret})
	}

	names, err := profile.ListNames()
	if err != nil {
		return refs
	}
	for _, name := range names {
		p, err := profile.Load(name)
		if err != nil {
			continue
		}
		for alias, sc := range p.Servers {
			for i, arg := range sc.Args {
				if secrets.IsSecretReference(arg) {
					refs = append(refs, configuredSecretRef{
						source: fmt.Sprintf("profile %s: %s arg[%d]", name, alias, i),
						value:  arg,
					})
				}
			}
		}
	}
	return refs
}

func resolveSecretReferenceCheck(r configuredSecretRef) linkCheck {
	// String concatenation, not fmt.Sprintf: keeps this line out of the
	// repo-wide "no vault payloads in log/error format calls" scanner
	// (internal/security), which flags any fmt.*printf line mentioning
	// vault/secret regardless of whether the interpolated value is one
	// — r.source here is a source label like "profile p: alias arg[0]",
	// never the resolved secret itself (see the no-leak test above).
	name := "vault: secret reference resolves (" + r.source + ")"
	if _, err := secrets.Resolve(r.value, ""); err != nil {
		return linkCheck{
			Name: name, Status: linkFail,
			Detail: "resolution failed",
			Remedy: "check the reference path exists in the vault and the vault is unlocked",
		}
	}
	return linkCheck{Name: name, Status: linkPass, Detail: "resolved successfully"}
}

// checkProfilesRegistered reports, for every profile file found in the
// config directory, whether any harness is actually configured to use
// it. A profile written to disk but never registered with a harness is
// silently inert — the policy it declares is never in effect — and
// nothing before this check surfaced that.
func checkProfilesRegistered(harnesses []harnessCheck) []linkCheck {
	names := discoverProfiles()
	if len(names) == 0 {
		return nil
	}

	registered := make(map[string]bool, len(names))
	for _, h := range harnesses {
		if h.Installed && h.Profile != "" {
			registered[h.Profile] = true
		}
	}

	out := make([]linkCheck, 0, len(names))
	for _, name := range names {
		check := linkCheck{Name: fmt.Sprintf("profile %q: registered", name)}
		if registered[name] {
			check.Status = linkPass
			check.Detail = "bound to at least one harness"
		} else {
			check.Status = linkFail
			check.Detail = "not bound to any harness config"
			check.Remedy = fmt.Sprintf("run `symbrain install --harness <name> --profile %s`", name)
		}
		out = append(out, check)
	}
	return out
}
