package main

import (
	"archive/tar"
	"archive/zip"
	"compress/gzip"
	"crypto/sha1"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"os"
	pathpkg "path"
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
	entries, err := os.ReadDir(assetsDir)
	if err != nil {
		return fmt.Errorf("read candidate directory: %w", err)
	}
	expectedNames := make(map[string]struct{}, len(expectedArchives)*2+1)
	expectedSBOMs := make(map[string]string, len(expectedArchives))
	for name := range expectedArchives {
		expectedNames[name] = struct{}{}
		sbomName, err := candidateSBOMName(cfg, name)
		if err != nil {
			return err
		}
		expectedNames[sbomName] = struct{}{}
		expectedSBOMs[name] = sbomName
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
	checksums := make(map[string]string, len(archiveNames)*2)
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
		archiveDigest := hex.EncodeToString(digest[:])
		checksums[name] = archiveDigest
		fileDigests, err := candidateArchiveFileDigests(files[name], name)
		if err != nil {
			return err
		}
		sbomName := expectedSBOMs[name]
		if err := verifyCandidateSBOM(files[sbomName], name, version, archiveDigest, fileDigests); err != nil {
			return err
		}
		sbomData, err := os.ReadFile(files[sbomName])
		if err != nil {
			return fmt.Errorf("read candidate SPDX SBOM %s: %w", sbomName, err)
		}
		sbomDigest := sha256.Sum256(sbomData)
		checksums[sbomName] = hex.EncodeToString(sbomDigest[:])
	}
	if err := verifyCandidateChecksums(files[checksumName], checksums); err != nil {
		return err
	}
	return nil
}

func verifyCandidateSBOM(path, archiveName, version, archiveDigest string, fileDigests map[string]archiveFileDigest) error {
	data, err := os.ReadFile(path)
	if err != nil {
		return fmt.Errorf("read candidate SPDX SBOM %s: %w", filepath.Base(path), err)
	}
	var document spdxDocument
	decoder := json.NewDecoder(strings.NewReader(string(data)))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&document); err != nil {
		return fmt.Errorf("parse candidate SPDX SBOM %s: %w", filepath.Base(path), err)
	}
	if err := decoder.Decode(new(any)); err != io.EOF {
		return fmt.Errorf("candidate SPDX SBOM %s has trailing data", filepath.Base(path))
	}
	name := archiveName + ".sbom.json"
	namespace := "https://spdx.symaira.dev/symbrain/" + version + "/" + archiveName + "#sha256-" + archiveDigest
	if document.SPDXVersion != "SPDX-2.3" || document.DataLicense != "CC0-1.0" ||
		document.SPDXID != "SPDXRef-DOCUMENT" || document.Name != name || document.DocumentNamespace != namespace {
		return fmt.Errorf("candidate SPDX SBOM %s has mismatched document identity", filepath.Base(path))
	}
	if len(document.Packages) != 1 {
		return fmt.Errorf("candidate SPDX SBOM %s must describe exactly one product package", filepath.Base(path))
	}
	pkg := document.Packages[0]
	if pkg.Name != "symbrain" || pkg.SPDXID != "SPDXRef-Package-symbrain" || pkg.VersionInfo != version ||
		pkg.DownloadLocation != "NOASSERTION" || !pkg.FilesAnalyzed || pkg.LicenseDeclared != "Apache-2.0" ||
		pkg.LicenseConcluded != "NOASSERTION" || pkg.CopyrightText != "NOASSERTION" ||
		len(pkg.Checksums) != 1 || pkg.Checksums[0] != (spdxChecksum{Algorithm: "SHA256", ChecksumValue: archiveDigest}) ||
		pkg.VerificationCode.Value != spdxPackageVerificationCode(fileDigests) {
		return fmt.Errorf("candidate SPDX SBOM %s has mismatched package identity or archive checksum", filepath.Base(path))
	}
	creators, ok := document.CreationInfo["creators"].([]any)
	if document.CreationInfo["created"] != "1970-01-01T00:00:00Z" || !ok || len(creators) != 1 || creators[0] != "Tool: scripts/dist-oracle" {
		return fmt.Errorf("candidate SPDX SBOM %s has unexpected creation metadata", filepath.Base(path))
	}
	if len(document.Files) != len(fileDigests) || len(document.Relationships) != len(fileDigests)+1 {
		return fmt.Errorf("candidate SPDX SBOM %s file inventory is incomplete", filepath.Base(path))
	}
	fileNames := make([]string, 0, len(fileDigests))
	for fileName := range fileDigests {
		fileNames = append(fileNames, fileName)
	}
	sort.Strings(fileNames)
	for index, fileName := range fileNames {
		file := document.Files[index]
		if file.FileName != fileName || file.SPDXID != fmt.Sprintf("SPDXRef-File-%d", index+1) ||
			len(file.Checksums) != 2 ||
			file.Checksums[0] != (spdxChecksum{Algorithm: "SHA1", ChecksumValue: fileDigests[fileName].SHA1}) ||
			file.Checksums[1] != (spdxChecksum{Algorithm: "SHA256", ChecksumValue: fileDigests[fileName].SHA256}) ||
			file.LicenseConcluded != "NOASSERTION" || len(file.LicenseInfo) != 1 || file.LicenseInfo[0] != "NOASSERTION" ||
			file.CopyrightText != "NOASSERTION" {
			return fmt.Errorf("candidate SPDX SBOM %s file inventory mismatch at %s", filepath.Base(path), fileName)
		}
		relationship := document.Relationships[index+1]
		if relationship != (spdxRelationship{
			SPDXElementID: "SPDXRef-Package-symbrain", RelationshipType: "CONTAINS", RelatedSPDXElement: file.SPDXID,
		}) {
			return fmt.Errorf("candidate SPDX SBOM %s relationship mismatch at %s", filepath.Base(path), fileName)
		}
	}
	if document.Relationships[0] != (spdxRelationship{
		SPDXElementID: "SPDXRef-DOCUMENT", RelationshipType: "DESCRIBES", RelatedSPDXElement: "SPDXRef-Package-symbrain",
	}) || document.Comment != "Archive file inventory only; transitive dependency completeness is not asserted." {
		return fmt.Errorf("candidate SPDX SBOM %s is missing its declared archive-only scope", filepath.Base(path))
	}
	return nil
}

