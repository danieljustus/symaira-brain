package obsidian

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
)

// ObsidianImporter imports notes from an Obsidian vault.
type ObsidianImporter struct {
	vaultPath      string
	folder         string   // optional: only import from specific folder
	tags           []string // optional: only import notes with these tags
	excludeFolders []string
	excludeTags    []string
	maxNoteLength  int // max chars per note
}

var (
	// Matches wikilinks [[...]]
	wikilinkRe = regexp.MustCompile(`\[\[([^\]|]+)(?:\|[^\]]+)?\]\]`)
	// Matches tags #tag (including hyphens)
	tagRe = regexp.MustCompile(`#([a-zA-Z0-9_/\-]+)`)
)

// ObsidianNote represents a parsed Obsidian note.
type ObsidianNote struct {
	Path        string
	Title       string
	Content     string
	Tags        []string
	Frontmatter map[string]interface{}
	Wikilinks   []string
	ModifiedAt  time.Time
	WordCount   int
}

// NewObsidianImporter creates a new Obsidian importer.
func NewObsidianImporter(vaultPath, folder string, tags, excludeFolders, excludeTags []string) *ObsidianImporter {
	if vaultPath == "" {
		vaultPath = detectVaultPath()
	}

	return &ObsidianImporter{
		vaultPath:      vaultPath,
		folder:         folder,
		tags:           tags,
		excludeFolders: excludeFolders,
		excludeTags:    excludeTags,
		maxNoteLength:  5000,
	}
}

func (o *ObsidianImporter) Name() string { return "obsidian" }

func (o *ObsidianImporter) Category() string { return "notes" }

func (o *ObsidianImporter) PrivacyLevel() importer.PrivacyLevel {
	return importer.PrivacyInternal
}

func (o *ObsidianImporter) RequiresPIIGuard() bool { return false }

// DiscoverSessions finds modified notes since the given time.
func (o *ObsidianImporter) DiscoverSessions(since time.Time) ([]importer.SessionRef, error) {
	if o.vaultPath == "" {
		return nil, fmt.Errorf("no vault path configured")
	}

	var sessions []importer.SessionRef

	err := filepath.Walk(o.vaultPath, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return nil // skip errors
		}

		// Skip directories
		if info.IsDir() {
			return nil
		}

		// Only process .md files
		if filepath.Ext(path) != ".md" {
			return nil
		}

		// Skip excluded folders
		relPath, _ := filepath.Rel(o.vaultPath, path)
		if o.isExcludedFolder(relPath) {
			return nil
		}

		// Check modification time
		if info.ModTime().Before(since) {
			return nil
		}

		// Parse note to check tags
		note, err := o.parseNote(path)
		if err != nil {
			return nil // skip unparseable notes
		}

		// Filter by tags if configured
		if len(o.tags) > 0 && !o.hasMatchingTag(note.Tags) {
			return nil
		}

		// Skip excluded tags
		if o.hasExcludedTag(note.Tags) {
			return nil
		}

		session := importer.SessionRef{
			Tool:       "obsidian",
			SessionID:  relPath,
			Path:       path,
			ModifiedAt: info.ModTime(),
			Metadata: map[string]string{
				"title":      note.Title,
				"vault":      filepath.Base(o.vaultPath),
				"word_count": fmt.Sprintf("%d", note.WordCount),
			},
		}

		if len(note.Tags) > 0 {
			jsonTags, _ := json.Marshal(note.Tags)
			session.Metadata["tags"] = string(jsonTags)
		}

		sessions = append(sessions, session)
		return nil
	})

	return sessions, err
}

// ImportSession imports a single note as facts.
func (o *ObsidianImporter) ImportSession(ref importer.SessionRef) ([]importer.ImportedFact, error) {
	note, err := o.parseNote(ref.Path)
	if err != nil {
		return nil, fmt.Errorf("failed to parse note: %w", err)
	}

	// Truncate content
	content := note.Content
	if len(content) > o.maxNoteLength {
		content = content[:o.maxNoteLength] + "..."
	}

	factContent := fmt.Sprintf("Note: %s\n\n%s", note.Title, content)

	metadata := map[string]string{
		"source":     "obsidian",
		"vault":      filepath.Base(o.vaultPath),
		"note_path":  ref.SessionID,
		"title":      note.Title,
		"word_count": fmt.Sprintf("%d", note.WordCount),
		"modified":   note.ModifiedAt.Format(time.RFC3339),
	}

	if len(note.Tags) > 0 {
		jsonTags, _ := json.Marshal(note.Tags)
		metadata["tags"] = string(jsonTags)
	}

	if len(note.Wikilinks) > 0 {
		jsonLinks, _ := json.Marshal(note.Wikilinks)
		metadata["wikilinks"] = string(jsonLinks)
	}

	if len(note.Frontmatter) > 0 {
		jsonFM, _ := json.Marshal(note.Frontmatter)
		metadata["frontmatter"] = string(jsonFM)
	}

	return []importer.ImportedFact{{
		Content:   factContent,
		Source:    "obsidian",
		SessionID: ref.SessionID,
		Timestamp: note.ModifiedAt,
		Metadata:  metadata,
	}}, nil
}

