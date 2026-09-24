package main

import (
	"archive/tar"
	"archive/zip"
	"bytes"
	"compress/gzip"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"gopkg.in/yaml.v3"
)

func TestCandidateArchiveBundleChecksNamesContentsAndChecksums(t *testing.T) {
	configBytes, err := os.ReadFile(filepath.Join("..", "..", ".goreleaser.yml"))
	if err != nil {
		t.Fatal(err)
	}
	var cfg goreleaserConfig
	if err := yaml.Unmarshal(configBytes, &cfg); err != nil {
		t.Fatal(err)
	}
	assets := t.TempDir()
	packages := filepath.Join(t.TempDir(), "native-packages")
	if err := os.Mkdir(packages, 0o700); err != nil {
		t.Fatal(err)
	}
	const version = "0.12.0"
	for _, goos := range cfg.Builds[0].GOOS {
		for _, goarch := range cfg.Builds[0].GOARCH {
			name := renderTemplate(cfg.Archives[0].NameTemplate, cfg.ProjectName, version, goos, goarch) + "." + formatFor(cfg.Archives[0], goos)
			binary := cfg.Builds[0].Binary
			if goos == "windows" && !strings.HasSuffix(binary, ".exe") {
				binary += ".exe"
			}
			members := append(append([]string(nil), cfg.Archives[0].Files...), binary)
			var data []byte
			if strings.HasSuffix(name, ".zip") {
				var output bytes.Buffer
				writer := zip.NewWriter(&output)
				for _, member := range members {
					header := &zip.FileHeader{Name: member}
					header.SetMode(0o644)
					file, err := writer.CreateHeader(header)
					if err != nil {
						t.Fatal(err)
					}
					if _, err := file.Write([]byte(member)); err != nil {
						t.Fatal(err)
					}
				}
				if err := writer.Close(); err != nil {
					t.Fatal(err)
				}
				data = output.Bytes()
			} else {
				var output bytes.Buffer
				gzipWriter := gzip.NewWriter(&output)
				tarWriter := tar.NewWriter(gzipWriter)
				for _, member := range members {
					contents := []byte(member)
					if err := tarWriter.WriteHeader(&tar.Header{Name: member, Mode: 0o644, Size: int64(len(contents)), Typeflag: tar.TypeReg}); err != nil {
						t.Fatal(err)
					}
					if _, err := tarWriter.Write(contents); err != nil {
						t.Fatal(err)
					}
				}
				if err := tarWriter.Close(); err != nil {
					t.Fatal(err)
				}
				if err := gzipWriter.Close(); err != nil {
					t.Fatal(err)
				}
				data = output.Bytes()
			}
			pkg := filepath.Join(packages, goos+"-"+goarch)
			if err := os.Mkdir(pkg, 0o700); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(filepath.Join(pkg, name), data, 0o600); err != nil {
				t.Fatal(err)
			}
			digest := sha256.Sum256(data)
			checksum := fmt.Sprintf("%s  %s\n", hex.EncodeToString(digest[:]), name)
			if err := os.WriteFile(filepath.Join(pkg, cfg.Checksum.NameTemplate), []byte(checksum), 0o600); err != nil {
				t.Fatal(err)
			}
		}
	}
	if err := mergeNativeCandidatePackages(&cfg, version, packages, assets); err != nil {
		t.Fatalf("valid native candidate packages rejected: %v", err)
	}
	occupied := t.TempDir()
	sentinel := filepath.Join(occupied, "preserve-me")
	if err := os.WriteFile(sentinel, []byte("foreign output"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := mergeNativeCandidatePackages(&cfg, version, packages, occupied); err == nil {
		t.Fatal("merge into a non-empty output directory unexpectedly succeeded")
	}
	if got := string(mustRead(t, sentinel)); got != "foreign output" {
		t.Fatalf("merge changed pre-existing output: %q", got)
	}

	checksums := filepath.Join(assets, cfg.Checksum.NameTemplate)
	checksumLines := strings.Split(strings.TrimSpace(string(mustRead(t, checksums))), "\n")
	changed := strings.Replace(string(mustRead(t, checksums)), checksumLines[0][:64], strings.Repeat("0", 64), 1)
	if err := os.WriteFile(checksums, []byte(changed), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := checkCandidateArtifacts(&cfg, version, assets); err == nil || !strings.Contains(err.Error(), "digest mismatch") {
		t.Fatalf("bad archive checksum error = %v, want digest mismatch", err)
	}
}

func TestNativeCandidateIdentityRequiresRustProductVersionAndTarget(t *testing.T) {
	validJSON := []byte(`{"tool":"symbrain","version":"0.12.0","schema_version":1}`)
	identity, err := parseVersionIdentity(validJSON)
	if err != nil || verifyVersionIdentity(identity, "0.12.0") != nil {
		t.Fatalf("parseVersionIdentity() = %#v, %v", identity, err)
	}
	for _, invalid := range [][]byte{
		[]byte(`{"tool":"symbrain-go","version":"0.12.0","schema_version":1}`),
		[]byte(`{"tool":"symbrain","version":"0.12.1","schema_version":1}`),
		[]byte(`{"tool":"symbrain","version":"0.12.0","schema_version":1} {}`),
	} {
		got, err := parseVersionIdentity(invalid)
		if err == nil {
			err = verifyVersionIdentity(got, "0.12.0")
		}
		if err == nil {
			t.Fatalf("parseVersionIdentity(%q) unexpectedly succeeded", invalid)
		}
	}
	if !matchesNativeRustVersion("symbrain 0.12.0\n  rust    rustc\n  os/arch darwin/arm64\n", "0.12.0", "darwin", "arm64") {
		t.Fatal("valid Rust target identity rejected")
	}
	for _, output := range []string{
		"symbrain 0.12.0\n  Go      go1.26\n  OS/Arch darwin/arm64\n",
		"symbrain 0.12.0\n  rust    rustc\n  os/arch darwin/amd64\n",
		"symbrain 0.12.0\n  rust    \n  os/arch darwin/arm64\n",
	} {
		if matchesNativeRustVersion(output, "0.12.0", "darwin", "arm64") {
			t.Fatalf("wrong identity accepted: %q", output)
		}
	}
}

func mustRead(t *testing.T, path string) []byte {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	return data
}
