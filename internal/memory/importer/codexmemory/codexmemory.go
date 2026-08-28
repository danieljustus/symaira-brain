// Package codexmemory imports the curated Markdown memory tier written by
// Codex and its activity-summary extensions. It deliberately does not read
// Codex rollout JSONL or Skysight's raw event store.
package codexmemory

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
	"github.com/danieljustus/symaira-corekit/evidencekit"
)

const (
	name             = "codex-memory"
	activityCategory = "activity"
	granularity10m   = "10min"
	granularity6h    = "6h"
	maxMarkdownBytes = 1 << 20
)

// ApplicationPolicy controls which bundle IDs may be imported. A non-empty
// allow-list requires at least one matching application; any deny-list match
// rejects the document. Deny always wins.
type ApplicationPolicy struct {
	Allowed []string
	Denied  []string
}

// Options configures a Codex memory importer.
type Options struct {
	// Root is the Codex memory root, normally ~/.codex/memories.
	Root string
	ApplicationPolicy
}

// CodexMemoryImporter imports Codex's Markdown memory tier read-only.
type CodexMemoryImporter struct {
	root  string
	allow map[string]struct{}
	deny  map[string]struct{}

	mu         sync.Mutex
	lastImport time.Time
}

// resourceFilename recognizes the observed Skysight resource naming scheme.
var resourceFilename = regexp.MustCompile(`^(\d{4}-\d{2}-\d{2}T\d{2}-\d{2}-\d{2})-[^-]+-(10min|6h)-(.+)\.md$`)

// NewCodexMemoryImporter creates an importer rooted at root. An empty root
// resolves to ~/.codex/memories. The optional policy keeps the common,
// path-only constructor compatible with other importers.
func NewCodexMemoryImporter(root string, policies ...ApplicationPolicy) *CodexMemoryImporter {
	var policy ApplicationPolicy
	if len(policies) > 0 {
		policy = policies[0]
	}
	return newImporter(Options{Root: root, ApplicationPolicy: policy})
}

// NewWithOptions creates an importer with an application allow/deny policy.
func NewWithOptions(options Options) *CodexMemoryImporter {
	return newImporter(options)
}

// Register adds the Codex Markdown importer to an existing registry. Keeping
// registration here avoids a package cycle while making the integration
// explicit for applications that construct importer registries themselves.
func Register(registry *importer.Registry, options ...Options) {
	if registry == nil {
		return
	}
	var optionsValue Options
	if len(options) > 0 {
		optionsValue = options[0]
	}
	registry.Register(NewWithOptions(optionsValue))
}

func newImporter(options Options) *CodexMemoryImporter {
	return &CodexMemoryImporter{
		root:  options.Root,
		allow: stringSet(options.Allowed),
		deny:  stringSet(options.Denied),
	}
}

func (c *CodexMemoryImporter) Name() string     { return name }
func (c *CodexMemoryImporter) Category() string { return activityCategory }
func (c *CodexMemoryImporter) PrivacyLevel() importer.PrivacyLevel {
	return importer.PrivacyConfidential
}
func (c *CodexMemoryImporter) RequiresPIIGuard() bool { return true }
func (c *CodexMemoryImporter) IsTranscript() bool     { return true }

// StageImportedFacts marks this source as autonomous, observational material
// that must enter the existing staged-review queue instead of going live.
func (c *CodexMemoryImporter) StageImportedFacts() bool { return true }

// ContentIsUntrusted tells the registry to apply the existing untrusted-data
// sanitizer before extraction, storage, or any future LLM handoff.
func (c *CodexMemoryImporter) ContentIsUntrusted() bool { return true }

// LastImportTime and MarkImported implement the optional incremental contract.
// The registry's durable import_state remains authoritative; this in-memory
// cursor also makes direct importer use incremental without writing into the
// Codex tree.
func (c *CodexMemoryImporter) LastImportTime() (time.Time, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.lastImport, nil
}

func (c *CodexMemoryImporter) MarkImported(ref importer.SessionRef) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if ref.ModifiedAt.After(c.lastImport) {
		c.lastImport = ref.ModifiedAt
	}
	return nil
}

