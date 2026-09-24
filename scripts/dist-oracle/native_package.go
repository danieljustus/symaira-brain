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
	for _, name := range append(append([]string(nil), archive.Files...), binaryName) {
		if filepath.Base(name) != name {
			return "", fmt.Errorf("archive member must be a filename: %s", name)
		}
	}
	if err := os.MkdirAll(assetsDir, 0o755); err != nil {
		return "", fmt.Errorf("create candidate directory: %w", err)
	}
	dirInfo, err := os.Lstat(assetsDir)
	if err != nil || !dirInfo.IsDir() || dirInfo.Mode()&os.ModeSymlink != 0 {
		return "", fmt.Errorf("native candidate path must be a real directory")
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
	checksumPath := filepath.Join(assetsDir, cfg.Checksum.NameTemplate)
	archiveCreated, checksumCreated, success := false, false, false
	defer func() {
		if !success {
			if checksumCreated {
				os.Remove(checksumPath)
			}
			if archiveCreated {
				os.Remove(archivePath)
			}
		}
	}()
	if err := writeCandidateArchive(archivePath, format, entries); err != nil {
		return "", err
	}
	archiveCreated = true
	members, err := candidateArchiveMembers(archivePath, archiveName)
	wantMembers := setOf(append(append([]string(nil), archive.Files...), binaryName))
	if err != nil || len(members) != len(wantMembers) {
		return "", fmt.Errorf("candidate archive members differ from config")
	}
	for _, member := range members {
		if _, ok := wantMembers[member]; !ok {
			return "", fmt.Errorf("candidate archive has unexpected member %q", member)
		}
	}
	data, err := os.ReadFile(archivePath)
	if err != nil {
		return "", fmt.Errorf("read candidate archive: %w", err)
	}
	digest := sha256.Sum256(data)
	checksum := fmt.Sprintf("%s  %s\n", hex.EncodeToString(digest[:]), archiveName)
	checksumFile, err := os.OpenFile(checksumPath, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o644)
	if err != nil {
		return "", fmt.Errorf("create candidate checksums: %w", err)
	}
	checksumCreated = true
	if _, err := checksumFile.WriteString(checksum); err != nil {
		checksumFile.Close()
		return "", fmt.Errorf("write candidate checksums: %w", err)
	}
	if err := checksumFile.Close(); err != nil {
		return "", fmt.Errorf("close candidate checksums: %w", err)
	}
	success = true
	return archiveName, nil
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
		return fmt.Errorf("candidate version identity does not match the expected tool, version, or schema")
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
	return len(lines) == 4 && lines[0] == "symbrain "+version && strings.HasPrefix(lines[1], "  rust    ") && strings.TrimSpace(strings.TrimPrefix(lines[1], "  rust    ")) != "" && lines[2] == "  os/arch "+goos+"/"+goarch && lines[3] == ""
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
		info, err := os.Lstat(entry.source)
		if err != nil {
			return fmt.Errorf("stat archive input %s: %w", entry.source, err)
		}
		if !info.Mode().IsRegular() || info.Mode()&os.ModeSymlink != 0 {
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
		info, err := os.Lstat(entry.source)
		if err != nil {
			return fmt.Errorf("stat archive input %s: %w", entry.source, err)
		}
		if !info.Mode().IsRegular() || info.Mode()&os.ModeSymlink != 0 {
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
