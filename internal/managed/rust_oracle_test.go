package managed

import (
	"archive/tar"
	"archive/zip"
	"bytes"
	"compress/gzip"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

type rustManagedCoreCase struct {
	ID                  string `json:"id"`
	Core                Core   `json:"core"`
	GOOS                string `json:"goos"`
	GOARCH              string `json:"goarch"`
	AssetName           string `json:"asset_name"`
	AssetNameAlt        string `json:"asset_name_alt"`
	ChecksumAssetName   string `json:"checksum_asset_name"`
	Tag                 string `json:"tag"`
	CertificateIdentity string `json:"certificate_identity"`
	BinaryPath          string `json:"binary_path"`
	SupportsPlatform    bool   `json:"supports_platform"`
}

type rustManagedArchiveCase struct {
	ID            string `json:"id"`
	Format        string `json:"format"`
	ArchiveHex    string `json:"archive_hex"`
	ExpectedHex   string `json:"expected_hex,omitempty"`
	ErrorContains string `json:"error_contains,omitempty"`
}

type rustManagedOracle struct {
	Manifest      *Manifest                `json:"manifest"`
	CoreCases     []rustManagedCoreCase    `json:"core_cases"`
	ChecksumData  string                   `json:"checksum_data"`
	Checksum      string                   `json:"checksum"`
	ChecksumText  string                   `json:"checksum_text"`
	ChecksumAsset string                   `json:"checksum_asset"`
	ArchiveCases  []rustManagedArchiveCase `json:"archive_cases"`
}

func rustCoreCase(id string, core Core, goos, goarch string) rustManagedCoreCase {
	return rustManagedCoreCase{
		ID: id, Core: core, GOOS: goos, GOARCH: goarch,
		AssetName: core.AssetName(goos, goarch), AssetNameAlt: core.AssetNameAlt(goos, goarch),
		ChecksumAssetName: core.ChecksumAssetName(), Tag: core.Tag(),
		CertificateIdentity: core.CertificateIdentity(), BinaryPath: core.BinaryPathInArchive(goos, goarch),
		SupportsPlatform: core.SupportsPlatform(goos),
	}
}

func rustTar(entries []archiveTestEntry) []byte {
	var buf bytes.Buffer
	gz := gzip.NewWriter(&buf)
	tw := tar.NewWriter(gz)
	for _, entry := range entries {
		typeflag := entry.typeflag
		if typeflag == 0 {
			typeflag = tar.TypeReg
		}
		size := int64(0)
		if typeflag == tar.TypeReg || typeflag == tar.TypeRegA {
			size = int64(len(entry.data))
		}
		_ = tw.WriteHeader(&tar.Header{Name: entry.name, Mode: 0o755, Size: size, Typeflag: typeflag, Linkname: entry.linkname})
		if size > 0 {
			_, _ = tw.Write(entry.data)
		}
	}
	_ = tw.Close()
	_ = gz.Close()
	return buf.Bytes()
}

func rustZip(entries []archiveTestEntry) []byte {
	var buf bytes.Buffer
	zw := zip.NewWriter(&buf)
	for _, entry := range entries {
		header := &zip.FileHeader{Name: entry.name, Method: zip.Store}
		if entry.mode != 0 {
			header.SetMode(entry.mode)
		} else {
			header.SetMode(0o755)
		}
		writer, _ := zw.CreateHeader(header)
		_, _ = writer.Write(entry.data)
	}
	_ = zw.Close()
	return buf.Bytes()
}

func rustArchiveCase(t *testing.T, id, format string, archive []byte, core Core) rustManagedArchiveCase {
	t.Helper()
	ext := ".tar.gz"
	if format == "zip" {
		ext = ".zip"
	}
	path := filepath.Join(t.TempDir(), "fixture"+ext)
	if err := os.WriteFile(path, archive, 0o600); err != nil {
		t.Fatal(err)
	}
	data, err := extractBinary(path, &core, "linux", "amd64")
	result := rustManagedArchiveCase{ID: id, Format: format, ArchiveHex: hex.EncodeToString(archive)}
	if err != nil {
		for _, class := range []string{"unsafe", "ambiguous", "duplicate", "not a regular file", "not found"} {
			if strings.Contains(err.Error(), class) {
				result.ErrorContains = class
				break
			}
		}
		if result.ErrorContains == "" {
			result.ErrorContains = err.Error()
		}
	} else {
		result.ExpectedHex = hex.EncodeToString(data)
	}
	return result
}

func generateRustManagedOracle(t *testing.T) rustManagedOracle {
	t.Helper()
	var manifest Manifest
	if err := json.Unmarshal(embeddedManifestJSON, &manifest); err != nil {
		t.Fatalf("parse embedded manifest: %v", err)
	}
	normal := Core{Version: "v1.2.3", Repo: "example/tool", BinaryName: "symtool", AssetPrefix: "symaira-tool", SHA256: map[string]string{}}
	vault := Core{Version: "v0.21.1", Repo: "danieljustus/symaira-vault", BinaryName: "symvault", AssetPrefix: "symaira-vault", HasCosign: true, SHA256: map[string]string{}}
	cockpit := Core{Version: "0.5.3", Repo: "danieljustus/symaira-cockpit", BinaryName: "symcockpit", AssetPrefix: "symcockpit", Platforms: []string{"darwin"}, AssetArch: "universal", SHA256: map[string]string{}}
	valid := []archiveTestEntry{{name: "nested/symtool", data: []byte("fallback")}, {name: "symtool", data: []byte("exact")}}
	ambiguous := []archiveTestEntry{{name: "one/symtool", data: []byte("one")}, {name: "two/symtool", data: []byte("two")}}
	data := "hello world\n"
	sum := sha256.Sum256([]byte(data))
	checksum := hex.EncodeToString(sum[:])
	return rustManagedOracle{
		Manifest: &manifest,
		CoreCases: []rustManagedCoreCase{
			rustCoreCase("normal-linux", normal, "linux", "amd64"),
			rustCoreCase("vault-darwin", vault, "darwin", "arm64"),
			rustCoreCase("cockpit-universal", cockpit, "darwin", "amd64"),
			rustCoreCase("cockpit-unsupported", cockpit, "linux", "amd64"),
		},
		ChecksumData: data, Checksum: checksum,
		ChecksumText: "# release\n" + checksum + "  symtool.tar.gz\n", ChecksumAsset: "symtool.tar.gz",
		ArchiveCases: []rustManagedArchiveCase{
			rustArchiveCase(t, "tar-valid", "tar.gz", rustTar(valid), normal),
			rustArchiveCase(t, "tar-traversal", "tar.gz", rustTar([]archiveTestEntry{{name: "../symtool", data: []byte("bad")}}), normal),
			rustArchiveCase(t, "tar-symlink", "tar.gz", rustTar([]archiveTestEntry{{name: "symtool", typeflag: tar.TypeSymlink, linkname: "/tmp/bad"}}), normal),
			rustArchiveCase(t, "tar-ambiguous", "tar.gz", rustTar(ambiguous), normal),
			rustArchiveCase(t, "zip-valid", "zip", rustZip(valid), normal),
			rustArchiveCase(t, "zip-traversal", "zip", rustZip([]archiveTestEntry{{name: `dir\..\symtool`, data: []byte("bad")}}), normal),
			rustArchiveCase(t, "zip-symlink", "zip", rustZip([]archiveTestEntry{{name: "symtool", data: []byte("target"), mode: os.ModeSymlink | 0o777}}), normal),
		},
	}
}

func TestRustManagedOracleFixture(t *testing.T) {
	_, sourceFile, _, _ := runtime.Caller(0)
	rustManifest := filepath.Join(filepath.Dir(sourceFile), "..", "..", "rust", "symbrain-managed", "assets", "manifest.json")
	rustManifestBytes, err := os.ReadFile(rustManifest)
	if err != nil {
		t.Fatalf("read Rust embedded manifest: %v", err)
	}
	if !bytes.Equal(rustManifestBytes, embeddedManifestJSON) {
		t.Fatal("Rust embedded manifest drifted from internal/managed/manifest.json")
	}

	oracle := generateRustManagedOracle(t)
	data, err := json.MarshalIndent(oracle, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	data = append(data, '\n')
	_, file, _, _ := runtime.Caller(0)
	fixture := filepath.Join(filepath.Dir(file), "..", "..", "rust", "symbrain-managed", "tests", "fixtures", "oracle_expectations.json")
	if os.Getenv("UPDATE_RUST_MANAGED_ORACLE") == "1" {
		if err := os.MkdirAll(filepath.Dir(fixture), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(fixture, data, 0o644); err != nil {
			t.Fatal(err)
		}
		return
	}
	existing, err := os.ReadFile(fixture)
	if err != nil {
		t.Fatalf("read fixture: %v; regenerate with UPDATE_RUST_MANAGED_ORACLE=1 go test ./internal/managed -run TestRustManagedOracleFixture", err)
	}
	var frozen rustManagedOracle
	if err := json.Unmarshal(existing, &frozen); err != nil {
		t.Fatalf("parse frozen fixture: %v", err)
	}
	frozenArchiveHex := make(map[string]string, len(frozen.ArchiveCases))
	for _, archiveCase := range frozen.ArchiveCases {
		frozenArchiveHex[archiveCase.ID] = archiveCase.ArchiveHex
	}
	for i := range oracle.ArchiveCases {
		if archiveHex, ok := frozenArchiveHex[oracle.ArchiveCases[i].ID]; ok {
			oracle.ArchiveCases[i].ArchiveHex = archiveHex
		}
	}
	data, err = json.MarshalIndent(oracle, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	data = append(data, '\n')
	if !bytes.Equal(existing, data) {
		t.Fatal("managed Rust oracle fixture drift; regenerate with UPDATE_RUST_MANAGED_ORACLE=1 go test ./internal/managed -run TestRustManagedOracleFixture")
	}
}