// DiscoverSessions finds consolidated Codex memory files and extension
// resources modified at or after since. Missing roots are a silent no-op.
func (c *CodexMemoryImporter) DiscoverSessions(since time.Time) ([]importer.SessionRef, error) {
	root, err := c.rootDir()
	if err != nil {
		return nil, err
	}
	if _, err := os.Stat(root); os.IsNotExist(err) {
		return nil, nil
	} else if err != nil {
		return nil, err
	}

	paths, err := c.discoverPaths(root)
	if err != nil {
		return nil, err
	}

	// Read all eligible 6h resource names, even when their mtime predates the
	// cursor, so a recent 10min file is not reintroduced alongside its parent.
	var sixHourStarts []time.Time
	for _, path := range paths {
		if granularity, start, ok := parseResourceName(filepath.Base(path)); ok && granularity == granularity6h {
			if c.applicationAllowed(path) {
				sixHourStarts = append(sixHourStarts, start)
			}
		}
	}

	sessions := make([]importer.SessionRef, 0, len(paths))
	for _, path := range paths {
		info, err := os.Stat(path)
		if err != nil || !info.Mode().IsRegular() || info.ModTime().Before(since) {
			continue
		}
		if !c.applicationAllowed(path) {
			continue
		}

		metadata := map[string]string{
			"source_kind":      "codex_markdown",
			"file_path":        path,
			"sync_exclude":     "true",
			"promotion_policy": "recurring_only;prefer_6h;single_10min_not_promotable",
		}
		if granularity, start, ok := parseResourceName(filepath.Base(path)); ok {
			metadata["extension"] = extensionName(root, path)
			metadata["granularity"] = granularity
			metadata["activity_started_at"] = start.UTC().Format(time.RFC3339)
			metadata["activity_ends_at"] = start.Add(activityDuration(granularity)).UTC().Format(time.RFC3339)
			if granularity == granularity10m {
				metadata["promotable"] = "false"
			} else {
				metadata["promotable"] = "candidate"
			}
			// A 6h summary is the preferred representation for every 10min
			// window it covers. Do not emit duplicate candidates.
			if granularity == granularity10m && coveredBySixHour(start, sixHourStarts) {
				continue
			}
		} else {
			metadata["granularity"] = "consolidated"
		}

		if sessionID, ok := sessionID(root, path); ok {
			sessions = append(sessions, importer.SessionRef{
				Tool:       name,
				SessionID:  sessionID,
				Path:       path,
				ModifiedAt: info.ModTime(),
				Metadata:   metadata,
			})
		}
	}
	return sessions, nil
}

// ImportSession parses one Markdown document. Frontmatter is metadata only;
// the body is the evidence and the Citations section is retained separately
// as onward provenance.
func (c *CodexMemoryImporter) ImportSession(ref importer.SessionRef) ([]importer.ImportedFact, error) {
	if strings.EqualFold(filepath.Base(ref.Path), "instructions.md") {
		return nil, nil
	}
	root, err := c.rootDir()
	if err != nil {
		return nil, err
	}
	rel, err := filepath.Rel(root, ref.Path)
	if err != nil || rel == ".." || strings.HasPrefix(rel, ".."+string(os.PathSeparator)) {
		return nil, nil
	}
	if !c.applicationAllowed(ref.Path) {
		return nil, nil
	}
	data, err := readMarkdownFile(ref.Path)
	if err != nil {
		return nil, fmt.Errorf("read codex memory %q: %w", ref.Path, err)
	}
	frontmatter, body := parseMarkdown(string(data))
	body = strings.TrimSpace(body)
	if body == "" {
		return nil, nil
	}

	info, err := os.Stat(ref.Path)
	if err != nil {
		return nil, fmt.Errorf("stat codex memory %q: %w", ref.Path, err)
	}
	timestamp := info.ModTime()
	if !ref.ModifiedAt.IsZero() {
		timestamp = ref.ModifiedAt
	}

	metadata := cloneMap(ref.Metadata)
	metadata["title"] = frontmatter["title"]
	metadata["description"] = frontmatter["description"]
	if len(frontmatter) > 0 {
		if encoded, err := json.Marshal(frontmatter); err == nil {
			metadata["frontmatter"] = string(encoded)
		}
	}
	if apps := parseApplications(frontmatter["applications"]); len(apps) > 0 {
		metadata["applications"] = strings.Join(apps, ",")
	}
	metadata["source_path"] = ref.Path
	if citations := sectionBody(body, "Citations"); citations != "" {
		metadata["citations"] = citations
	}

	if granularity, start, ok := parseResourceName(filepath.Base(ref.Path)); ok {
		metadata["granularity"] = granularity
		metadata["activity_started_at"] = start.UTC().Format(time.RFC3339)
		metadata["activity_ends_at"] = start.Add(activityDuration(granularity)).UTC().Format(time.RFC3339)
		if granularity == granularity10m {
			metadata["promotable"] = "false"
		} else {
			metadata["promotable"] = "candidate"
		}
	}
	if metadata["sync_exclude"] == "" {
		metadata["sync_exclude"] = "true"
	}
	if metadata["promotion_policy"] == "" {
		metadata["promotion_policy"] = "recurring_only;prefer_6h;single_10min_not_promotable"
	}

	return []importer.ImportedFact{{
		Content:   body,
		Source:    name,
		SessionID: ref.SessionID,
		Timestamp: timestamp,
		Metadata:  metadata,
		Evidence: []evidencekit.Extraction{{
			Source:          evidencekit.SourceRef{ID: ref.SessionID, Kind: name},
			Text:            body,
			EvidenceText:    body,
			Span:            evidencekit.Span{Start: 0, End: len(body)},
			AlignmentStatus: evidencekit.AlignmentExact,
		}},
	}}, nil
}

