// Command dist-oracle makes the release-distribution contract rows
// (DIST-001 archive/binary names, DIST-002 checksums/signatures/SBOM/Homebrew)
// executable. It statically parses the in-repo .goreleaser.yml, derives the
// exact release-asset set that configuration yields for the pinned version,
// and asserts that set equals the golden manifest captured from the real
// GitHub release.
//
// The golden fixture (scripts/dist-oracle/fixtures/manifest_v0.12.0.json) is a
// real capture: `gh release view v0.11.0 --json ...assets`, real asset names,
// sizes and sha256 digests only. This tool is offline and deterministic; it
// never invents names and never claims to have run cosign/syft/goreleaser.
// Capabilities not exercised here are reported as explicit `not-run:` lines.
//
//	go run ./scripts/dist-oracle -check                      # gate (exit 0/1)
//	go run ./scripts/dist-oracle -check -manifest <path>     # point at another manifest copy (negative tests)
//	go run ./scripts/dist-oracle -candidate-check -version 0.12.0 -assets <archive-bundle>
//	go run ./scripts/dist-oracle -native-package -version 0.12.0 -binary target/<triple>/release/symbrain -assets <one-target-dir>
//	go run ./scripts/dist-oracle -candidate-merge -version 0.12.0 -packages <six-native-package-dirs> -assets <archive-bundle>
//
// The macOS GUI DMG is not produced by goreleaser; it is uploaded by
// .github/workflows/release.yml via `gh release upload`. It is therefore
// declared as an external asset here, and the workflow file is asserted to
// still bind that upload, so the extra manifest name stays traceable.
package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strings"

	"gopkg.in/yaml.v3"
)

const (
	defaultGoreleaser = ".goreleaser.yml"
	defaultManifest   = "scripts/dist-oracle/fixtures/manifest_v0.12.0.json"
	defaultWorkflow   = ".github/workflows/release.yml"

	// sigSuffix is goreleaser's default signature sidecar suffix. The config
	// only names `${signature}` (via cosign --output-signature); the .sig
	// suffix itself is corroborated by the captured release manifest below.
	sigSuffix = ".sig"
)

// ---- manifest fixture schema (real gh capture) ----

type manifestAsset struct {
	Name        string `json:"name"`
	Size        int64  `json:"size"`
	ContentType string `json:"content_type"`
	Digest      string `json:"digest"`
}

type manifest struct {
	SchemaVersion int             `json:"schema_version"`
	Source        string          `json:"source"`
	Repository    string          `json:"repository"`
	Tag           string          `json:"tag"`
	Version       string          `json:"version"`
	IsDraft       bool            `json:"is_draft"`
	IsPrerelease  bool            `json:"is_prerelease"`
	PublishedAt   string          `json:"published_at"`
	AssetCount    int             `json:"asset_count"`
	Assets        []manifestAsset `json:"assets"`
}

// ---- .goreleaser.yml schema (only the fields this gate asserts) ----

type gBuild struct {
	ID      string   `yaml:"id"`
	Main    string   `yaml:"main"`
	Binary  string   `yaml:"binary"`
	Env     []string `yaml:"env"`
	GOOS    []string `yaml:"goos"`
	GOARCH  []string `yaml:"goarch"`
	Ldflags []string `yaml:"ldflags"`
}

type gFormatOverride struct {
	GOOS    string   `yaml:"goos"`
	Formats []string `yaml:"formats"`
}

type gArchive struct {
	ID              string            `yaml:"id"`
	Formats         []string          `yaml:"formats"`
	NameTemplate    string            `yaml:"name_template"`
	FormatOverrides []gFormatOverride `yaml:"format_overrides"`
	Files           []string          `yaml:"files"`
}

type gChecksum struct {
	NameTemplate string `yaml:"name_template"`
	Algorithm    string `yaml:"algorithm"`
}

type gSBOM struct {
	Artifacts string   `yaml:"artifacts"`
	Documents []string `yaml:"documents"`
	Cmd       string   `yaml:"cmd"`
	Args      []string `yaml:"args"`
}

type gSign struct {
	Cmd         string   `yaml:"cmd"`
	Certificate string   `yaml:"certificate"`
	Args        []string `yaml:"args"`
	Artifacts   string   `yaml:"artifacts"`
}

type gBrewRepository struct {
	Owner string `yaml:"owner"`
	Name  string `yaml:"name"`
	Token string `yaml:"token"`
}

