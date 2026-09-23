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
	const version = "0.12.0"
	var checksumLines []string
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
			if err := os.WriteFile(filepath.Join(assets, name), data, 0o600); err != nil {
				t.Fatal(err)
			}
			digest := sha256.Sum256(data)
			checksumLines = append(checksumLines, fmt.Sprintf("%s  %s", hex.EncodeToString(digest[:]), name))
		}
	}
	if err := os.WriteFile(filepath.Join(assets, cfg.Checksum.NameTemplate), []byte(strings.Join(checksumLines, "\n")+"\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := checkCandidateArtifacts(&cfg, version, assets); err != nil {
		t.Fatalf("valid local candidate bundle rejected: %v", err)
	}

	checksums := filepath.Join(assets, cfg.Checksum.NameTemplate)
	changed := strings.Replace(string(mustRead(t, checksums)), checksumLines[0][:64], strings.Repeat("0", 64), 1)
	if err := os.WriteFile(checksums, []byte(changed), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := checkCandidateArtifacts(&cfg, version, assets); err == nil || !strings.Contains(err.Error(), "digest mismatch") {
		t.Fatalf("bad archive checksum error = %v, want digest mismatch", err)
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
