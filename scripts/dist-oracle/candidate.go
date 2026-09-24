package main

import (
	"archive/tar"
	"archive/zip"
	"compress/gzip"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"sort"
	"strings"
	"time"
)

type versionIdentity struct {
	Tool          string `json:"tool"`
	Version       string `json:"version"`
	SchemaVersion int    `json:"schema_version"`
}

type archiveEntry struct {
	name   string
	source string
}

// packageNativeCandidate verifies and packages one native Rust binary. Each
// target must run on its native OS/architecture so the executable handshake
// proves product identity instead of trusting an archive filename.
func packageNativeCandidate(cfg *goreleaserConfig, version, binaryPath, assetsDir string) (string, error) {
	if version == "" || strings.ContainsAny(version, "/\\\x00") {
		return "", fmt.Errorf("invalid candidate version %q", version)
	}
	if len(cfg.Builds) == 0 || len(cfg.Archives) == 0 || cfg.Checksum == nil {
		return "", fmt.Errorf("config missing builds/archives/checksum section")
	}
	build, archive := cfg.Builds[0], cfg.Archives[0]
	if !contains(build.GOOS, runtime.GOOS) || !contains(build.GOARCH, runtime.GOARCH) {
		return "", fmt.Errorf("native target %s/%s is not configured", runtime.GOOS, runtime.GOARCH)
	}
	if build.Binary == "" || archive.NameTemplate == "" || len(archive.Files) == 0 {
		return "", fmt.Errorf("config missing binary, archive name template, or archive files")
	}
	binaryPath, err := filepath.Abs(binaryPath)
	if err != nil {
		return "", fmt.Errorf("resolve candidate binary: %w", err)
	}
	info, err := os.Lstat(binaryPath)
	if err != nil {
		return "", fmt.Errorf("stat candidate binary: %w", err)
	}
	if !info.Mode().IsRegular() || info.Mode()&os.ModeSymlink != 0 {
		return "", fmt.Errorf("candidate binary is not a regular file")
	}
	if err := verifyNativeRustIdentity(binaryPath, version, runtime.GOOS, runtime.GOARCH); err != nil {
		return "", err
	}

	name := renderTemplate(archive.NameTemplate, cfg.ProjectName, version, runtime.GOOS, runtime.GOARCH)
	format := formatFor(archive, runtime.GOOS)
	if hasUnrenderedTokens(name) || filepath.Base(name) != name || (format != "tar.gz" && format != "zip") {
		return "", fmt.Errorf("cannot derive native archive name or format for %s/%s", runtime.GOOS, runtime.GOARCH)
	}
	binaryName := build.Binary
	if runtime.GOOS == "windows" && !strings.HasSuffix(strings.ToLower(binaryName), ".exe") {
		binaryName += ".exe"
	}
	archiveName := name + "." + format
	if filepath.Base(cfg.Checksum.NameTemplate) != cfg.Checksum.NameTemplate {
		return "", fmt.Errorf("checksum name must be a filename")
	}
	if err := os.MkdirAll(assetsDir, 0o755); err != nil {
		return "", fmt.Errorf("create candidate directory: %w", err)
	}
	if files, err := os.ReadDir(assetsDir); err != nil || len(files) != 0 {
		return "", fmt.Errorf("native candidate directory must be empty")
	}
	entries := make([]archiveEntry, 0, len(archive.Files)+1)
	for _, name := range archive.Files {
		entries = append(entries, archiveEntry{name: name, source: name})
	}
	entries = append(entries, archiveEntry{name: binaryName, source: binaryPath})
	archivePath := filepath.Join(assetsDir, archiveName)
	if err := writeCandidateArchive(archivePath, format, entries); err != nil {
		return "", err
	}
	members, err := candidateArchiveMembers(archivePath, archiveName)
	wantMembers := setOf(append(append([]string(nil), archive.Files...), binaryName))
	if err != nil || len(members) != len(wantMembers) {
		os.Remove(archivePath)
		return "", fmt.Errorf("candidate archive members differ from config")
	}
	for _, member := range members {
		if _, ok := wantMembers[member]; !ok {
			os.Remove(archivePath)
			return "", fmt.Errorf("candidate archive has unexpected member %q", member)
		}
	}
	data, err := os.ReadFile(archivePath)
	if err != nil {
		return "", fmt.Errorf("read candidate archive: %w", err)
	}
	digest := sha256.Sum256(data)
	checksum := fmt.Sprintf("%s  %s\n", hex.EncodeToString(digest[:]), archiveName)
	checksumPath := filepath.Join(assetsDir, cfg.Checksum.NameTemplate)
	checksumFile, err := os.OpenFile(checksumPath, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o644)
	if err != nil {
		os.Remove(archivePath)
		return "", fmt.Errorf("create candidate checksums: %w", err)
	}
	if _, err := checksumFile.WriteString(checksum); err != nil {
		checksumFile.Close()
		os.Remove(checksumPath)
		os.Remove(archivePath)
		return "", fmt.Errorf("write candidate checksums: %w", err)
	}
	if err := checksumFile.Close(); err != nil {
		os.Remove(checksumPath)
		os.Remove(archivePath)
		return "", fmt.Errorf("close candidate checksums: %w", err)
	}
	return archiveName, nil
}

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
	if err := os.MkdirAll(assetsDir, 0o755); err != nil {
		return fmt.Errorf("create merged candidate directory: %w", err)
	}
	if entries, err := os.ReadDir(assetsDir); err != nil || len(entries) != 0 {
		return fmt.Errorf("merged candidate directory must be empty")
	}
	var names []string
	var checksums []string
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
		for _, file := range files {
			if file.Name() != cfg.Checksum.NameTemplate {
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
		want, ok := expected[archiveName]
		if !ok {
			return fmt.Errorf("native package %s does not contain one configured archive", pkg.Name())
		}
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
		if err := copyExclusive(archivePath, filepath.Join(assetsDir, archiveName)); err != nil {
			return err
		}
		names = append(names, archiveName)
		checksums = append(checksums, checksum+"  "+archiveName)
	}
	if len(names) != len(expected) {
		return fmt.Errorf("native packages cover %d archives, want %d", len(names), len(expected))
	}
	sort.Strings(checksums)
	checksumPath := filepath.Join(assetsDir, cfg.Checksum.NameTemplate)
	if err := os.WriteFile(checksumPath, []byte(strings.Join(checksums, "\n")+"\n"), 0o644); err != nil {
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

func verifyNativeRustIdentity(binaryPath, version, goos, goarch string) error {
	cmd := exec.Command(binaryPath, "version", "--json")
	stdout, err := cmd.Output()
	if err != nil {
		return fmt.Errorf("run candidate version --json: %w", err)
	}
	identity, err := parseVersionIdentity(stdout)
	if err != nil {
		return fmt.Errorf("parse candidate version --json: %w", err)
	}
	if err := verifyVersionIdentity(identity, version); err != nil {
		return err
	}
	cmd = exec.Command(binaryPath, "version")
	stdout, err = cmd.Output()
	if err != nil {
		return fmt.Errorf("run candidate version: %w", err)
	}
	if !matchesNativeRustVersion(string(stdout), version, goos, goarch) {
		return fmt.Errorf("candidate human version does not identify Rust symbrain %s for %s/%s", version, goos, goarch)
	}
	return nil
}

func verifyVersionIdentity(identity versionIdentity, version string) error {
	if identity.Tool != "symbrain" || identity.Version != version || identity.SchemaVersion != 1 {
		return fmt.Errorf("candidate identity is tool=%q version=%q schema_version=%d; want symbrain %s schema_version=1", identity.Tool, identity.Version, identity.SchemaVersion, version)
	}
	return nil
}

func parseVersionIdentity(stdout []byte) (versionIdentity, error) {
	var identity versionIdentity
	decoder := json.NewDecoder(strings.NewReader(string(stdout)))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&identity); err != nil {
		return identity, err
	}
	if err := decoder.Decode(new(any)); err != io.EOF {
		return identity, fmt.Errorf("candidate version --json has trailing data")
	}
	return identity, nil
}

func matchesNativeRustVersion(stdout, version, goos, goarch string) bool {
	lines := strings.Split(string(stdout), "\n")
	return len(lines) == 4 && lines[0] == "symbrain "+version && strings.HasPrefix(lines[1], "  rust    ") && lines[2] == "  os/arch "+goos+"/"+goarch && lines[3] == ""
}

func contains(values []string, want string) bool {
	for _, value := range values {
		if value == want {
			return true
		}
	}
	return false
}

func writeCandidateArchive(path, format string, entries []archiveEntry) error {
	if format == "zip" {
		return writeCandidateZIP(path, entries)
	}
	return writeCandidateTarGz(path, entries)
}

func writeCandidateTarGz(path string, entries []archiveEntry) error {
	file, err := os.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o644)
	if err != nil {
		return fmt.Errorf("create candidate archive: %w", err)
	}
	ok := false
	defer func() {
		file.Close()
		if !ok {
			os.Remove(path)
		}
	}()
	gz := gzip.NewWriter(file)
	gz.Header.ModTime = time.Unix(0, 0)
	tarWriter := tar.NewWriter(gz)
	for _, entry := range entries {
		info, err := os.Stat(entry.source)
		if err != nil {
			return fmt.Errorf("stat archive input %s: %w", entry.source, err)
		}
		if !info.Mode().IsRegular() {
			return fmt.Errorf("archive input is not a regular file: %s", entry.source)
		}
		input, err := os.Open(entry.source)
		if err != nil {
			return fmt.Errorf("open archive input %s: %w", entry.source, err)
		}
		header := &tar.Header{Name: entry.name, Mode: int64(info.Mode().Perm()), Size: info.Size(), ModTime: time.Unix(0, 0), Typeflag: tar.TypeReg, Format: tar.FormatUSTAR}
		if err := tarWriter.WriteHeader(header); err != nil {
			input.Close()
			return fmt.Errorf("write archive member %s: %w", entry.name, err)
		}
		_, copyErr := io.Copy(tarWriter, input)
		closeErr := input.Close()
		if copyErr != nil {
			return fmt.Errorf("copy archive member %s: %w", entry.name, copyErr)
		}
		if closeErr != nil {
			return fmt.Errorf("close archive input %s: %w", entry.source, closeErr)
		}
	}
	if err := tarWriter.Close(); err != nil {
		return fmt.Errorf("finish tar archive: %w", err)
	}
	if err := gz.Close(); err != nil {
		return fmt.Errorf("finish gzip archive: %w", err)
	}
	if err := file.Close(); err != nil {
		return fmt.Errorf("close candidate archive: %w", err)
	}
	ok = true
	return nil
}