func (c *CodexMemoryImporter) rootDir() (string, error) {
	if c.root != "" {
		return filepath.Clean(c.root), nil
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(home, ".codex", "memories"), nil
}

func (c *CodexMemoryImporter) discoverPaths(root string) ([]string, error) {
	var paths []string
	rootFiles := map[string]bool{"MEMORY.md": true, "memory_summary.md": true, "raw_memories.md": true}
	for file := range rootFiles {
		path := filepath.Join(root, file)
		if info, err := os.Stat(path); err == nil && info.Mode().IsRegular() {
			paths = append(paths, path)
		}
	}

	walkErr := filepath.Walk(root, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			if os.IsNotExist(err) {
				return nil
			}
			return nil
		}
		if info.IsDir() {
			return nil
		}
		rel, err := filepath.Rel(root, path)
		if err != nil {
			return nil
		}
		parts := strings.Split(filepath.ToSlash(rel), "/")
		if len(parts) == 2 && parts[0] == "rollout_summaries" && strings.EqualFold(filepath.Ext(path), ".md") {
			paths = append(paths, path)
		}
		if len(parts) >= 4 && parts[0] == "extensions" && parts[2] == "resources" && strings.EqualFold(filepath.Ext(path), ".md") {
			paths = append(paths, path)
		}
		return nil
	})
	if walkErr != nil {
		return nil, walkErr
	}
	sort.Strings(paths)
	return deduplicate(paths), nil
}

func (c *CodexMemoryImporter) applicationAllowed(path string) bool {
	if len(c.allow) == 0 && len(c.deny) == 0 {
		return true
	}
	data, err := readMarkdownFile(path)
	if err != nil {
		return false
	}
	fm, _ := parseMarkdown(string(data))
	apps := parseApplications(fm["applications"])
	for _, app := range apps {
		if _, denied := c.deny[app]; denied {
			return false
		}
	}
	if len(c.allow) == 0 {
		return true
	}
	for _, app := range apps {
		if _, allowed := c.allow[app]; allowed {
			return true
		}
	}
	return false
}

func sessionID(root, path string) (string, bool) {
	rel, err := filepath.Rel(root, path)
	if err != nil || rel == "." || strings.HasPrefix(rel, ".."+string(os.PathSeparator)) {
		return "", false
	}
	return name + ":" + filepath.ToSlash(rel), true
}

func extensionName(root, path string) string {
	rel, err := filepath.Rel(root, path)
	if err != nil {
		return ""
	}
	parts := strings.Split(filepath.ToSlash(rel), "/")
	if len(parts) >= 2 && parts[0] == "extensions" {
		return parts[1]
	}
	return ""
}

func parseResourceName(base string) (string, time.Time, bool) {
	match := resourceFilename.FindStringSubmatch(base)
	if match == nil {
		return "", time.Time{}, false
	}
	start, err := time.Parse("2006-01-02T15-04-05", match[1])
	if err != nil {
		return "", time.Time{}, false
	}
	return match[2], start, true
}

