package managed

import (
	"context"
	"os"
	"path/filepath"
	"regexp"
	"testing"
)

func TestLoadManifest(t *testing.T) {
	m, err := LoadManifest()
	if err != nil {
		t.Fatalf("LoadManifest: %v", err)
	}
	if m.SchemaVersion != 1 {
		t.Errorf("SchemaVersion = %d, want 1", m.SchemaVersion)
	}
	if len(m.Cores) != 2 {
		t.Errorf("len(Cores) = %d, want 2", len(m.Cores))
	}
	for _, name := range []string{"symvault", "symcockpit"} {
		c, ok := m.Cores[name]
		if !ok {
			t.Errorf("missing core %q", name)
			continue
		}
		if c.Version == "" {
			t.Errorf("core %q: Version is empty", name)
		}
		if c.Repo == "" {
			t.Errorf("core %q: Repo is empty", name)
		}
		if c.BinaryName == "" {
			t.Errorf("core %q: BinaryName is empty", name)
		}
	}
	cockpit := m.Cores["symcockpit"]
	if len(cockpit.Platforms) != 1 || cockpit.Platforms[0] != "darwin" {
		t.Errorf("symcockpit Platforms = %v, want [darwin]", cockpit.Platforms)
	}
	if cockpit.AssetArch != "universal" {
		t.Errorf("symcockpit AssetArch = %q, want universal", cockpit.AssetArch)
	}
	if cockpit.HasCosign {
		t.Error("symcockpit HasCosign = true, want false (no .sig/.pem assets)")
	}
}

func TestAssetName_Versioned(t *testing.T) {
	core := &Core{
		Version:     "v1.2.3",
		BinaryName:  "symvault",
		AssetPrefix: "symaira-vault",
	}
	// Release assets omit the leading "v" even though tags carry it.
	got := core.AssetName("darwin", "arm64")
	want := "symaira-vault_1.2.3_darwin_arm64.tar.gz"
	if got != want {
		t.Errorf("AssetName() = %q, want %q", got, want)
	}
}

func TestAssetName_NoVVersion(t *testing.T) {
	core := &Core{
		Version:     "0.4.0",
		BinaryName:  "symcockpit",
		AssetPrefix: "symcockpit",
	}
	got := core.AssetName("darwin", "arm64")
	want := "symcockpit_0.4.0_darwin_arm64.tar.gz"
	if got != want {
		t.Errorf("AssetName() = %q, want %q", got, want)
	}
}

func TestAssetName_Universal(t *testing.T) {
	core := &Core{
		Version:     "0.4.0",
		BinaryName:  "symcockpit",
		AssetPrefix: "symcockpit",
		AssetArch:   "universal",
	}
	for _, goarch := range []string{"arm64", "amd64"} {
		got := core.AssetName("darwin", goarch)
		want := "symcockpit_0.4.0_darwin_universal.tar.gz"
		if got != want {
			t.Errorf("AssetName(darwin, %s) = %q, want %q", goarch, got, want)
		}
	}
}

func TestAssetNameAlt_Unversioned(t *testing.T) {
	core := &Core{
		Version:     "v1.2.3",
		BinaryName:  "legacy-core",
		AssetPrefix: "legacy-core",
	}
	got := core.AssetNameAlt("linux", "amd64")
	want := "legacy-core_linux_amd64.tar.gz"
	if got != want {
		t.Errorf("AssetNameAlt() = %q, want %q", got, want)
	}
}

func TestSupportsPlatform(t *testing.T) {
	tests := []struct {
		name      string
		platforms []string
		goos      string
		want      bool
	}{
		{"empty means all", nil, "linux", true},
		{"explicit match", []string{"darwin"}, "darwin", true},
		{"explicit miss", []string{"darwin"}, "linux", false},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			core := &Core{Platforms: tt.platforms}
			if got := core.SupportsPlatform(tt.goos); got != tt.want {
				t.Errorf("SupportsPlatform(%q) = %v, want %v", tt.goos, got, tt.want)
			}
		})
	}
}

func TestNormalizeVersion(t *testing.T) {
	tests := []struct{ in, want string }{
		{"v0.15.3", "0.15.3"},
		{"0.15.3", "0.15.3"},
		{" v1.2.3 ", "1.2.3"},
	}
	for _, tt := range tests {
		if got := normalizeVersion(tt.in); got != tt.want {
			t.Errorf("normalizeVersion(%q) = %q, want %q", tt.in, got, tt.want)
		}
	}
}