// parseNote parses an Obsidian note file.
func (o *ObsidianImporter) parseNote(path string) (*ObsidianNote, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer func() { _ = file.Close() }()

	note := &ObsidianNote{
		Path:        path,
		Frontmatter: make(map[string]interface{}),
	}

	var contentBuilder strings.Builder
	scanner := bufio.NewScanner(file)

	inFrontmatter := false
	frontmatterLines := []string{}
	lineNum := 0

	for scanner.Scan() {
		line := scanner.Text()
		lineNum++

		// Handle frontmatter
		if lineNum == 1 && line == "---" {
			inFrontmatter = true
			continue
		}

		if inFrontmatter {
			if line == "---" {
				inFrontmatter = false
				// Parse YAML frontmatter (simple key: value)
				o.parseSimpleYAML(frontmatterLines, note)
				continue
			}
			frontmatterLines = append(frontmatterLines, line)
			continue
		}

		contentBuilder.WriteString(line)
		contentBuilder.WriteString("\n")
	}

	note.Content = strings.TrimSpace(contentBuilder.String())

	// Extract title from first H1 or filename
	note.Title = o.extractTitle(note.Content, path)

	// Extract tags from content
	note.Tags = o.extractTags(note.Content)

	// Extract wikilinks
	note.Wikilinks = o.extractWikilinks(note.Content)

	// Word count
	note.WordCount = len(strings.Fields(note.Content))

	// Modified time
	info, err := os.Stat(path)
	if err == nil {
		note.ModifiedAt = info.ModTime()
	}

	return note, scanner.Err()
}

// extractTitle extracts the note title from content or filename.
func (o *ObsidianImporter) extractTitle(content, path string) string {
	// Try to find first H1
	lines := strings.Split(content, "\n")
	for _, line := range lines {
		line = strings.TrimSpace(line)
		if strings.HasPrefix(line, "# ") {
			return strings.TrimPrefix(line, "# ")
		}
	}

	// Fall back to filename without extension
	basename := filepath.Base(path)
	return strings.TrimSuffix(basename, ".md")
}

// extractTags extracts tags from content.
func (o *ObsidianImporter) extractTags(content string) []string {
	matches := tagRe.FindAllStringSubmatch(content, -1)
	var tags []string
	seen := make(map[string]bool)

	for _, match := range matches {
		tag := match[1]
		if !seen[tag] {
			seen[tag] = true
			tags = append(tags, tag)
		}
	}

	return tags
}

// extractWikilinks extracts wikilinks from content.
func (o *ObsidianImporter) extractWikilinks(content string) []string {
	matches := wikilinkRe.FindAllStringSubmatch(content, -1)
	var links []string
	seen := make(map[string]bool)

	for _, match := range matches {
		link := match[1]
		if !seen[link] {
			seen[link] = true
			links = append(links, link)
		}
	}

	return links
}

// parseSimpleYAML parses simple YAML key: value pairs.
func (o *ObsidianImporter) parseSimpleYAML(lines []string, note *ObsidianNote) {
	for _, line := range lines {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}

		parts := strings.SplitN(line, ":", 2)
		if len(parts) == 2 {
			key := strings.TrimSpace(parts[0])
			value := strings.TrimSpace(parts[1])
			// Remove quotes
			value = strings.Trim(value, "\"'")
			note.Frontmatter[key] = value
		}
	}
}

// isExcludedFolder checks if a path should be excluded.
func (o *ObsidianImporter) isExcludedFolder(relPath string) bool {
	defaultExcludes := []string{".obsidian", ".trash", "Templates", "node_modules"}
	excludes := append(defaultExcludes, o.excludeFolders...)

	for _, exclude := range excludes {
		if strings.HasPrefix(relPath, exclude+"/") || strings.Contains(relPath, "/"+exclude+"/") || relPath == exclude {
			return true
		}
	}

	return false
}

// hasMatchingTag checks if any note tags match the filter.
func (o *ObsidianImporter) hasMatchingTag(noteTags []string) bool {
	for _, noteTag := range noteTags {
		for _, filterTag := range o.tags {
			if noteTag == filterTag {
				return true
			}
		}
	}
	return false
}

// hasExcludedTag checks if any note tags are excluded.
func (o *ObsidianImporter) hasExcludedTag(noteTags []string) bool {
	for _, noteTag := range noteTags {
		for _, excludeTag := range o.excludeTags {
			if noteTag == excludeTag {
				return true
			}
		}
	}
	return false
}

// detectVaultPath attempts to auto-detect the Obsidian vault path.
func detectVaultPath() string {
	// Check env
	if envPath := os.Getenv("OBSIDIAN_VAULT"); envPath != "" {
		if _, err := os.Stat(envPath); err == nil {
			return envPath
		}
	}

	// Default LifeOS path
	home, err := os.UserHomeDir()
	if err != nil {
		return ""
	}

	lifeOSPath := filepath.Join(home, "Library", "Mobile Documents", "iCloud~md~obsidian", "Documents", "LifeOS")
	if _, err := os.Stat(lifeOSPath); err == nil {
		return lifeOSPath
	}

	return ""
}
