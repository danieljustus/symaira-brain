package main

import (
	"archive/tar"
	"archive/zip"
	"bytes"
	"compress/gzip"
	"debug/buildinfo"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"testing"
)

func TestGoBuildTargetUsesEmbeddedArchitecture(t *testing.T) {
	info, err := buildinfo.ReadFile(os.Args[0])
	if err != nil {
		t.Fatalf("read test executable Go build info: %v", err)
	}
	if err := verifyGoBuildTarget(info, runtime.GOOS, runtime.GOARCH); err != nil {
		t.Fatalf("accept current target: %v", err)
	}
	wrongArch := "amd64"
	if runtime.GOARCH == wrongArch {
		wrongArch = "arm64"
	}
	if err := verifyGoBuildTarget(info, runtime.GOOS, wrongArch); err == nil {
		t.Fatal("accepted an archive target that differs from embedded Go build info")
	}
}

func TestStrippedSymbrainBinaryKeepsDistributionIdentity(t *testing.T) {
	root := filepath.Clean(filepath.Join("..", ".."))
	binary := filepath.Join(t.TempDir(), "symbrain")
	command := exec.Command("go", "build", "-ldflags", "-s -w", "-o", binary, "./cmd/symbrain")
	command.Dir = root
	if output, err := command.CombinedOutput(); err != nil {
		t.Fatalf("build stripped Go symbrain: %v\n%s", err, output)
	}
	info, err := buildinfo.ReadFile(binary)
	if err != nil {
		t.Fatalf("read stripped Go symbrain build info: %v", err)
	}
	if info.Main.Path != goSymbrainModule {
		t.Fatalf("main module = %q, want %q", info.Main.Path, goSymbrainModule)
	}
	if err := verifyGoBuildTarget(info, runtime.GOOS, runtime.GOARCH); err != nil {
		t.Fatalf("verify native Go symbrain target: %v", err)
	}
}

func TestGoBinaryIdentityCanBeReadFromBothArchiveFormats(t *testing.T) {
	binary, err := os.ReadFile(os.Args[0])
	if err != nil {
		t.Fatal(err)
	}
	dir := t.TempDir()
	for _, format := range []string{"tar.gz", "zip"} {
		t.Run(format, func(t *testing.T) {
			path := filepath.Join(dir, "candidate."+format)
			if format == "zip" {
				file, err := os.Create(path)
				if err != nil {
					t.Fatal(err)
				}
				writer := zip.NewWriter(file)
				member, err := writer.Create("symbrain.exe")
				if err != nil {
					t.Fatal(err)
				}
				if _, err := member.Write(binary); err != nil {
					t.Fatal(err)
				}
				if err := writer.Close(); err != nil {
					t.Fatal(err)
				}
				if err := file.Close(); err != nil {
					t.Fatal(err)
				}
			} else {
				file, err := os.Create(path)
				if err != nil {
					t.Fatal(err)
				}
				compressed := gzip.NewWriter(file)
				archive := tar.NewWriter(compressed)
				if err := archive.WriteHeader(&tar.Header{Name: "symbrain", Mode: 0o755, Size: int64(len(binary)), Typeflag: tar.TypeReg}); err != nil {
					t.Fatal(err)
				}
				if _, err := archive.Write(binary); err != nil {
					t.Fatal(err)
				}
				if err := archive.Close(); err != nil {
					t.Fatal(err)
				}
				if err := compressed.Close(); err != nil {
					t.Fatal(err)
				}
				if err := file.Close(); err != nil {
					t.Fatal(err)
				}
			}

			member := "symbrain"
			if format == "zip" {
				member = "symbrain.exe"
			}
			got, err := readCandidateArchiveMember(path, filepath.Base(path), member)
			if err != nil {
				t.Fatal(err)
			}
			info, err := buildinfo.Read(bytes.NewReader(got))
			if err != nil {
				t.Fatalf("read embedded build info: %v", err)
			}
			if err := verifyGoBuildTarget(info, runtime.GOOS, runtime.GOARCH); err != nil {
				t.Fatal(err)
			}
		})
	}
}
