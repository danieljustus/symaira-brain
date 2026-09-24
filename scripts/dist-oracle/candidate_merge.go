package main

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

// mergeNativeCandidatePackages combines six one-target packages produced and
// identity-checked on their respective native runners into candidate-check's
// complete archive/checksum bundle.
func mergeNativeCandidatePackages(cfg *goreleaserConfig, version, packagesDir, assetsDir string) error {
	if version == "" || strings.ContainsAny(version, "/\\\x00") {
		return fmt.Errorf("invalid candidate version %q", version)
	}
	if len(cfg.Builds) == 0 || len(cfg.Archives) == 0 || cfg.Checksum == nil {
		return fmt.Errorf("config missing builds/archives/checksum section")
	}
	rootInfo, err := os.Lstat(packagesDir)
	if err != nil || !rootInfo.IsDir() || rootInfo.Mode()&os.ModeSymlink != 0 {
		return fmt.Errorf("native packages path must be a real directory")
	}
	build, archive := cfg.Builds[0], cfg.Archives[0]
	expected := make(map[string][]string)
	for _, goos := range build.GOOS {
		for _, goarch := range build.GOARCH {
			name := renderTemplate(archive.NameTemplate, cfg.ProjectName, version, goos, goarch) + "." + formatFor(archive, goos)
			binary := build.Binary
			if goos == "windows" && !strings.HasSuffix(strings.ToLower(binary), ".exe") {
				binary += ".exe"
			}
			expected[name] = append(append([]string(nil), archive.Files...), binary)
		}
	}
	packages, err := os.ReadDir(packagesDir)
	if err != nil {
		return fmt.Errorf("read native package directories: %w", err)
	}
	if len(packages) != len(expected) {
		return fmt.Errorf("got %d native package directories, want %d", len(packages), len(expected))
	}
	type candidate struct{ name, path string }
	var candidates []candidate
	var checksums []string
	seen := make(map[string]struct{}, len(expected))
	for _, pkg := range packages {
		if !pkg.IsDir() || pkg.Type()&os.ModeSymlink != 0 {
			return fmt.Errorf("native package is not a directory: %s", pkg.Name())
		}
		pkgPath := filepath.Join(packagesDir, pkg.Name())
		files, err := os.ReadDir(pkgPath)
		if err != nil || len(files) != 2 {
			return fmt.Errorf("native package %s must contain one archive and checksums.txt", pkg.Name())
		}
		var archiveName string
		checksumPresent := false
		for _, file := range files {
			if file.Name() == cfg.Checksum.NameTemplate {
				checksumPresent = true
			} else {
				if _, ok := expected[file.Name()]; !ok {
					return fmt.Errorf("unexpected native package asset %q", file.Name())
				}
				archiveName = file.Name()
			}
			if file.Type()&os.ModeSymlink != 0 || file.IsDir() {
				return fmt.Errorf("native package asset is not a regular file: %s", file.Name())
			}
			info, err := file.Info()
			if err != nil || !info.Mode().IsRegular() {
				return fmt.Errorf("native package asset is not a regular file: %s", file.Name())
			}
		}
		if !checksumPresent {
			return fmt.Errorf("native package %s has no %s", pkg.Name(), cfg.Checksum.NameTemplate)
		}
		want, ok := expected[archiveName]
		if !ok {
			return fmt.Errorf("native package %s does not contain one configured archive", pkg.Name())
		}
		if _, duplicate := seen[archiveName]; duplicate {
			return fmt.Errorf("duplicate native package archive %s", archiveName)
		}
		seen[archiveName] = struct{}{}
		archivePath := filepath.Join(pkgPath, archiveName)
		members, err := candidateArchiveMembers(archivePath, archiveName)
		if err != nil || !sameStrings(members, want) {
			return fmt.Errorf("native package %s archive members do not match config", pkg.Name())
		}
		data, err := os.ReadFile(archivePath)
		if err != nil {
			return fmt.Errorf("read native package archive %s: %w", archiveName, err)
		}
		digest := sha256.Sum256(data)
		checksum := hex.EncodeToString(digest[:])
		if err := verifyCandidateChecksums(filepath.Join(pkgPath, cfg.Checksum.NameTemplate), map[string]string{archiveName: checksum}, nil); err != nil {
			return fmt.Errorf("verify native package %s: %w", pkg.Name(), err)
		}
		candidates = append(candidates, candidate{name: archiveName, path: archivePath})
		checksums = append(checksums, checksum+"  "+archiveName)
	}
	if len(candidates) != len(expected) {
		return fmt.Errorf("native packages cover %d archives, want %d", len(candidates), len(expected))
	}
	if err := os.MkdirAll(assetsDir, 0o755); err != nil {
		return fmt.Errorf("create merged candidate directory: %w", err)
	}
	dirInfo, err := os.Lstat(assetsDir)
	if err != nil || !dirInfo.IsDir() || dirInfo.Mode()&os.ModeSymlink != 0 {
		return fmt.Errorf("merged candidate path must be a real directory")
	}
	if entries, err := os.ReadDir(assetsDir); err != nil || len(entries) != 0 {
		return fmt.Errorf("merged candidate directory must be empty")
	}
	for _, candidate := range candidates {
		if err := copyExclusive(candidate.path, filepath.Join(assetsDir, candidate.name)); err != nil {
			return err
		}
	}
	sort.Strings(checksums)
	checksumPath := filepath.Join(assetsDir, cfg.Checksum.NameTemplate)
	checksumFile, err := os.OpenFile(checksumPath, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o644)
	if err != nil {
		return fmt.Errorf("create merged checksums: %w", err)
	}
	if _, err := checksumFile.WriteString(strings.Join(checksums, "\n") + "\n"); err != nil {
		checksumFile.Close()
		return fmt.Errorf("write merged checksums: %w", err)
	}
	if err := checksumFile.Close(); err != nil {
		return fmt.Errorf("write merged checksums: %w", err)
	}
	if err := checkCandidateArtifacts(cfg, version, assetsDir); err != nil {
		return fmt.Errorf("check merged candidate: %w", err)
	}
	return nil
}

func copyExclusive(source, destination string) error {
	in, err := os.Open(source)
	if err != nil {
		return fmt.Errorf("open package archive: %w", err)
	}
	defer in.Close()
	out, err := os.OpenFile(destination, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o644)
	if err != nil {
		return fmt.Errorf("create merged archive: %w", err)
	}
	if _, err := io.Copy(out, in); err != nil {
		out.Close()
		os.Remove(destination)
		return fmt.Errorf("copy merged archive: %w", err)
	}
	if err := out.Close(); err != nil {
		os.Remove(destination)
		return fmt.Errorf("close merged archive: %w", err)
	}
	return nil
}

func sameStrings(got, want []string) bool {
	if len(got) != len(want) {
		return false
	}
	gotSet, wantSet := setOf(got), setOf(want)
	if len(gotSet) != len(wantSet) {
		return false
	}
	for name := range wantSet {
		if _, ok := gotSet[name]; !ok {
			return false
		}
	}
	return true
}