func activityDuration(granularity string) time.Duration {
	if granularity == granularity6h {
		return 6 * time.Hour
	}
	return 10 * time.Minute
}

func coveredBySixHour(start time.Time, sixHourStarts []time.Time) bool {
	for _, parent := range sixHourStarts {
		if !start.Before(parent) && start.Before(parent.Add(6*time.Hour)) {
			return true
		}
	}
	return false
}

func readMarkdownFile(path string) ([]byte, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer func() { _ = file.Close() }()
	data, err := io.ReadAll(io.LimitReader(file, maxMarkdownBytes+1))
	if err != nil {
		return nil, err
	}
	if len(data) > maxMarkdownBytes {
		data = data[:maxMarkdownBytes]
	}
	return data, nil
}

func parseMarkdown(content string) (map[string]string, string) {
	fm := make(map[string]string)
	content = strings.ReplaceAll(content, "\r\n", "\n")
	if !strings.HasPrefix(content, "---\n") {
		return fm, content
	}
	end := strings.Index(content[4:], "\n---")
	if end < 0 {
		return fm, content
	}
	end += 4
	var currentKey string
	for _, line := range strings.Split(content[4:end], "\n") {
		trimmed := strings.TrimSpace(line)
		if trimmed == "" || strings.HasPrefix(trimmed, "#") {
			continue
		}
		if strings.HasPrefix(trimmed, "-") && currentKey == "applications" {
			value := strings.TrimSpace(strings.TrimPrefix(trimmed, "-"))
			if value != "" {
				if fm[currentKey] != "" {
					fm[currentKey] += ","
				}
				fm[currentKey] += value
			}
			continue
		}
		parts := strings.SplitN(trimmed, ":", 2)
		if len(parts) != 2 {
			currentKey = ""
			continue
		}
		key := strings.TrimSpace(parts[0])
		if key == "" {
			currentKey = ""
			continue
		}
		currentKey = key
		fm[key] = strings.TrimSpace(strings.Trim(strings.TrimSpace(parts[1]), "\"'"))
	}
	return fm, strings.TrimPrefix(content[end+4:], "\n")
}

func parseApplications(raw string) []string {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		return nil
	}
	if strings.HasPrefix(raw, "[") && strings.HasSuffix(raw, "]") {
		var values []string
		if json.Unmarshal([]byte(raw), &values) == nil {
			return cleanApplications(values)
		}
		raw = strings.TrimSpace(raw[1 : len(raw)-1])
	}
	return cleanApplications(strings.Split(raw, ","))
}

func cleanApplications(values []string) []string {
	seen := make(map[string]struct{}, len(values))
	out := make([]string, 0, len(values))
	for _, value := range values {
		value = strings.Trim(strings.TrimSpace(value), "\"'")
		if value != "" {
			if _, ok := seen[value]; !ok {
				seen[value] = struct{}{}
				out = append(out, value)
			}
		}
	}
	sort.Strings(out)
	return out
}

func sectionBody(body, heading string) string {
	lines := strings.Split(body, "\n")
	start := -1
	for i, line := range lines {
		if strings.EqualFold(strings.TrimSpace(line), "## "+heading) {
			start = i + 1
			break
		}
	}
	if start < 0 {
		return ""
	}
	end := len(lines)
	for i := start; i < len(lines); i++ {
		if strings.HasPrefix(strings.TrimSpace(lines[i]), "## ") {
			end = i
			break
		}
	}
	return strings.TrimSpace(strings.Join(lines[start:end], "\n"))
}

func cloneMap(src map[string]string) map[string]string {
	dst := make(map[string]string, len(src)+8)
	for key, value := range src {
		dst[key] = value
	}
	return dst
}

func stringSet(values []string) map[string]struct{} {
	set := make(map[string]struct{}, len(values))
	for _, value := range values {
		value = strings.TrimSpace(value)
		if value != "" {
			set[value] = struct{}{}
		}
	}
	return set
}

func deduplicate(paths []string) []string {
	out := paths[:0]
	for _, path := range paths {
		if len(out) == 0 || out[len(out)-1] != path {
			out = append(out, path)
		}
	}
	return out
}