type gBrew struct {
	Name        string          `yaml:"name"`
	Repository  gBrewRepository `yaml:"repository"`
	Directory   string          `yaml:"directory"`
	Homepage    string          `yaml:"homepage"`
	Description string          `yaml:"description"`
	License     string          `yaml:"license"`
	Test        string          `yaml:"test"`
}

type gRelease struct {
	GitHub struct {
		Owner string `yaml:"owner"`
		Name  string `yaml:"name"`
	} `yaml:"github"`
	Draft        *bool  `yaml:"draft"`
	NameTemplate string `yaml:"name_template"`
}

type goreleaserConfig struct {
	Version     int        `yaml:"version"`
	ProjectName string     `yaml:"project_name"`
	Builds      []gBuild   `yaml:"builds"`
	Archives    []gArchive `yaml:"archives"`
	Checksum    *gChecksum `yaml:"checksum"`
	SBOMs       []gSBOM    `yaml:"sboms"`
	Signs       []gSign    `yaml:"signs"`
	Brews       []gBrew    `yaml:"brews"`
	Release     *gRelease  `yaml:"release"`
}

// ---- assertion harness ----

type checker struct {
	passed  int
	failed  int
	manPass int
	manFail int
}

// assert records one gate assertion. cat is "config" or "manifest"; manifest
// assertions are counted separately because the acceptance contract asks how
// many manifest assertions passed.
func (c *checker) assert(id, cat string, ok bool, detail string) {
	if ok {
		c.passed++
		if cat == "manifest" {
			c.manPass++
		}
		fmt.Printf("ok   %s: %s\n", id, detail)
		return
	}
	c.failed++
	if cat == "manifest" {
		c.manFail++
	}
	fmt.Printf("FAIL %s: %s\n", id, detail)
}

// ---- helpers ----

func resolvePath(p string) string {
	if _, err := os.Stat(p); err == nil {
		return p
	}
	dir, err := os.Getwd()
	if err != nil {
		return p
	}
	for i := 0; i < 12; i++ {
		cand := filepath.Join(dir, p)
		if _, err := os.Stat(cand); err == nil {
			return cand
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			break
		}
		dir = parent
	}
	return p
}

func setOf(items []string) map[string]struct{} {
	s := make(map[string]struct{}, len(items))
	for _, it := range items {
		s[it] = struct{}{}
	}
	return s
}

func sortedDiff(a, b map[string]struct{}) (onlyA, onlyB []string) {
	for k := range a {
		if _, ok := b[k]; !ok {
			onlyA = append(onlyA, k)
		}
	}
	for k := range b {
		if _, ok := a[k]; !ok {
			onlyB = append(onlyB, k)
		}
	}
	sort.Strings(onlyA)
	sort.Strings(onlyB)
	return onlyA, onlyB
}

func renderTemplate(tmpl, projectName, version, goos, goarch string) string {
	r := strings.NewReplacer(
		"{{ .ProjectName }}", projectName,
		"{{ .Version }}", version,
		"{{ .Os }}", goos,
		"{{ .Arch }}", goarch,
	)
	return r.Replace(tmpl)
}

func hasUnrenderedTokens(s string) bool {
	return strings.Contains(s, "{{") || strings.Contains(s, "}}")
}

// formatFor returns the archive format for a goos: a matching
// format_overrides entry wins over the section-level formats list.
func formatFor(a gArchive, goos string) string {
	for _, ov := range a.FormatOverrides {
		if ov.GOOS == goos && len(ov.Formats) > 0 {
			return ov.Formats[0]
		}
	}
	if len(a.Formats) > 0 {
		return a.Formats[0]
	}
	return ""
}

