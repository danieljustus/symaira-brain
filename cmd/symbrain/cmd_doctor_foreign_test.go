package main

import (
	"testing"
)

func TestCheckForeignAccessRisks_ReadWithNoToolsReadIsFlagged(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)

	writeProfileFile(t, home, "personal", `
[profile]
name = "personal"

[servers.docs]
enabled = true
command = "/usr/local/bin/docs-mcp"
access  = "read"
`)

	got := checkForeignAccessRisks()
	if len(got) != 1 {
		t.Fatalf("checkForeignAccessRisks() = %+v, want 1 risk", got)
	}
	if got[0].Profile != "personal" || got[0].Server != "docs" {
		t.Errorf("got %+v, want profile=personal server=docs", got[0])
	}
}

func TestCheckForeignAccessRisks_ReadWithToolsReadIsNotFlagged(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)

	writeProfileFile(t, home, "personal", `
[profile]
name = "personal"

[servers.docs]
enabled    = true
command    = "/usr/local/bin/docs-mcp"
access     = "read"
tools_read = ["search"]
`)

	if got := checkForeignAccessRisks(); len(got) != 0 {
		t.Errorf("checkForeignAccessRisks() = %+v, want none (tools_read override present)", got)
	}
}

func TestCheckForeignAccessRisks_WriteAccessIsNotFlagged(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)

	writeProfileFile(t, home, "personal", `
[profile]
name = "personal"

[servers.docs]
enabled = true
command = "/usr/local/bin/docs-mcp"
`)

	if got := checkForeignAccessRisks(); len(got) != 0 {
		t.Errorf("checkForeignAccessRisks() = %+v, want none (default access is write, not read)", got)
	}
}

func TestCheckForeignAccessRisks_CoreAliasIsNeverFlagged(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)

	writeProfileFile(t, home, "personal", `
[profile]
name = "personal"

[servers.vault]
enabled = true
mode    = "full"
`)

	if got := checkForeignAccessRisks(); len(got) != 0 {
		t.Errorf("checkForeignAccessRisks() = %+v, want none (core aliases have no access field)", got)
	}
}

func TestCheckForeignAccessRisks_DisabledServerIsNotFlagged(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)

	writeProfileFile(t, home, "personal", `
[profile]
name = "personal"

[servers.docs]
enabled = false
command = "/usr/local/bin/docs-mcp"
access  = "read"
`)

	if got := checkForeignAccessRisks(); len(got) != 0 {
		t.Errorf("checkForeignAccessRisks() = %+v, want none (disabled server)", got)
	}
}

func TestCheckForeignAccessRisks_NoProfilesReturnsNil(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)

	if got := checkForeignAccessRisks(); len(got) != 0 {
		t.Errorf("checkForeignAccessRisks() with no profiles = %v, want empty", got)
	}
}