func writeCandidateZIP(path string, entries []archiveEntry) error {
	file, err := os.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o644)
	if err != nil {
		return fmt.Errorf("create candidate archive: %w", err)
	}
	ok := false
	defer func() {
		file.Close()
		if !ok {
			os.Remove(path)
		}
	}()
	writer := zip.NewWriter(file)
	for _, entry := range entries {
		info, err := os.Stat(entry.source)
		if err != nil {
			return fmt.Errorf("stat archive input %s: %w", entry.source, err)
		}
		if !info.Mode().IsRegular() {
			return fmt.Errorf("archive input is not a regular file: %s", entry.source)
		}
		header := &zip.FileHeader{Name: entry.name, Method: zip.Deflate}
		header.SetMode(info.Mode().Perm())
		output, err := writer.CreateHeader(header)
		if err != nil {
			return fmt.Errorf("create archive member %s: %w", entry.name, err)
		}
		input, err := os.Open(entry.source)
		if err != nil {
			return fmt.Errorf("open archive input %s: %w", entry.source, err)
		}
		_, copyErr := io.Copy(output, input)
		closeErr := input.Close()
		if copyErr != nil {
			return fmt.Errorf("copy archive member %s: %w", entry.name, copyErr)
		}
		if closeErr != nil {
			return fmt.Errorf("close archive input %s: %w", entry.source, closeErr)
		}
	}
	if err := writer.Close(); err != nil {
		return fmt.Errorf("finish zip archive: %w", err)
	}
	if err := file.Close(); err != nil {
		return fmt.Errorf("close candidate archive: %w", err)
	}
	ok = true
	return nil
}

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