// deriveAssets computes the complete release-asset set that the parsed
// goreleaser config yields for the given version, plus the externally
// CI-uploaded assets (declared separately).
func deriveAssets(cfg *goreleaserConfig, version string) (goreleaserSet, externalSet map[string]struct{}, err error) {
	if len(cfg.Builds) == 0 || len(cfg.Archives) == 0 || cfg.Checksum == nil {
		return nil, nil, fmt.Errorf("config missing builds/archives/checksum section")
	}
	build := cfg.Builds[0]
	arch := cfg.Archives[0]

	goreleaserSet = make(map[string]struct{})

	// Archives: one per goos x goarch, name template rendered, format per OS.
	for _, goos := range build.GOOS {
		for _, goarch := range build.GOARCH {
			name := renderTemplate(arch.NameTemplate, cfg.ProjectName, version, goos, goarch)
			if hasUnrenderedTokens(name) {
				return nil, nil, fmt.Errorf("archive name template has unrendered tokens: %q", name)
			}
			goreleaserSet[name+"."+formatFor(arch, goos)] = struct{}{}
		}
	}

	// SBOM documents: one per archive artifact (`artifacts: archive`).
	var sbomDocs []string
	if len(cfg.SBOMs) > 0 && cfg.SBOMs[0].Artifacts == "archive" && len(cfg.SBOMs[0].Documents) > 0 {
		docTmpl := cfg.SBOMs[0].Documents[0]
		if !strings.HasPrefix(docTmpl, "${artifact}") {
			return nil, nil, fmt.Errorf("sbom document template does not derive from ${artifact}: %q", docTmpl)
		}
		sbomSuffix := strings.TrimPrefix(docTmpl, "${artifact}")
		for name := range goreleaserSet {
			sbomDocs = append(sbomDocs, name+sbomSuffix)
		}
		sort.Strings(sbomDocs)
		for _, d := range sbomDocs {
			goreleaserSet[d] = struct{}{}
		}
	}

	// Checksum file.
	checksumName := renderTemplate(cfg.Checksum.NameTemplate, cfg.ProjectName, version, "", "")
	if hasUnrenderedTokens(checksumName) {
		return nil, nil, fmt.Errorf("checksum name template has unrendered tokens: %q", checksumName)
	}
	goreleaserSet[checksumName] = struct{}{}

	// Signatures: `artifacts: all` signs archives, SBOM documents and the
	// checksum file; each signed artifact gets a .sig and a .pem sidecar
	// (certificate template `${artifact}.pem`).
	if len(cfg.Signs) > 0 && cfg.Signs[0].Artifacts == "all" {
		certTmpl := cfg.Signs[0].Certificate
		if !strings.HasPrefix(certTmpl, "${artifact}") {
			return nil, nil, fmt.Errorf("sign certificate template does not derive from ${artifact}: %q", certTmpl)
		}
		certSuffix := strings.TrimPrefix(certTmpl, "${artifact}")
		signed := make([]string, 0, len(goreleaserSet))
		for name := range goreleaserSet {
			signed = append(signed, name)
		}
		sort.Strings(signed)
		for _, name := range signed {
			goreleaserSet[name+sigSuffix] = struct{}{}
			goreleaserSet[name+certSuffix] = struct{}{}
		}
	}

	// Externally uploaded assets (not goreleaser-produced).
	externalSet = map[string]struct{}{
		fmt.Sprintf("Symaira-Brain-%s-macos.dmg", version): {},
	}
	return goreleaserSet, externalSet, nil
}

// notRunLines are capabilities this gate deliberately does not exercise.
// They are printed on every run so a green gate never implies more than what
// was actually asserted.
func notRunLines() []string {
	return []string{
		"not-run: cosign signature verification (naming-only gate; verifying bytes needs cosign + release key)",
		"not-run: archive rebuild / byte comparison (needs goreleaser + full cross toolchain)",
		"not-run: syft SBOM regeneration (needs syft; config only asserts CycloneDX naming)",
		"not-run: Homebrew formula install (needs homebrew-tap push + brew)",
		"not-run: sha256 digest recomputation against asset bytes (needs downloading all release assets; gate checks digest presence/format, values are pinned by the committed capture)",
		"not-run: live GitHub release fetch (this gate is offline+deterministic; live parity asserted separately via gh)",
	}
}

