package main

import (
	"crypto/sha1"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

type spdxChecksum struct {
	Algorithm     string `json:"algorithm"`
	ChecksumValue string `json:"checksumValue"`
}

type spdxFile struct {
	FileName         string         `json:"fileName"`
	SPDXID           string         `json:"SPDXID"`
	Checksums        []spdxChecksum `json:"checksums"`
	LicenseConcluded string         `json:"licenseConcluded"`
	LicenseInfo      []string       `json:"licenseInfoInFiles"`
	CopyrightText    string         `json:"copyrightText"`
}

type spdxPackage struct {
	Name             string               `json:"name"`
	SPDXID           string               `json:"SPDXID"`
	VersionInfo      string               `json:"versionInfo"`
	DownloadLocation string               `json:"downloadLocation"`
	FilesAnalyzed    bool                 `json:"filesAnalyzed"`
	VerificationCode spdxVerificationCode `json:"packageVerificationCode"`
	Checksums        []spdxChecksum       `json:"checksums"`
	LicenseConcluded string               `json:"licenseConcluded"`
	LicenseDeclared  string               `json:"licenseDeclared"`
	CopyrightText    string               `json:"copyrightText"`
}

type spdxVerificationCode struct {
	Value string `json:"packageVerificationCodeValue"`
}

type archiveFileDigest struct {
	SHA256 string
	SHA1   string
}

type spdxRelationship struct {
	SPDXElementID      string `json:"spdxElementId"`
	RelationshipType   string `json:"relationshipType"`
	RelatedSPDXElement string `json:"relatedSpdxElement"`
}

type spdxDocument struct {
	SPDXVersion       string             `json:"spdxVersion"`
	DataLicense       string             `json:"dataLicense"`
	SPDXID            string             `json:"SPDXID"`
	Name              string             `json:"name"`
	DocumentNamespace string             `json:"documentNamespace"`
	CreationInfo      map[string]any     `json:"creationInfo"`
	Packages          []spdxPackage      `json:"packages"`
	Files             []spdxFile         `json:"files"`
	Relationships     []spdxRelationship `json:"relationships"`
	Comment           string             `json:"comment,omitempty"`
}

func candidateSBOMName(cfg *goreleaserConfig, archiveName string) (string, error) {
	if len(cfg.SBOMs) != 1 || cfg.SBOMs[0].Artifacts != "archive" || len(cfg.SBOMs[0].Documents) != 1 {
		return "", fmt.Errorf("config must define one archive SBOM document")
	}
	template := cfg.SBOMs[0].Documents[0]
	name := strings.ReplaceAll(template, "${artifact}", archiveName)
	if !strings.Contains(template, "${artifact}") || strings.Contains(name, "${") || filepath.Base(name) != name || !strings.HasSuffix(name, ".sbom.json") {
		return "", fmt.Errorf("cannot derive configured SBOM name for %s", archiveName)
	}
	return name, nil
}

func candidateSPDX(archivePath, archiveName, version, archiveDigest string) ([]byte, error) {
	digests, err := candidateArchiveFileDigests(archivePath, archiveName)
	if err != nil {
		return nil, fmt.Errorf("read candidate archive for SPDX: %w", err)
	}
	fileNames := make([]string, 0, len(digests))
	for name := range digests {
		fileNames = append(fileNames, name)
	}
	sort.Strings(fileNames)
	document := spdxDocument{
		SPDXVersion:       "SPDX-2.3",
		DataLicense:       "CC0-1.0",
		SPDXID:            "SPDXRef-DOCUMENT",
		Name:              archiveName + ".sbom.json",
		DocumentNamespace: "https://spdx.symaira.dev/symbrain/" + version + "/" + archiveName + "#sha256-" + archiveDigest,
		CreationInfo: map[string]any{
			"created":  "1970-01-01T00:00:00Z",
			"creators": []string{"Tool: scripts/dist-oracle"},
		},
		Packages: []spdxPackage{{
			Name: "symbrain", SPDXID: "SPDXRef-Package-symbrain", VersionInfo: version,
			DownloadLocation: "NOASSERTION", FilesAnalyzed: true,
			VerificationCode: spdxVerificationCode{Value: spdxPackageVerificationCode(digests)},
			Checksums:        []spdxChecksum{{Algorithm: "SHA256", ChecksumValue: archiveDigest}},
			LicenseConcluded: "NOASSERTION", LicenseDeclared: "Apache-2.0", CopyrightText: "NOASSERTION",
		}},
		Comment: "Archive file inventory only; transitive dependency completeness is not asserted.",
	}
	for index, name := range fileNames {
		id := fmt.Sprintf("SPDXRef-File-%d", index+1)
		document.Files = append(document.Files, spdxFile{
			FileName: name, SPDXID: id,
			Checksums: []spdxChecksum{
				{Algorithm: "SHA1", ChecksumValue: digests[name].SHA1},
				{Algorithm: "SHA256", ChecksumValue: digests[name].SHA256},
			},
			LicenseConcluded: "NOASSERTION", LicenseInfo: []string{"NOASSERTION"}, CopyrightText: "NOASSERTION",
		})
		document.Relationships = append(document.Relationships, spdxRelationship{
			SPDXElementID: "SPDXRef-Package-symbrain", RelationshipType: "CONTAINS", RelatedSPDXElement: id,
		})
	}
	document.Relationships = append([]spdxRelationship{{
		SPDXElementID: "SPDXRef-DOCUMENT", RelationshipType: "DESCRIBES", RelatedSPDXElement: "SPDXRef-Package-symbrain",
	}}, document.Relationships...)
	return json.MarshalIndent(document, "", "  ")
}

func spdxPackageVerificationCode(digests map[string]archiveFileDigest) string {
	fileHashes := make([]string, 0, len(digests))
	for _, digest := range digests {
		fileHashes = append(fileHashes, digest.SHA1)
	}
	sort.Strings(fileHashes)
	packageHash := sha1.Sum([]byte(strings.Join(fileHashes, "")))
	return hex.EncodeToString(packageHash[:])
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
	type candidate struct{ name, path, sbomName, sbomPath string }
	var candidates []candidate
	var checksums []string
	seen := make(map[string]struct{}, len(expected))
	for _, pkg := range packages {
		if !pkg.IsDir() || pkg.Type()&os.ModeSymlink != 0 {
			return fmt.Errorf("native package is not a directory: %s", pkg.Name())
		}
		pkgPath := filepath.Join(packagesDir, pkg.Name())
		files, err := os.ReadDir(pkgPath)
		if err != nil || len(files) != 3 {
			return fmt.Errorf("native package %s must contain one archive, its SBOM, and checksums.txt", pkg.Name())
		}
		var archiveName, sbomName string
		checksumPresent := false
		for _, file := range files {
			if file.Name() == cfg.Checksum.NameTemplate {
				checksumPresent = true
			} else if _, ok := expected[file.Name()]; ok {
				archiveName = file.Name()
			} else if strings.HasSuffix(file.Name(), ".sbom.json") {
				sbomName = file.Name()
			} else {
				return fmt.Errorf("unexpected native package asset %q", file.Name())
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
		wantSBOM, err := candidateSBOMName(cfg, archiveName)
		if err != nil || sbomName != wantSBOM {
			return fmt.Errorf("native package %s SBOM does not match its archive", pkg.Name())
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
		sbomPath := filepath.Join(pkgPath, sbomName)
		fileDigests, err := candidateArchiveFileDigests(archivePath, archiveName)
		if err != nil {
			return err
		}
		if err := verifyCandidateSBOM(sbomPath, archiveName, version, checksum, fileDigests); err != nil {
			return err
		}
		sbomData, err := os.ReadFile(sbomPath)
		if err != nil {
			return fmt.Errorf("read native package SBOM %s: %w", sbomName, err)
		}
		sbomDigest := sha256.Sum256(sbomData)
		sbomChecksum := hex.EncodeToString(sbomDigest[:])
		if err := verifyCandidateChecksums(
			filepath.Join(pkgPath, cfg.Checksum.NameTemplate),
			map[string]string{archiveName: checksum, sbomName: sbomChecksum},
		); err != nil {
			return fmt.Errorf("verify native package %s: %w", pkg.Name(), err)
		}
		candidates = append(candidates, candidate{
			name: archiveName, path: archivePath, sbomName: sbomName, sbomPath: sbomPath,
		})
		checksums = append(checksums, checksum+"  "+archiveName, sbomChecksum+"  "+sbomName)
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
		if err := copyExclusive(candidate.sbomPath, filepath.Join(assetsDir, candidate.sbomName)); err != nil {
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