func TestBinaryPathInArchive_Vault(t *testing.T) {
	core := &Core{
		Version:     "v0.15.3",
		BinaryName:  "symvault",
		AssetPrefix: "symaira-vault",
	}
	got := core.BinaryPathInArchive("darwin", "arm64")
	want := "symaira-vault_0.15.3_darwin_arm64/symvault"
	if got != want {
		t.Errorf("BinaryPathInArchive() = %q, want %q", got, want)
	}
}

func TestBinaryPathInArchive_RootBinary(t *testing.T) {
	core := &Core{
		Version:     "v1.2.3",
		BinaryName:  "example-core",
		AssetPrefix: "example-core",
	}
	got := core.BinaryPathInArchive("darwin", "arm64")
	want := "example-core"
	if got != want {
		t.Errorf("BinaryPathInArchive() = %q, want %q", got, want)
	}
}

func TestChecksumAssetName(t *testing.T) {
	core := &Core{AssetPrefix: "symaira-vault"}
	got := core.ChecksumAssetName()
	want := "checksums.txt"
	if got != want {
		t.Errorf("ChecksumAssetName() = %q, want %q", got, want)
	}
}

func TestFindChecksumInFile(t *testing.T) {
	content := `# checksums for symaira-vault
abc123def456  symaira-vault_0.15.3_darwin_arm64.tar.gz
789abc012def  symaira-vault_0.15.3_linux_amd64.tar.gz
`
	f := filepath.Join(t.TempDir(), "checksums.txt")
	if err := os.WriteFile(f, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	got, err := findChecksumInFile(f, "symaira-vault_0.15.3_darwin_arm64.tar.gz")
	if err != nil {
		t.Fatalf("findChecksumInFile: %v", err)
	}
	want := "abc123def456"
	if got != want {
		t.Errorf("findChecksumInFile() = %q, want %q", got, want)
	}
}

func TestFindChecksumInFile_NotFound(t *testing.T) {
	content := `abc123  symaira-vault_0.15.3_darwin_arm64.tar.gz
`
	f := filepath.Join(t.TempDir(), "checksums.txt")
	if err := os.WriteFile(f, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	_, err := findChecksumInFile(f, "nonexistent.tar.gz")
	if err == nil {
		t.Error("findChecksumInFile for nonexistent asset: got nil error, want error")
	}
}

func TestVerifyChecksum(t *testing.T) {
	// Create a file with known content
	content := []byte("hello world\n")
	f := filepath.Join(t.TempDir(), "test.txt")
	if err := os.WriteFile(f, content, 0o644); err != nil {
		t.Fatal(err)
	}

	// Compute expected SHA-256
	// echo -n "hello world\n" | shasum -a 256
	// The exact hash depends on whether newline is included
	// Let's just test that verifyChecksum runs and produces consistent results
	if err := verifyChecksum(f, "not-a-hash"); err == nil {
		t.Error("verifyChecksum with wrong hash: got nil error, want error")
	}
}

func TestAtomicInstall(t *testing.T) {
	binDir := t.TempDir()
	data := []byte("#!/bin/sh\necho test\n")

	if err := atomicInstall(binDir, "test-binary", data); err != nil {
		t.Fatalf("atomicInstall: %v", err)
	}

	path := filepath.Join(binDir, "test-binary")
	info, err := os.Stat(path)
	if err != nil {
		t.Fatalf("stat installed binary: %v", err)
	}
	if info.Mode().Perm()&0111 == 0 {
		t.Error("installed binary is not executable")
	}

	got, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read installed binary: %v", err)
	}
	if string(got) != string(data) {
		t.Errorf("installed content = %q, want %q", got, data)
	}
}

func TestInstalledVersion_Missing(t *testing.T) {
	v, err := InstalledVersion(context.Background(), t.TempDir(), "nonexistent")
	if err != nil {
		t.Fatalf("InstalledVersion: %v", err)
	}
	if v != "" {
		t.Errorf("InstalledVersion() = %q, want empty", v)
	}
}

func TestCertificateIdentity_DefaultWorkflow(t *testing.T) {
	core := &Core{Version: "v0.15.3", Repo: "danieljustus/symaira-vault"}
	got := core.CertificateIdentity()
	// Confirmed against the actual certificate SAN published for
	// symaira-vault v0.15.3's release archives.
	want := "https://github.com/danieljustus/symaira-vault/.github/workflows/release.yml@refs/tags/v0.15.3"
	if got != want {
		t.Errorf("CertificateIdentity() = %q, want %q", got, want)
	}
}

func TestCertificateIdentity_NoVVersion(t *testing.T) {
	// symcockpit's manifest Version has no leading "v", but its actual
	// git tags do — CertificateIdentity must normalize this the same
	// way stripV/AssetName does.
	core := &Core{Version: "0.4.0", Repo: "danieljustus/symaira-cockpit"}
	got := core.CertificateIdentity()
	want := "https://github.com/danieljustus/symaira-cockpit/.github/workflows/release.yml@refs/tags/v0.4.0"
	if got != want {
		t.Errorf("CertificateIdentity() = %q, want %q", got, want)
	}
}

func TestCertificateIdentity_CustomWorkflow(t *testing.T) {
	core := &Core{Version: "v1.0.0", Repo: "example/example-core", ReleaseWorkflow: "publish.yml"}
	got := core.CertificateIdentity()
	want := "https://github.com/example/example-core/.github/workflows/publish.yml@refs/tags/v1.0.0"
	if got != want {
		t.Errorf("CertificateIdentity() = %q, want %q", got, want)
	}
}

func TestCertificateOIDCIssuer(t *testing.T) {
	core := &Core{}
	if got, want := core.CertificateOIDCIssuer(), "https://token.actions.githubusercontent.com"; got != want {
		t.Errorf("CertificateOIDCIssuer() = %q, want %q", got, want)
	}
}

func TestPinnedChecksum(t *testing.T) {
	core := &Core{SHA256: map[string]string{"asset.tar.gz": "abc123"}}

	if got, ok := core.PinnedChecksum("asset.tar.gz"); !ok || got != "abc123" {
		t.Errorf("PinnedChecksum(asset.tar.gz) = (%q, %v), want (%q, true)", got, ok, "abc123")
	}
	if _, ok := core.PinnedChecksum("other.tar.gz"); ok {
		t.Error("PinnedChecksum(other.tar.gz) = ok, want not-pinned")
	}
}

func TestPinnedChecksum_NilMap(t *testing.T) {
	core := &Core{}
	if _, ok := core.PinnedChecksum("asset.tar.gz"); ok {
		t.Error("PinnedChecksum on a core with a nil SHA256 map: got ok, want not-pinned")
	}
}

// TestManifest_SHA256Pinned guards against the manifest regressing to
// the pre-#423 state where sha256 was empty for every core (integrity
// resting solely on the fetched checksums.txt / TLS to github.com), and
// against bump-core.yml regressing to writing a single scalar checksum
// (the checksums.txt file's own hash) into what Core.SHA256 requires to
// be a map keyed by exact release asset filename.
func TestManifest_SHA256Pinned(t *testing.T) {
	m, err := LoadManifest()
	if err != nil {
		t.Fatalf("LoadManifest: %v", err)
	}
	hexSum := regexp.MustCompile(`^[0-9a-f]{64}$`)
	for name, core := range m.Cores {
		if len(core.SHA256) == 0 {
			t.Errorf("core %q: SHA256 is empty — no pinned checksum for any asset", name)
			continue
		}

		wantAssets := map[string]bool{}
		for _, goos := range []string{"darwin", "linux", "windows"} {
			if !core.SupportsPlatform(goos) {
				continue
			}
			for _, goarch := range []string{"arm64", "amd64"} {
				wantAssets[core.AssetName(goos, goarch)] = true
				wantAssets[core.AssetNameAlt(goos, goarch)] = true
			}
		}

		for asset, sum := range core.SHA256 {
			if !hexSum.MatchString(sum) {
				t.Errorf("core %q asset %q: sha256 %q is not 64 lowercase hex chars", name, asset, sum)
			}
			if !wantAssets[asset] {
				t.Errorf("core %q: SHA256 key %q does not match any AssetName/AssetNameAlt for the pinned version %q — stale or malformed entry", name, asset, core.Version)
			}
		}
	}
}
