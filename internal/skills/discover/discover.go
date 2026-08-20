// Package discover exposes read-only discovery of unmanaged skill sources in known harness roots and explicit paths.
package discover

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"os"
	"path/filepath"
	"sort"

	"github.com/danieljustus/symaira-brain/internal/skills/harness"
	"github.com/danieljustus/symaira-brain/internal/skills/render"
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

type Candidate struct {
	SourceID    string        `json:"source_id"`
	Target      render.Target `json:"target,omitempty"`
	Kind        string        `json:"kind"`
	DisplayName string        `json:"display_name"`
	Location    string        `json:"location"`
	Managed     bool          `json:"managed"`
	Valid       bool          `json:"valid"`
	Source      string        `json:"source"`
	Status      string        `json:"status"`
	Diagnostics []string      `json:"diagnostics,omitempty"`
}

type Options struct {
	HomeDir    string       `json:"home_dir,omitempty"`
	ProjectDir string       `json:"project_dir,omitempty"`
	Scope      render.Scope `json:"scope,omitempty"`
	Paths      []string     `json:"paths,omitempty"`
}

type scanRoot struct {
	path   string
	source string
	target render.Target
}

// DiscoverScanned inspects documented harness roots and explicit paths for skill candidates without writing or copying content.
func DiscoverScanned(opts Options) ([]Candidate, error) {
	if opts.HomeDir == "" {
		if h, err := os.UserHomeDir(); err == nil {
			opts.HomeDir = h
		}
	}
	if opts.Scope == "" {
		opts.Scope = render.ScopeUser
	}

	var roots []scanRoot
	for _, desc := range harness.Descriptors() {
		rPath := desc.SkillRoot(opts.HomeDir, opts.ProjectDir, opts.Scope)
		roots = append(roots, scanRoot{
			path:   rPath,
			source: fmt.Sprintf("harness-root:%s", desc.Target),
			target: desc.Target,
		})
	}

	for _, p := range opts.Paths {
		if p != "" {
			abs, err := filepath.Abs(p)
			if err != nil {
				abs = p
			}
			roots = append(roots, scanRoot{
				path:   abs,
				source: "explicit-path",
				target: "",
			})
		}
	}

	candidatesMap := make(map[string]Candidate)

	for _, r := range roots {
		scanDirectory(r, candidatesMap)
	}

	results := make([]Candidate, 0, len(candidatesMap))
	for _, c := range candidatesMap {
		results = append(results, c)
	}

	sort.Slice(results, func(i, j int) bool {
		if results[i].Location != results[j].Location {
			return results[i].Location < results[j].Location
		}
		return results[i].SourceID < results[j].SourceID
	})

	return results, nil
}

func scanDirectory(r scanRoot, out map[string]Candidate) {
	info, err := os.Lstat(r.path)
	if err != nil {
		if r.source == "explicit-path" {
			// Record diagnostic candidate for missing explicit path
			id := computeSourceID(r.path)
			out[id] = Candidate{
				SourceID:    id,
				Target:      r.target,
				Kind:        "skill_bundle",
				DisplayName: filepath.Base(r.path),
				Location:    r.path,
				Managed:     false,
				Valid:       false,
				Source:      r.source,
				Status:      "unreadable",
				Diagnostics: []string{fmt.Sprintf("path not accessible: %v", err)},
			}
		}
		return
	}

	// Check if r.path is itself a skill directory (contains SKILL.md directly)
	skillMDPath := filepath.Join(r.path, "SKILL.md")
	if _, err := os.Stat(skillMDPath); err == nil {
		cand := inspectCandidate(r.path, r.source, r.target)
		if cand.SourceID != "" {
			out[cand.SourceID] = cand
		}
		return
	}

	if !info.IsDir() && (info.Mode()&os.ModeSymlink == 0) {
		return
	}

	entries, err := os.ReadDir(r.path)
	if err != nil {
		if r.source == "explicit-path" {
			id := computeSourceID(r.path)
			out[id] = Candidate{
				SourceID:    id,
				Target:      r.target,
				Kind:        "skill_bundle",
				DisplayName: filepath.Base(r.path),
				Location:    r.path,
				Managed:     false,
				Valid:       false,
				Source:      r.source,
				Status:      "unreadable",
				Diagnostics: []string{fmt.Sprintf("read directory: %v", err)},
			}
		}
		return
	}

	for _, entry := range entries {
		childPath := filepath.Join(r.path, entry.Name())
		// Look for SKILL.md inside childPath
		childSkillMD := filepath.Join(childPath, "SKILL.md")
		if _, err := os.Stat(childSkillMD); err == nil {
			cand := inspectCandidate(childPath, r.source, r.target)
			if cand.SourceID != "" {
				// Avoid overwriting a candidate if already found under a specific target
				if existing, ok := out[cand.SourceID]; ok {
					if existing.Target == "" && cand.Target != "" {
						out[cand.SourceID] = cand
					}
				} else {
					out[cand.SourceID] = cand
				}
			}
		}
	}
}

func inspectCandidate(dirPath, source string, target render.Target) Candidate {
	evalPath := dirPath
	if resolved, err := filepath.EvalSymlinks(dirPath); err == nil {
		evalPath = resolved
	}

	sourceID := computeSourceID(evalPath)
	displayName := filepath.Base(dirPath)

	managed := isManagedDir(dirPath) || isManagedDir(evalPath)
	status := "candidate"
	if managed {
		status = "managed"
	}

	cand := Candidate{
		SourceID:    sourceID,
		Target:      target,
		Kind:        "skill_bundle",
		DisplayName: displayName,
		Location:    dirPath,
		Managed:     managed,
		Valid:       true,
		Source:      source,
		Status:      status,
	}

	bundle, err := skill.LoadBundle(dirPath)
	if err != nil {
		cand.Valid = false
		cand.Status = "invalid"
		cand.Diagnostics = append(cand.Diagnostics, err.Error())
		return cand
	}

	if bundle.Frontmatter.Name != "" {
		cand.DisplayName = bundle.Frontmatter.Name
	}

	issues := skill.Validate(bundle)
	for _, issue := range issues {
		cand.Diagnostics = append(cand.Diagnostics, fmt.Sprintf("[%s] %s: %s", issue.Severity, issue.Code, issue.Message))
		if issue.Severity == "error" {
			cand.Valid = false
			if cand.Status == "candidate" {
				cand.Status = "invalid"
			}
		}
	}

	return cand
}

func isManagedDir(path string) bool {
	markerPath := filepath.Join(path, ".symskills.json")
	_, err := os.Stat(markerPath)
	return err == nil
}

func computeSourceID(path string) string {
	skillMD := filepath.Join(path, "SKILL.md")
	content, _ := os.ReadFile(skillMD)
	h := sha256.New()
	h.Write([]byte(path))
	h.Write([]byte{0})
	h.Write(content)
	return "sha256:" + hex.EncodeToString(h.Sum(nil))[:16]
}
