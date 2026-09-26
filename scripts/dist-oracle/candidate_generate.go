package main

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"os"
	"path/filepath"
	"sort"
)

// generateCandidateSPDX adds deterministic SPDX sidecars to an unpublished
// GoReleaser snapshot whose checksum file currently covers archives only.
func generateCandidateSPDX(cfg *goreleaserConfig, version, assetsDir string) error {
	if version == "" || len(cfg.Builds) == 0 || len(cfg.Archives) == 0 || cfg.Checksum == nil {
		return fmt.Errorf("config or candidate version is incomplete")
	}
	dirInfo, err := os.Lstat(assetsDir)
	if err != nil || !dirInfo.IsDir() || dirInfo.Mode()&os.ModeSymlink != 0 {
		return fmt.Errorf("candidate assets path must be a real directory")
	}
	build, archive := cfg.Builds[0], cfg.Archives[0]
	archives := make([]string, 0, len(build.GOOS)*len(build.GOARCH))
	for _, goos := range build.GOOS {
		for _, goarch := range build.GOARCH {
			name := renderTemplate(archive.NameTemplate, cfg.ProjectName, version, goos, goarch)
			format := formatFor(archive, goos)
			if hasUnrenderedTokens(name) || (format != "tar.gz" && format != "zip") {
				return fmt.Errorf("cannot derive archive for %s/%s", goos, goarch)
			}
			archives = append(archives, name+"."+format)
		}
	}
	if len(archives) == 0 {
		return fmt.Errorf("config has no candidate archives")
	}
	checksumName := cfg.Checksum.NameTemplate
	if filepath.Base(checksumName) != checksumName {
		return fmt.Errorf("checksum name must be a filename")
	}
	entries, err := os.ReadDir(assetsDir)
	if err != nil {
		return fmt.Errorf("read candidate assets: %w", err)
	}
	if len(entries) != len(archives)+1 {
		return fmt.Errorf("snapshot must contain %d archives and %s before SPDX generation", len(archives), checksumName)
	}
	archiveChecksums := make(map[string]string, len(archives))
	for _, name := range archives {
		path := filepath.Join(assetsDir, name)
		info, err := os.Lstat(path)
		if err != nil || !info.Mode().IsRegular() || info.Mode()&os.ModeSymlink != 0 {
			return fmt.Errorf("candidate archive %s is not a regular file", name)
		}
		data, err := os.ReadFile(path)
		if err != nil {
			return fmt.Errorf("read candidate archive %s: %w", name, err)
		}
		digest := sha256.Sum256(data)
		archiveChecksums[name] = hex.EncodeToString(digest[:])
	}
	for _, entry := range entries {
		if entry.Name() == checksumName {
			info, err := entry.Info()
			if err != nil || !info.Mode().IsRegular() || info.Mode()&os.ModeSymlink != 0 {
				return fmt.Errorf("candidate checksums are not a regular file")
			}
			continue
		}
		if _, ok := archiveChecksums[entry.Name()]; !ok {
			return fmt.Errorf("unexpected snapshot asset %q", entry.Name())
		}
	}
	checksumPath := filepath.Join(assetsDir, checksumName)
	if err := verifyCandidateChecksums(checksumPath, archiveChecksums); err != nil {
		return fmt.Errorf("verify GoReleaser archive checksums: %w", err)
	}

	type sidecar struct {
		name, path string
		data       []byte
	}
	sidecars := make([]sidecar, 0, len(archives))
	checksums := make(map[string]string, len(archives)*2)
	for _, name := range archives {
		checksums[name] = archiveChecksums[name]
		sbomName, err := candidateSBOMName(cfg, name)
		if err != nil {
			return err
		}
		sbom, err := candidateSPDX(filepath.Join(assetsDir, name), name, version, archiveChecksums[name])
		if err != nil {
			return err
		}
		digest := sha256.Sum256(sbom)
		sbomDigest := hex.EncodeToString(digest[:])
		checksums[sbomName] = sbomDigest
		sidecars = append(sidecars, sidecar{name: sbomName, path: filepath.Join(assetsDir, sbomName), data: sbom})
	}
	created := make([]string, 0, len(sidecars))
	defer func() {
		for _, path := range created {
			_ = os.Remove(path)
		}
	}()
	for _, sbom := range sidecars {
		file, err := os.OpenFile(sbom.path, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o644)
		if err != nil {
			return fmt.Errorf("create candidate SPDX SBOM %s: %w", sbom.name, err)
		}
		created = append(created, sbom.path)
		if _, err := file.Write(sbom.data); err != nil {
			file.Close()
			return fmt.Errorf("write candidate SPDX SBOM %s: %w", sbom.name, err)
		}
		if err := file.Close(); err != nil {
			return fmt.Errorf("close candidate SPDX SBOM %s: %w", sbom.name, err)
		}
	}

	names := make([]string, 0, len(checksums))
	for name := range checksums {
		names = append(names, name)
	}
	sort.Strings(names)
	content := make([]byte, 0, len(names)*100)
	for _, name := range names {
		content = append(content, checksums[name]...)
		content = append(content, "  "...)
		content = append(content, name...)
		content = append(content, '\n')
	}
	tmp, err := os.CreateTemp(assetsDir, ".checksums-*")
	if err != nil {
		return fmt.Errorf("stage candidate checksums: %w", err)
	}
	tmpPath := tmp.Name()
	defer os.Remove(tmpPath)
	if _, err := tmp.Write(content); err != nil {
		tmp.Close()
		return fmt.Errorf("write candidate checksums: %w", err)
	}
	if err := tmp.Close(); err != nil {
		return fmt.Errorf("close candidate checksums: %w", err)
	}
	if err := os.Rename(tmpPath, checksumPath); err != nil {
		return fmt.Errorf("replace candidate checksums: %w", err)
	}
	created = nil
	return nil
}
