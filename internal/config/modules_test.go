package config

import (
	"testing"
)

func TestModules_DefaultDisabledAndDoNotSelectManagedBinaries(t *testing.T) {
	m := Defaults().Modules
	if m.Browse || m.Operate || m.Scope {
		t.Fatalf("defaults enabled optional module: %+v", m)
	}
	if got := m.EnabledCores(); len(got) != 0 {
		t.Fatalf("default optional modules selected managed binaries: %#v", got)
	}
}

func TestModules_ExplicitSelectionKeepsScopeOperateOutOfInstallerSelection(t *testing.T) {
	m := ModulesConfig{Browse: true, Operate: true, Scope: true}
	if got := m.EnabledCores(); len(got) != 1 || !got["symbrowse"] {
		t.Fatalf("scope/operate unexpectedly became install targets: %#v", got)
	}
}