func candidateArchiveFileDigests(archivePath, archiveName string) (map[string]archiveFileDigest, error) {
	digests := make(map[string]archiveFileDigest)
	add := func(name string, reader io.Reader) error {
		clean := pathpkg.Clean(name)
		if name == "" || strings.Contains(name, "\\") || pathpkg.IsAbs(name) || clean == "." || clean == ".." || strings.HasPrefix(clean, "../") {
			return fmt.Errorf("archive %s contains unsafe member %q", archiveName, name)
		}
		if _, exists := digests[name]; exists {
			return fmt.Errorf("archive %s contains duplicate member %q", archiveName, name)
		}
		sha256Digest, sha1Digest := sha256.New(), sha1.New()
		if _, err := io.Copy(io.MultiWriter(sha256Digest, sha1Digest), reader); err != nil {
			return fmt.Errorf("hash archive member %s: %w", name, err)
		}
		digests[name] = archiveFileDigest{
			SHA256: hex.EncodeToString(sha256Digest.Sum(nil)),
			SHA1:   hex.EncodeToString(sha1Digest.Sum(nil)),
		}
		return nil
	}
	if strings.HasSuffix(archiveName, ".tar.gz") {
		file, err := os.Open(archivePath)
		if err != nil {
			return nil, fmt.Errorf("open archive %s: %w", archiveName, err)
		}
		defer file.Close()
		gz, err := gzip.NewReader(file)
		if err != nil {
			return nil, fmt.Errorf("open gzip archive %s: %w", archiveName, err)
		}
		defer gz.Close()
		reader := tar.NewReader(gz)
		for {
			header, err := reader.Next()
			if err == io.EOF {
				break
			}
			if err != nil {
				return nil, fmt.Errorf("read tar archive %s: %w", archiveName, err)
			}
			if header.Typeflag != tar.TypeReg && header.Typeflag != tar.TypeRegA {
				return nil, fmt.Errorf("archive %s contains non-regular entry %q", archiveName, header.Name)
			}
			if err := add(header.Name, reader); err != nil {
				return nil, err
			}
		}
		return digests, nil
	}
	if strings.HasSuffix(archiveName, ".zip") {
		archive, err := zip.OpenReader(archivePath)
		if err != nil {
			return nil, fmt.Errorf("open zip archive %s: %w", archiveName, err)
		}
		defer archive.Close()
		for _, member := range archive.File {
			if member.FileInfo().IsDir() || !member.Mode().IsRegular() {
				return nil, fmt.Errorf("archive %s contains non-regular entry %q", archiveName, member.Name)
			}
			reader, err := member.Open()
			if err != nil {
				return nil, fmt.Errorf("open archive member %s: %w", member.Name, err)
			}
			hashErr := add(member.Name, reader)
			closeErr := reader.Close()
			if hashErr != nil {
				return nil, hashErr
			}
			if closeErr != nil {
				return nil, fmt.Errorf("close archive member %s: %w", member.Name, closeErr)
			}
		}
		return digests, nil
	}
	return nil, fmt.Errorf("unsupported archive format for %s", archiveName)
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

func verifyCandidateChecksums(path string, expected map[string]string) error {
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
	if len(got) != len(expected) {
		return fmt.Errorf("checksums.txt has %d entries, want %d archive and SBOM entries", len(got), len(expected))
	}
	for name, digest := range expected {
		if got[name] != digest {
			return fmt.Errorf("checksums.txt digest mismatch for %s", name)
		}
	}
	for name := range got {
		if _, exists := expected[name]; !exists {
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
