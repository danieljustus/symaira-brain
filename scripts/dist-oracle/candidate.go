package main

import (
	"archive/tar"
	"archive/zip"
	"compress/gzip"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

// checkCandidateArtifacts validates a local archive/checksum bundle derived
// from the current candidate. It intentionally makes no release, signing,
// SBOM, Homebrew, DMG, or publication claim.
func checkCandidateArtifacts(cfg *goreleaserConfig, version, assetsDir string) error {
	if version == "" || strings.ContainsAny(version, "/\\\x00") {
		return fmt.Errorf("invalid candidate version %q", version)
	}
	if len(cfg.Builds) == 0 || len(cfg.Archives) == 0 || cfg.Checksum == nil {
		return fmt.Errorf("config missing builds/archives/checksum section")
	}
	build, archive := cfg.Builds[0], cfg.Archives[0]
	if build.Binary == "" || archive.NameTemplate == "" || len(archive.Files) == 0 {
		return fmt.Errorf("config missing binary, archive name template, or archive files")
	}

	expectedArchives := make(map[string][]string)
	for _, goos := range build.GOOS {
		for _, goarch := range build.GOARCH {
			name := renderTemplate(archive.NameTemplate, cfg.ProjectName, version, goos, goarch)
			format := formatFor(archive, goos)
			if hasUnrenderedTokens(name) || format == "" {
				return fmt.Errorf("cannot derive archive for %s/%s", goos, goarch)
			}
			binary := build.Binary
			if goos == "windows" && !strings.HasSuffix(strings.ToLower(binary), ".exe") {
				binary += ".exe"
			}
			members := append(append([]string(nil), archive.Files...), binary)
			expectedArchives[name+"."+format] = members
		}
	}
	if len(expectedArchives) != len(build.GOOS)*len(build.GOARCH) {
		return fmt.Errorf("config has duplicate goos/goarch archive names")
	}
	checksumName := renderTemplate(cfg.Checksum.NameTemplate, cfg.ProjectName, version, "", "")
	if checksumName == "" || hasUnrenderedTokens(checksumName) {
		return fmt.Errorf("cannot derive checksum filename")
	}
	allowedChecksumExtras := make(map[string]struct{})
	if len(cfg.SBOMs) > 0 && cfg.SBOMs[0].Artifacts == "archive" && len(cfg.SBOMs[0].Documents) > 0 {
		documentTemplate := cfg.SBOMs[0].Documents[0]
		if strings.HasPrefix(documentTemplate, "${artifact}") {
			suffix := strings.TrimPrefix(documentTemplate, "${artifact}")
			for name := range expectedArchives {
				allowedChecksumExtras[name+suffix] = struct{}{}
			}
		}
	}

	entries, err := os.ReadDir(assetsDir)
	if err != nil {
		return fmt.Errorf("read candidate directory: %w", err)
	}
	expectedNames := make(map[string]struct{}, len(expectedArchives)+1)
	for name := range expectedArchives {
		expectedNames[name] = struct{}{}
	}
	expectedNames[checksumName] = struct{}{}
	if len(entries) != len(expectedNames) {
		return fmt.Errorf("bundle must contain exactly %d archives plus %s (got %d entries)", len(expectedArchives), checksumName, len(entries))
	}
	files := make(map[string]string, len(entries))
	for _, entry := range entries {
		name := entry.Name()
		if _, ok := expectedNames[name]; !ok {
			return fmt.Errorf("unexpected candidate asset %q", name)
		}
		info, err := entry.Info()
		if err != nil {
			return fmt.Errorf("stat candidate asset %q: %w", name, err)
		}
		if !info.Mode().IsRegular() || info.Mode()&os.ModeSymlink != 0 {
			return fmt.Errorf("candidate asset %q is not a regular file", name)
		}
		files[name] = filepath.Join(assetsDir, name)
	}

	archiveNames := make([]string, 0, len(expectedArchives))
	for name := range expectedArchives {
		archiveNames = append(archiveNames, name)
	}
	sort.Strings(archiveNames)
	checksums := make(map[string]string, len(archiveNames))
	for _, name := range archiveNames {
		members, err := candidateArchiveMembers(files[name], name)
		if err != nil {
			return err
		}
		want := expectedArchives[name]
		if len(members) != len(want) {
			return fmt.Errorf("archive %s has members %v, want %v", name, members, want)
		}
		wantSet := setOf(want)
		for _, member := range members {
			if _, ok := wantSet[member]; !ok {
				return fmt.Errorf("archive %s has unexpected member %q", name, member)
			}
			delete(wantSet, member)
		}
		if len(wantSet) != 0 {
			return fmt.Errorf("archive %s is missing members %v", name, sortedKeys(wantSet))
		}
		data, err := os.ReadFile(files[name])
		if err != nil {
			return fmt.Errorf("read archive %s: %w", name, err)
		}
		digest := sha256.Sum256(data)
		checksums[name] = hex.EncodeToString(digest[:])
	}
	if err := verifyCandidateChecksums(files[checksumName], checksums, allowedChecksumExtras); err != nil {
		return err
	}
	return nil
}

func candidateArchiveMembers(path, name string) ([]string, error) {
	if strings.HasSuffix(name, ".tar.gz") {
		file, err := os.Open(path)
		if err != nil {
			return nil, fmt.Errorf("open archive %s: %w", name, err)
		}
		defer file.Close()
		reader, err := gzip.NewReader(file)
		if err != nil {
			return nil, fmt.Errorf("open gzip archive %s: %w", name, err)
		}
		defer reader.Close()
		tarReader := tar.NewReader(reader)
		var members []string
		for {
			header, err := tarReader.Next()
			if err == io.EOF {
				break
			}
			if err != nil {
				return nil, fmt.Errorf("read tar archive %s: %w", name, err)
			}
			if header.Typeflag != tar.TypeReg && header.Typeflag != tar.TypeRegA {
				return nil, fmt.Errorf("archive %s contains non-regular entry %q", name, header.Name)
			}
			members = append(members, header.Name)
		}
		return members, nil
	}
	if strings.HasSuffix(name, ".zip") {
		archive, err := zip.OpenReader(path)
		if err != nil {
			return nil, fmt.Errorf("open zip archive %s: %w", name, err)
		}
		defer archive.Close()
		members := make([]string, 0, len(archive.File))
		for _, file := range archive.File {
			if !file.Mode().IsRegular() {
				return nil, fmt.Errorf("archive %s contains non-regular entry %q", name, file.Name)
			}
			members = append(members, file.Name)
		}
		return members, nil
	}
	return nil, fmt.Errorf("unsupported archive format for %s", name)
}

func verifyCandidateChecksums(
	path string,
	expected map[string]string,
	allowedExtras map[string]struct{},
) error {
	data, err := os.ReadFile(path)
	if err != nil {
		return fmt.Errorf("read candidate checksums: %w", err)
	}
	got := make(map[string]string)
	for lineNumber, line := range strings.Split(strings.TrimSuffix(string(data), "\n"), "\n") {
		if line == "" {
			continue
		}
		fields := strings.SplitN(line, "  ", 2)
		if len(fields) != 2 || len(fields[0]) != 64 {
			return fmt.Errorf("checksums.txt line %d is malformed", lineNumber+1)
		}
		if _, err := hex.DecodeString(fields[0]); err != nil {
			return fmt.Errorf("checksums.txt line %d has invalid SHA-256", lineNumber+1)
		}
		if _, exists := got[fields[1]]; exists {
			return fmt.Errorf("checksums.txt has duplicate entry for %q", fields[1])
		}
		got[fields[1]] = fields[0]
	}
	if len(got) < len(expected) ||
		len(got) > len(expected)+len(allowedExtras) ||
		(len(got) > len(expected) && len(got) != len(expected)+len(allowedExtras)) {
		return fmt.Errorf("checksums.txt has %d entries, want %d archive entries plus configured SBOM entries", len(got), len(expected))
	}
	for name, digest := range expected {
		if got[name] != digest {
			return fmt.Errorf("checksums.txt digest mismatch for %s", name)
		}
	}
	for name := range got {
		if _, isArchive := expected[name]; isArchive {
			continue
		}
		if _, isConfiguredSBOM := allowedExtras[name]; !isConfiguredSBOM {
			return fmt.Errorf("checksums.txt has unexpected entry for %q", name)
		}
	}
	return nil
}

func sortedKeys(values map[string]struct{}) []string {
	keys := make([]string, 0, len(values))
	for key := range values {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	return keys
}