func main() {
	check := flag.Bool("check", false, "run the contract gate and exit non-zero on any failed assertion")
	candidateCheck := flag.Bool("candidate-check", false, "verify local candidate archives and checksums without a release")
	candidateSBOM := flag.Bool("candidate-sbom", false, "generate deterministic SPDX sidecars and checksums for an unpublished snapshot")
	nativePackage := flag.Bool("native-package", false, "verify a native Rust symbrain binary and package its archive/checksum")
	candidateMerge := flag.Bool("candidate-merge", false, "merge six native Rust packages and run the full candidate artifact check")
	assetsDir := flag.String("assets", "", "directory containing candidate archives and checksums.txt")
	packagesDir := flag.String("packages", "", "directory with six subdirectories from -native-package")
	binaryPath := flag.String("binary", "", "prebuilt Rust symbrain binary for -native-package")
	version := flag.String("version", "", "candidate version without the v prefix")
	manifestPath := flag.String("manifest", defaultManifest, "path to the pinned release manifest JSON")
	goreleaserPath := flag.String("goreleaser", defaultGoreleaser, "path to the goreleaser config")
	workflowPath := flag.String("workflow", defaultWorkflow, "path to the release workflow binding external assets")
	flag.Parse()

	if *candidateSBOM {
		if *assetsDir == "" || *version == "" {
			fmt.Fprintln(os.Stderr, "dist-oracle: -candidate-sbom requires -version and -assets")
			os.Exit(2)
		}
		goreleaserPathResolved := resolvePath(*goreleaserPath)
		raw, err := os.ReadFile(goreleaserPathResolved)
		if err != nil {
			fmt.Fprintf(os.Stderr, "dist-oracle: read %s: %v\n", goreleaserPathResolved, err)
			os.Exit(2)
		}
		var cfg goreleaserConfig
		if err := yaml.Unmarshal(raw, &cfg); err != nil {
			fmt.Fprintf(os.Stderr, "dist-oracle: parse %s: %v\n", goreleaserPathResolved, err)
			os.Exit(2)
		}
		if err := generateCandidateSPDX(&cfg, *version, resolvePath(*assetsDir)); err != nil {
			fmt.Fprintf(os.Stderr, "dist-oracle: generate candidate SPDX: %v\n", err)
			os.Exit(1)
		}
		fmt.Printf("candidate-spdx: PASS generated %d deterministic SPDX SBOMs and checksum entries for %s (unsigned)\n", len(cfg.Builds[0].GOOS)*len(cfg.Builds[0].GOARCH), *version)
		return
	}

	if *candidateMerge {
		if *assetsDir == "" || *version == "" || *packagesDir == "" {
			fmt.Fprintln(os.Stderr, "dist-oracle: -candidate-merge requires -version, -packages, and -assets")
			os.Exit(2)
		}
		goreleaserPathResolved := resolvePath(*goreleaserPath)
		raw, err := os.ReadFile(goreleaserPathResolved)
		if err != nil {
			fmt.Fprintf(os.Stderr, "dist-oracle: read %s: %v\n", goreleaserPathResolved, err)
			os.Exit(2)
		}
		var cfg goreleaserConfig
		if err := yaml.Unmarshal(raw, &cfg); err != nil {
			fmt.Fprintf(os.Stderr, "dist-oracle: parse %s: %v\n", goreleaserPathResolved, err)
			os.Exit(2)
		}
		if err := mergeNativeCandidatePackages(&cfg, *version, resolvePath(*packagesDir), resolvePath(*assetsDir)); err != nil {
			fmt.Fprintf(os.Stderr, "dist-oracle: merge native candidates: %v\n", err)
			os.Exit(1)
		}
		fmt.Printf("candidate-merge: PASS %d native targets for %s (archives, SPDX SBOMs, checksums; unsigned)\n", len(cfg.Builds[0].GOOS)*len(cfg.Builds[0].GOARCH), *version)
		return
	}

	if *nativePackage {
		if *assetsDir == "" || *version == "" || *binaryPath == "" {
			fmt.Fprintln(os.Stderr, "dist-oracle: -native-package requires -version, -binary, and -assets")
			os.Exit(2)
		}
		goreleaserPathResolved := resolvePath(*goreleaserPath)
		raw, err := os.ReadFile(goreleaserPathResolved)
		if err != nil {
			fmt.Fprintf(os.Stderr, "dist-oracle: read %s: %v\n", goreleaserPathResolved, err)
			os.Exit(2)
		}
		var cfg goreleaserConfig
		if err := yaml.Unmarshal(raw, &cfg); err != nil {
			fmt.Fprintf(os.Stderr, "dist-oracle: parse %s: %v\n", goreleaserPathResolved, err)
			os.Exit(2)
		}
		archive, err := packageNativeCandidate(&cfg, *version, *binaryPath, resolvePath(*assetsDir))
		if err != nil {
			fmt.Fprintf(os.Stderr, "dist-oracle: native candidate: %v\n", err)
			os.Exit(1)
		}
		fmt.Printf("native-candidate: PASS %s and SPDX SBOM %s.sbom.json %s/%s Rust symbrain %s (unsigned)\n", archive, archive, runtime.GOOS, runtime.GOARCH, *version)
		return
	}

	if *candidateCheck {
		if *assetsDir == "" || *version == "" {
			fmt.Fprintln(os.Stderr, "dist-oracle: -candidate-check requires -assets and -version")
			os.Exit(2)
		}
		goreleaserPathResolved := resolvePath(*goreleaserPath)
		raw, err := os.ReadFile(goreleaserPathResolved)
		if err != nil {
			fmt.Fprintf(os.Stderr, "dist-oracle: read %s: %v\n", goreleaserPathResolved, err)
			os.Exit(2)
		}
		var cfg goreleaserConfig
		if err := yaml.Unmarshal(raw, &cfg); err != nil {
			fmt.Fprintf(os.Stderr, "dist-oracle: parse %s: %v\n", goreleaserPathResolved, err)
			os.Exit(2)
		}
		if err := checkCandidateArtifacts(&cfg, *version, resolvePath(*assetsDir)); err != nil {
			fmt.Fprintf(os.Stderr, "dist-oracle: candidate artifacts: %v\n", err)
			os.Exit(1)
		}
		fmt.Printf("candidate-artifacts: PASS %d archives, SPDX SBOMs, and checksum entries for %s (unsigned)\n", len(cfg.Builds[0].GOOS)*len(cfg.Builds[0].GOARCH), *version)
		fmt.Println("not-run: release signatures/certificates, Homebrew metadata/install, DMG, and publication")
		return
	}

	if !*check {
		fmt.Fprintln(os.Stderr, "dist-oracle: refusing to run without -check (this tool only gates; it never writes fixtures)")
		flag.Usage()
		os.Exit(2)
	}

	goreleaserPathResolved := resolvePath(*goreleaserPath)
	manifestPathResolved := resolvePath(*manifestPath)
	workflowPathResolved := resolvePath(*workflowPath)

	raw, err := os.ReadFile(goreleaserPathResolved)
	if err != nil {
		fmt.Fprintf(os.Stderr, "dist-oracle: read %s: %v\n", goreleaserPathResolved, err)
		os.Exit(2)
	}
	var cfg goreleaserConfig
	if err := yaml.Unmarshal(raw, &cfg); err != nil {
		fmt.Fprintf(os.Stderr, "dist-oracle: parse %s: %v\n", goreleaserPathResolved, err)
		os.Exit(2)
	}

	mraw, err := os.ReadFile(manifestPathResolved)
	if err != nil {
		fmt.Fprintf(os.Stderr, "dist-oracle: read %s: %v\n", manifestPathResolved, err)
		os.Exit(2)
	}
	var mf manifest
	if err := json.Unmarshal(mraw, &mf); err != nil {
		fmt.Fprintf(os.Stderr, "dist-oracle: parse %s: %v\n", manifestPathResolved, err)
		os.Exit(2)
	}

	c := &checker{}

	// ---- config assertions (DIST-001/DIST-002 release configuration) ----
	c.assert("config.version-2", "config", cfg.Version == 2, fmt.Sprintf("goreleaser config version == 2 (got %d)", cfg.Version))
	c.assert("config.project-name", "config", cfg.ProjectName == "symbrain", fmt.Sprintf("project_name == symbrain (got %q)", cfg.ProjectName))

	if len(cfg.Builds) == 0 {
		c.assert("config.build-present", "config", false, "builds section present")
		fmt.Fprintf(os.Stderr, "dist-oracle: config has no builds; cannot derive contract\n")
		os.Exit(1)
	}
	b := cfg.Builds[0]
	c.assert("config.build-id", "config", b.ID == "symbrain", fmt.Sprintf("build id == symbrain (got %q)", b.ID))
	c.assert("config.binary-name", "config", b.Binary == "symbrain", fmt.Sprintf("binary == symbrain (got %q)", b.Binary))
	c.assert("config.build-main", "config", b.Main == "./cmd/symbrain", fmt.Sprintf("main == ./cmd/symbrain (got %q)", b.Main))
	cgo := false
	for _, e := range b.Env {
		if e == "CGO_ENABLED=0" {
			cgo = true
		}
	}
	c.assert("config.cgo-disabled", "config", cgo, "build env pins CGO_ENABLED=0")

	goosSet := setOf(b.GOOS)
	wantGOOS := setOf([]string{"darwin", "linux", "windows"})
	_, missGOOS := sortedDiff(wantGOOS, goosSet)
	_, extraGOOS := sortedDiff(wantGOOS, goosSet)
	c.assert("config.goos-set", "config", len(missGOOS) == 0 && len(extraGOOS) == 0,
		fmt.Sprintf("goos == {darwin,linux,windows} (missing=%v extra=%v)", missGOOS, extraGOOS))

	goarchSet := setOf(b.GOARCH)
	wantGOARCH := setOf([]string{"amd64", "arm64"})
	_, missGOARCH := sortedDiff(wantGOARCH, goarchSet)
	_, extraGOARCH := sortedDiff(wantGOARCH, goarchSet)
	c.assert("config.goarch-set", "config", len(missGOARCH) == 0 && len(extraGOARCH) == 0,
		fmt.Sprintf("goarch == {amd64,arm64} (missing=%v extra=%v)", missGOARCH, extraGOARCH))

	ldflagJoined := strings.Join(b.Ldflags, " ")
	c.assert("config.ldflags-version", "config", strings.Contains(ldflagJoined, "-X main.version="),
		"ldflags inject main.version so the binary reports the release version")

	if len(cfg.Archives) == 0 {
		c.assert("config.archive-present", "config", false, "archives section present")
		fmt.Fprintf(os.Stderr, "dist-oracle: config has no archives; cannot derive contract\n")
		os.Exit(1)
	}
	a := cfg.Archives[0]
	c.assert("config.archive-id", "config", a.ID == "default", fmt.Sprintf("archive id == default (got %q)", a.ID))
	formats := setOf(a.Formats)
	_, missTar := sortedDiff(setOf([]string{"tar.gz"}), formats)
	c.assert("config.archive-format-targz", "config", len(missTar) == 0, "default archive format is tar.gz")

	tokens := []string{"{{ .ProjectName }}", "{{ .Version }}", "{{ .Os }}", "{{ .Arch }}"}
	allTokens := true
	for _, t := range tokens {
		if !strings.Contains(a.NameTemplate, t) {
			allTokens = false
		}
	}
	c.assert("config.archive-name-template", "config", allTokens,
		"archive name_template is {{ .ProjectName }}_{{ .Version }}_{{ .Os }}_{{ .Arch }}")

	winZip := false
	for _, ov := range a.FormatOverrides {
		if ov.GOOS == "windows" {
			for _, f := range ov.Formats {
				if f == "zip" {
					winZip = true
				}
			}
		}
	}
	c.assert("config.archive-windows-zip", "config", winZip, "windows archives override to zip")

	c.assert("config.checksum-present", "config", cfg.Checksum != nil, "checksum section present")
	c.assert("config.checksum-name", "config", cfg.Checksum != nil && cfg.Checksum.NameTemplate == "checksums.txt",
		fmt.Sprintf("checksum name_template == checksums.txt (got %q)", derefStr(cfg.Checksum)))
	c.assert("config.checksum-sha256", "config", cfg.Checksum != nil && cfg.Checksum.Algorithm == "sha256",
		fmt.Sprintf("checksum algorithm == sha256 (got %q)", derefAlg(cfg.Checksum)))

	if len(cfg.SBOMs) == 0 {
		c.assert("config.sbom-present", "config", false, "sboms section present")
	} else {
		s := cfg.SBOMs[0]
		c.assert("config.sbom-artifacts-archive", "config", s.Artifacts == "archive",
			fmt.Sprintf("sbom artifacts == archive (got %q)", s.Artifacts))
		c.assert("config.sbom-document-template", "config", len(s.Documents) > 0 && s.Documents[0] == "${artifact}.sbom.json",
			fmt.Sprintf("sbom document == ${artifact}.sbom.json (got %v)", s.Documents))
		c.assert("config.sbom-cmd-syft", "config", s.Cmd == "syft", fmt.Sprintf("sbom cmd == syft (got %q)", s.Cmd))
		argsJoined := strings.Join(s.Args, " ")
		c.assert("config.sbom-cyclonedx", "config", strings.Contains(argsJoined, "cyclonedx-json=$document"),
			"sbom output format is cyclonedx-json (CycloneDX)")
	}

	if len(cfg.Signs) == 0 {
		c.assert("config.sign-present", "config", false, "signs section present")
	} else {
		s := cfg.Signs[0]
		c.assert("config.sign-cmd-cosign", "config", s.Cmd == "cosign", fmt.Sprintf("sign cmd == cosign (got %q)", s.Cmd))
		c.assert("config.sign-certificate-pem", "config", s.Certificate == "${artifact}.pem",
			fmt.Sprintf("certificate sidecar == ${artifact}.pem (got %q)", s.Certificate))
		signArgs := strings.Join(s.Args, " ")
		c.assert("config.sign-blob", "config", strings.Contains(signArgs, "sign-blob"), "cosign invoked with sign-blob")
		c.assert("config.sign-output-signature", "config", strings.Contains(signArgs, "--output-signature=${signature}"),
			"cosign writes --output-signature=${signature} sidecars")
		c.assert("config.sign-output-certificate", "config", strings.Contains(signArgs, "--output-certificate=${certificate}"),
			"cosign writes --output-certificate=${certificate} sidecars")
		c.assert("config.sign-artifacts-all", "config", s.Artifacts == "all",
			fmt.Sprintf("sign artifacts == all (archives, SBOMs and checksum are signed) (got %q)", s.Artifacts))
	}

	if len(cfg.Brews) == 0 {
		c.assert("config.brew-present", "config", false, "brews section present")
	} else {
		br := cfg.Brews[0]
		c.assert("config.brew-name", "config", br.Name == "symbrain", fmt.Sprintf("brew formula name == symbrain (got %q)", br.Name))
		c.assert("config.brew-tap", "config", br.Repository.Owner == "danieljustus" && br.Repository.Name == "homebrew-tap",
			fmt.Sprintf("brew tap == danieljustus/homebrew-tap (got %s/%s)", br.Repository.Owner, br.Repository.Name))
		c.assert("config.brew-directory", "config", br.Directory == "Formula", fmt.Sprintf("brew directory == Formula (got %q)", br.Directory))
		c.assert("config.brew-metadata", "config",
			br.Homepage != "" && br.Description != "" && br.License != "",
			"brew formula declares homepage, description and license")
		c.assert("config.brew-test-binary", "config",
			strings.Contains(br.Test, "#{bin}/symbrain") || strings.Contains(br.Test, "bin/symbrain"),
			"brew test runs the installed symbrain binary")
	}

	if cfg.Release == nil {
		c.assert("config.release-present", "config", false, "release section present")
	} else {
		c.assert("config.release-repo", "config", cfg.Release.GitHub.Owner == "danieljustus" && cfg.Release.GitHub.Name == "symaira-brain",
			fmt.Sprintf("release target == danieljustus/symaira-brain (got %s/%s)", cfg.Release.GitHub.Owner, cfg.Release.GitHub.Name))
		c.assert("config.release-not-draft", "config", cfg.Release.Draft != nil && !*cfg.Release.Draft,
			"release.draft == false (published releases)")
		c.assert("config.release-name-template", "config", cfg.Release.NameTemplate == "v{{.Version}}",
			fmt.Sprintf("release name_template == v{{.Version}} (got %q)", cfg.Release.NameTemplate))
	}

	// ---- manifest assertions (pinned real capture vs derived contract) ----
	c.assert("manifest.tag-version", "manifest", mf.Tag == "v"+mf.Version,
		fmt.Sprintf("tag %q is v-prefixed %q", mf.Tag, mf.Version))
	c.assert("manifest.published-not-draft", "manifest", !mf.IsDraft,
		fmt.Sprintf("captured release is published (is_draft=%v, is_prerelease=%v)", mf.IsDraft, mf.IsPrerelease))
	c.assert("manifest.asset-count-matches", "manifest", mf.AssetCount == len(mf.Assets),
		fmt.Sprintf("asset_count %d == len(assets) %d", mf.AssetCount, len(mf.Assets)))

	manifestNames := make([]string, 0, len(mf.Assets))
	manifestSet := make(map[string]struct{}, len(mf.Assets))
	allDigests := true
	for _, asset := range mf.Assets {
		manifestNames = append(manifestNames, asset.Name)
		manifestSet[asset.Name] = struct{}{}
		if !strings.HasPrefix(asset.Digest, "sha256:") || len(asset.Digest) != len("sha256:")+64 {
			allDigests = false
		}
	}
	sort.Strings(manifestNames)
	dupes := []string{}
	seen := map[string]struct{}{}
	for _, n := range manifestNames {
		if _, ok := seen[n]; ok {
			dupes = append(dupes, n)
		}
		seen[n] = struct{}{}
	}
	c.assert("manifest.no-duplicate-assets", "manifest", len(dupes) == 0,
		fmt.Sprintf("no duplicate asset names (dupes=%v)", dupes))
	c.assert("manifest.digests-sha256", "manifest", allDigests,
		"every captured asset carries a sha256: digest (real capture provenance)")

	// Per-category presence assertions derived from the pinned version.
	derived, external, err := deriveAssets(&cfg, mf.Version)
	if err != nil {
		fmt.Fprintf(os.Stderr, "dist-oracle: derive assets: %v\n", err)
		os.Exit(1)
	}

	// Platform coverage: every configured goos×goarch archive present.
	missingPlatforms := []string{}
	for _, goos := range b.GOOS {
		for _, goarch := range b.GOARCH {
			name := renderTemplate(a.NameTemplate, cfg.ProjectName, mf.Version, goos, goarch) + "." + formatFor(a, goos)
			if _, ok := manifestSet[name]; !ok {
				missingPlatforms = append(missingPlatforms, name)
			}
		}
	}
	sort.Strings(missingPlatforms)
	c.assert("manifest.platform-coverage", "manifest", len(missingPlatforms) == 0,
		fmt.Sprintf("all %d goos×goarch archives present (missing=%v)", len(b.GOOS)*len(b.GOARCH), missingPlatforms))

	// Checksum trio present.
	checksumBase := "checksums.txt"
	if cfg.Checksum != nil {
		checksumBase = cfg.Checksum.NameTemplate
	}
	missingChecksum := []string{}
	for _, n := range []string{checksumBase, checksumBase + sigSuffix, checksumBase + ".pem"} {
		if _, ok := manifestSet[n]; !ok {
			missingChecksum = append(missingChecksum, n)
		}
	}
	c.assert("manifest.checksum-sidecars", "manifest", len(missingChecksum) == 0,
		fmt.Sprintf("%s + %s + .pem all published (missing=%v)", checksumBase, sigSuffix, missingChecksum))

	// SBOM coverage: every archive has a .sbom.json with both sidecars.
	missingSBOM := []string{}
	for name := range derived {
		if !strings.HasSuffix(name, ".sbom.json") {
			continue
		}
		for _, n := range []string{name, name + sigSuffix, name + ".pem"} {
			if _, ok := manifestSet[n]; !ok {
				missingSBOM = append(missingSBOM, n)
			}
		}
	}
	sort.Strings(missingSBOM)
	sbomDerived := 0
	for name := range derived {
		if strings.HasSuffix(name, ".sbom.json") && !strings.HasSuffix(name, sigSuffix) {
			sbomDerived++
		}
	}
	c.assert("manifest.sbom-sidecars", "manifest", sbomDerived > 0 && len(missingSBOM) == 0,
		fmt.Sprintf("all %d SBOM documents published with .sig+.pem (missing=%v)", sbomDerived, missingSBOM))

	// Exact set equality: derived(goreleaser) ∪ external(CI dmg) == manifest.
	expectedSet := make(map[string]struct{}, len(derived)+len(external))
	for k := range derived {
		expectedSet[k] = struct{}{}
	}
	for k := range external {
		expectedSet[k] = struct{}{}
	}
	missing, extra := sortedDiff(expectedSet, manifestSet)
	c.assert("manifest.set-equality", "manifest", len(missing) == 0 && len(extra) == 0,
		fmt.Sprintf("manifest asset set == config-derived set (missing=%v unexpected=%v)", missing, extra))

	// External DMG must stay bound to its producer workflow.
	workflowRaw, werr := os.ReadFile(workflowPathResolved)
	workflowStr := string(workflowRaw)
	bound := werr == nil &&
		strings.Contains(workflowStr, "gh release upload") &&
		strings.Contains(workflowStr, "Symaira-Brain-$VERSION-macos.dmg")
	c.assert("manifest.external-dmg-bound", "manifest", bound,
		"external DMG asset is uploaded by release.yml (gh release upload Symaira-Brain-$VERSION-macos.dmg)")

	// ---- report ----
	total := c.passed + c.failed
	if c.failed == 0 {
		fmt.Printf("dist-oracle: PASS %d/%d assertions passed (manifest assertions: %d)\n", total, total, c.manPass)
	} else {
		fmt.Printf("dist-oracle: FAIL %d/%d assertions passed, %d failed (manifest: %d passed, %d failed)\n",
			c.passed, total, c.failed, c.manPass, c.manFail)
	}
	fmt.Printf("dist-oracle: golden manifest %s (%s, %d assets, tag %s)\n",
		filepath.Base(manifestPathResolved), mf.Source, mf.AssetCount, mf.Tag)
	for _, line := range notRunLines() {
		fmt.Println(line)
	}
	if c.failed > 0 {
		os.Exit(1)
	}
}

func derefStr(c *gChecksum) string {
	if c == nil {
		return "<nil>"
	}
	return c.NameTemplate
}

func derefAlg(c *gChecksum) string {
	if c == nil {
		return "<nil>"
	}
	return c.Algorithm
}
