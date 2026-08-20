package config

import "testing"

func TestVCSEnabled(t *testing.T) {
	if !(&Config{}).VCSEnabled() {
		t.Error("nil VCS config must default to enabled")
	}
	on := true
	if !(&Config{VCS: VCSConfig{Enabled: &on}}).VCSEnabled() {
		t.Error("explicit true must be enabled")
	}
	off := false
	if (&Config{VCS: VCSConfig{Enabled: &off}}).VCSEnabled() {
		t.Error("explicit false must be disabled")
	}
}
