package policy

import (
	"testing"
)

func TestMarginalCapabilityCheck_NoAllowed(t *testing.T) {
	if MarginalCapabilityCheck("read_file", nil) {
		t.Error("expected false when no capabilities are allowed")
	}
	if MarginalCapabilityCheck("read_file", map[string]bool{}) {
		t.Error("expected false when allowed set is empty")
	}
}

func TestMarginalCapabilityCheck_SameEffect(t *testing.T) {
	// Two read_file capabilities — same effect
	if !MarginalCapabilityCheck("read_private", map[string]bool{"read_private": true}) {
		t.Error("expected true for same capability")
	}
}

func TestMarginalCapabilityCheck_ShellSuperset(t *testing.T) {
	// Shell encompasses read_private and write_file
	if !MarginalCapabilityCheck("read_private", map[string]bool{"shell": true}) {
		t.Error("shell should encompass read_private")
	}
	if !MarginalCapabilityCheck("write_file", map[string]bool{"shell": true}) {
		t.Error("shell should encompass write_file")
	}
}

func TestMarginalCapabilityCheck_ShellNoSupersetForSecret(t *testing.T) {
	// Shell does NOT encompass read_secret capabilities
	if MarginalCapabilityCheck("read_secret", map[string]bool{"shell": true}) {
		t.Error("shell should not encompass read_secret")
	}
}

func TestMarginalCapabilityCheck_NetworkSuperset(t *testing.T) {
	if !MarginalCapabilityCheck("read_public", map[string]bool{"network": true}) {
		t.Error("network should encompass read_public")
	}
}

func TestMarginalCapabilityCheck_UnknownCap(t *testing.T) {
	if MarginalCapabilityCheck("unknown_cap", map[string]bool{"shell": true}) {
		t.Error("unknown capability should not be capped")
	}
}

func TestClassifyRisk_Static(t *testing.T) {
	tests := []struct {
		cap   string
		level RiskLevel
	}{
		{"read_public", RiskLow},
		{"read_private", RiskMedium},
		{"read_secret", RiskCritical},
		{"write_file", RiskMedium},
		{"shell", RiskHigh},
		{"network", RiskMedium},
		{"browser", RiskHigh},
		{"credential_use", RiskHigh},
		{"deploy", RiskCritical},
		{"destructive", RiskCritical},
		{"unknown", RiskUnknown},
	}
	for _, tt := range tests {
		t.Run(tt.cap, func(t *testing.T) {
			got := ClassifyRisk(tt.cap, false)
			if got != tt.level {
				t.Errorf("ClassifyRisk(%q, false) = %v, want %v", tt.cap, got, tt.level)
			}
		})
	}
}

func TestClassifyRisk_MarginalCapping(t *testing.T) {
	tests := []struct {
		name     string
		cap      string
		marginal bool
		want     RiskLevel
	}{
		{"shell capped to low", "shell", true, RiskLow},
		{"read_secret capped to low", "read_secret", true, RiskLow},
		{"deploy capped to low", "deploy", true, RiskLow},
		{"read_public stays low", "read_public", true, RiskLow},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := ClassifyRisk(tt.cap, tt.marginal)
			if got != tt.want {
				t.Errorf("ClassifyRisk(%q, %v) = %v, want %v", tt.cap, tt.marginal, got, tt.want)
			}
		})
	}
}

func TestClassifyRisk_NoCappingWithoutMarginal(t *testing.T) {
	if got := ClassifyRisk("shell", false); got != RiskHigh {
		t.Errorf("ClassifyRisk(shell, false) = %v, want RiskHigh", got)
	}
}

func TestMarginalCapabilityCheck_CredentialUseSuperset(t *testing.T) {
	// Credential use encompasses read_secret
	if !MarginalCapabilityCheck("read_secret", map[string]bool{"credential_use": true}) {
		t.Error("credential_use should encompass read_secret")
	}
}

func TestMarginalCapabilityCheck_DeploySuperset(t *testing.T) {
	// Deploy encompasses write_file and network
	if !MarginalCapabilityCheck("write_file", map[string]bool{"deploy": true}) {
		t.Error("deploy should encompass write_file")
	}
	if !MarginalCapabilityCheck("network", map[string]bool{"deploy": true}) {
		t.Error("deploy should encompass network")
	}
}

func TestMarginalCapabilityCheck_DestructiveSuperset(t *testing.T) {
	if !MarginalCapabilityCheck("write_file", map[string]bool{"destructive": true}) {
		t.Error("destructive should encompass write_file")
	}
}

func TestMarginalCapabilityCheck_ReadSecretNotCappedByShell(t *testing.T) {
	// Verify inverse: read_secret is NOT encompassed by shell
	if MarginalCapabilityCheck("read_secret", map[string]bool{"shell": true}) {
		t.Error("read_secret should not be capped by shell")
	}
	// But it IS encompassed by credential_use
	if !MarginalCapabilityCheck("read_secret", map[string]bool{"credential_use": true}) {
		t.Error("read_secret should be capped by credential_use")
	}
}

// FuzzMarginalCapabilityCheck tests that arbitrary capability names
// don't cause panics during marginal capability evaluation.
func FuzzMarginalCapabilityCheck(f *testing.F) {
	seeds := []string{"shell", "read_private", "read_secret", "", "\x00"}
	for _, s := range seeds {
		f.Add(s, "shell")
		f.Add(s, "")
		f.Add(s, "\x00")
	}

	f.Fuzz(func(t *testing.T, toolCap, allowed string) {
		_ = MarginalCapabilityCheck(toolCap, map[string]bool{allowed: true})
		_ = MarginalCapabilityCheck(toolCap, nil)
	})
}
