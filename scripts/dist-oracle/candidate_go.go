package main

import (
	"archive/tar"
	"archive/zip"
	"bytes"
	"compress/gzip"
	"debug/buildinfo"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
)

const (
	goSymbrainModule   = "github.com/danieljustus/symaira-brain"
	maxCandidateBinary = 256 << 20
)

// checkGoCandidateArtifacts adds target identity checks to the regular archive,
// SPDX, and checksum gate. It reads Go build metadata and never executes a
// cross-target binary.
func checkGoCandidateArtifacts(cfg *goreleaserConfig, version, assetsDir string) error {
	if err := checkCandidateArtifacts(cfg, version, assetsDir); err != nil {
		return err
	}
	if len(cfg.Builds) == 0 || len(cfg.Archives) == 0 {
		return fmt.Errorf("config missing build or archive section")
	}
	build, archive := cfg.Builds[0], cfg.Archives[0]
	for _, goos := range build.GOOS {
		for _, goarch := range build.GOARCH {
			name := renderTemplate(archive.NameTemplate, cfg.ProjectName, version, goos, goarch) + "." + formatFor(archive, goos)
			binaryName := build.Binary
			if goos == "windows" && !strings.HasSuffix(strings.ToLower(binaryName), ".exe") {
				binaryName += ".exe"
			}
			binary, err := readCandidateArchiveMember(filepath.Join(assetsDir, name), name, binaryName)
			if err != nil {
				return err
			}
			info, err := buildinfo.Read(bytes.NewReader(binary))
			if err != nil {
				return fmt.Errorf("read Go build identity from %s: %w", name, err)
			}
			if info.Main.Path != goSymbrainModule {
				return fmt.Errorf("archive %s contains Go main module %q, want %q", name, info.Main.Path, goSymbrainModule)
			}
			if err := verifyGoBuildTarget(info, goos, goarch); err != nil {
				return fmt.Errorf("archive %s: %w", name, err)
			}
		}
	}
	return nil
}

func verifyGoBuildTarget(info *buildinfo.BuildInfo, goos, goarch string) error {
	settings := make(map[string]string, len(info.Settings))
	for _, setting := range info.Settings {
		settings[setting.Key] = setting.Value
	}
	if settings["GOOS"] != goos || settings["GOARCH"] != goarch {
		return fmt.Errorf("Go build target is %s/%s, want %s/%s", settings["GOOS"], settings["GOARCH"], goos, goarch)
	}
	return nil
}

func readCandidateArchiveMember(path, archiveName, wanted string) ([]byte, error) {
	read := func(reader io.Reader) ([]byte, error) {
		data, err := io.ReadAll(io.LimitReader(reader, maxCandidateBinary+1))
		if err != nil {
			return nil, fmt.Errorf("read binary from %s: %w", archiveName, err)
		}
		if len(data) > maxCandidateBinary {
			return nil, fmt.Errorf("binary in %s exceeds size limit", archiveName)
		}
		return data, nil
	}
	if strings.HasSuffix(archiveName, ".tar.gz") {
		file, err := os.Open(path)
		if err != nil {
			return nil, fmt.Errorf("open archive %s: %w", archiveName, err)
		}
		defer file.Close()
		gz, err := gzip.NewReader(file)
		if err != nil {
			return nil, fmt.Errorf("open gzip archive %s: %w", archiveName, err)
		}
		defer gz.Close()
		archive := tar.NewReader(gz)
		for {
			header, err := archive.Next()
			if err == io.EOF {
				break
			}
			if err != nil {
				return nil, fmt.Errorf("read archive %s: %w", archiveName, err)
			}
			if header.Name == wanted {
				if header.Typeflag != tar.TypeReg && header.Typeflag != tar.TypeRegA {
					return nil, fmt.Errorf("binary in %s is not a regular file", archiveName)
				}
				if header.Size < 0 || header.Size > maxCandidateBinary {
					return nil, fmt.Errorf("binary in %s exceeds size limit", archiveName)
				}
				return read(archive)
			}
		}
		return nil, fmt.Errorf("archive %s does not contain %s", archiveName, wanted)
	}
	if strings.HasSuffix(archiveName, ".zip") {
		archive, err := zip.OpenReader(path)
		if err != nil {
			return nil, fmt.Errorf("open zip archive %s: %w", archiveName, err)
		}
		defer archive.Close()
		for _, member := range archive.File {
			if member.Name != wanted {
				continue
			}
			if !member.Mode().IsRegular() || member.UncompressedSize64 > maxCandidateBinary {
				return nil, fmt.Errorf("binary in %s is not a bounded regular file", archiveName)
			}
			stream, err := member.Open()
			if err != nil {
				return nil, fmt.Errorf("open binary in %s: %w", archiveName, err)
			}
			data, readErr := read(stream)
			closeErr := stream.Close()
			if readErr != nil {
				return nil, readErr
			}
			if closeErr != nil {
				return nil, fmt.Errorf("close binary in %s: %w", archiveName, closeErr)
			}
			return data, nil
		}
	}
	return nil, fmt.Errorf("archive %s does not contain %s", archiveName, wanted)
}
