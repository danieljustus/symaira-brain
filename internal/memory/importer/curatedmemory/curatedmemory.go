package curatedmemory

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

// CuratedMemoryImporter imports curated memory files from other AI agents
// (Claude Code memory directories and Hermes curated memories).
type CuratedMemoryImporter struct {
	homeDir string // override for testing
}

// CuratedMemoryFile represents a parsed curated memory file.
type CuratedMemoryFile struct {
	Path        string
	Source      string // "claude-code" or "hermes"
	Project     string // project name (claude-code only)
	Content     string
	Frontmatter map[string]interface{}
	Links       []string
	ModifiedAt  time.Time
	WordCount   int
}

// Matches [[links]] in content.
var linkRe = regexp.MustCompile(`\[\[([^\]|]+)(?:\|[^\]]+)?\]\]`)

// NewCuratedMemoryImporter creates a new CuratedMemoryImporter.
// If homeDir is empty, the user's home directory is used.
func NewCuratedMemoryImporter(homeDir string) *CuratedMemoryImporter {
	return &CuratedMemoryImporter{homeDir: homeDir}
}

func (c *CuratedMemoryImporter) Name() string     { return "curated-memory" }
func (c *CuratedMemoryImporter) Category() string { return "notes" }
func (c *CuratedMemoryImporter) PrivacyLevel() importer.PrivacyLevel {
	return importer.PrivacyConfidential
}
func (c *CuratedMemoryImporter) RequiresPIIGuard() bool { return true }

// DiscoverSessions finds curated memory files modified since the given time.
// It scans two source profiles:
//   - Claude Code: ~/.claude/projects/**/memory/*.md (skips MEMORY.md index files)
//   - Hermes: ~/.hermes/memories/MEMORY.md and ~/.hermes/memories/USER.md
func (c *CuratedMemoryImporter) DiscoverSessions(since time.Time) ([]importer.SessionRef, error) {
	home, err := c.getHomeDir()
	if err != nil {
		return nil, err
	}

	var sessions []importer.SessionRef

	// Discover Claude Code memory files: ~/.claude/projects/**/memory/*.md
	claudeBase := filepath.Join(home, ".claude", "projects")
	claudeSessions, err := c.discoverClaudeCode(claudeBase, since)
	if err != nil {
		// Non-fatal: directory may not exist
		fmt.Fprintf(os.Stderr, "curated-memory: skipping claude-code discovery: %v\n", err)
	} else {
		sessions = append(sessions, claudeSessions...)
	}

	// Discover Hermes curated memories: ~/.hermes/memories/*.md
	hermesBase := filepath.Join(home, ".hermes", "memories")
	hermesSessions, err := c.discoverHermes(hermesBase, since)
	if err != nil {
		fmt.Fprintf(os.Stderr, "curated-memory: skipping hermes discovery: %v\n", err)
	} else {
		sessions = append(sessions, hermesSessions...)
	}

	return sessions, nil
}

// ImportSession imports a single curated memory file as facts.
func (c *CuratedMemoryImporter) ImportSession(ref importer.SessionRef) ([]importer.ImportedFact, error) {
	file, err := os.Open(ref.Path)
	if err != nil {
		return nil, fmt.Errorf("failed to read memory file: %w", err)
	}
	defer func() { _ = file.Close() }()

	memFile, err := c.parseMemoryFile(file, ref.Path, ref.Metadata)
	if err != nil {
		return nil, fmt.Errorf("failed to parse memory file: %w", err)
	}

	if memFile.Content == "" {
		return nil, nil // skip empty files
	}

	content := memFile.Content
	if len(content) > 5000 {
		content = content[:5000] + "..."
	}

	metadata := map[string]string{
		"source":     ref.Tool,
		"file_path":  ref.SessionID,
		"word_count": fmt.Sprintf("%d", memFile.WordCount),
		"modified":   memFile.ModifiedAt.Format(time.RFC3339),
	}

	if memFile.Project != "" {
		metadata["project"] = memFile.Project
	}

	if len(memFile.Frontmatter) > 0 {
		if name, ok := memFile.Frontmatter["name"]; ok {
			if s, ok := name.(string); ok {
				metadata["title"] = s
			}
		}
		if desc, ok := memFile.Frontmatter["description"]; ok {
			if s, ok := desc.(string); ok {
				metadata["description"] = s
			}
		}
		if typ, ok := memFile.Frontmatter["type"]; ok {
			if s, ok := typ.(string); ok {
				metadata["memory_type"] = s
			}
		}
		jsonFM, _ := json.Marshal(memFile.Frontmatter)
		metadata["frontmatter"] = string(jsonFM)
	}

	if len(memFile.Links) > 0 {
		jsonLinks, _ := json.Marshal(memFile.Links)
		metadata["links"] = string(jsonLinks)
	}

	return []importer.ImportedFact{{
		Content:   content,
		Source:    ref.Tool,
		SessionID: ref.SessionID,
		Timestamp: memFile.ModifiedAt,
		Metadata:  metadata,
	}}, nil
}

// discoverClaudeCode walks ~/.claude/projects/**/memory/*.md,
// skipping any MEMORY.md index files at the project root.
func (c *CuratedMemoryImporter) discoverClaudeCode(basePath string, since time.Time) ([]importer.SessionRef, error) {
	var sessions []importer.SessionRef

	err := filepath.Walk(basePath, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return nil // skip errors
		}
		if info.IsDir() {
			return nil
		}
		if filepath.Ext(path) != ".md" {
			return nil
		}

		// Skip MEMORY.md index files at project root level
		// (these are at ~/.claude/projects/<project>/MEMORY.md)
		if info.Name() == "MEMORY.md" {
			parent := filepath.Dir(path)
			// If parent is "memory" dir, this is inside the memory dir — skip
			if filepath.Base(parent) == "memory" {
				return nil
			}
			// If parent contains a "memory" subdir, this is the index — skip
			if _, statErr := os.Stat(filepath.Join(parent, "memory")); statErr == nil {
				return nil
			}
		}

		if info.ModTime().Before(since) {
			return nil
		}

		// Extract project name from path: ~/.claude/projects/<project>/memory/*.md
		relPath, _ := filepath.Rel(basePath, path)
		parts := strings.SplitN(relPath, string(os.PathSeparator), 2)
		project := ""
		if len(parts) > 0 {
			project = parts[0]
		}

		sessionID := "claude-code:" + relPath

		sessions = append(sessions, importer.SessionRef{
			Tool:       "curated-memory",
			SessionID:  sessionID,
			Path:       path,
			ModifiedAt: info.ModTime(),
			Metadata: map[string]string{
				"source_tool": "claude-code",
				"project":     project,
			},
		})
		return nil
	})

	return sessions, err
}

// discoverHermes scans ~/.hermes/memories/ for MEMORY.md and USER.md.
func (c *CuratedMemoryImporter) discoverHermes(basePath string, since time.Time) ([]importer.SessionRef, error) {
	var sessions []importer.SessionRef

	// Hermes uses specific files, not a directory walk
	targetFiles := []string{"MEMORY.md", "USER.md"}

	for _, name := range targetFiles {
		path := filepath.Join(basePath, name)
		info, err := os.Stat(path)
		if err != nil {
			continue // file doesn't exist, skip
		}
		if info.ModTime().Before(since) {
			continue
		}

		sessionID := "hermes:" + name

		sessions = append(sessions, importer.SessionRef{
			Tool:       "curated-memory",
			SessionID:  sessionID,
			Path:       path,
			ModifiedAt: info.ModTime(),
			Metadata: map[string]string{
				"source_tool": "hermes",
			},
		})
	}

	return sessions, nil
}

// parseMemoryFile reads and parses a curated memory markdown file.
func (c *CuratedMemoryImporter) parseMemoryFile(file *os.File, path string, refMeta map[string]string) (*CuratedMemoryFile, error) {
	memFile := &CuratedMemoryFile{
		Path:        path,
		Frontmatter: make(map[string]interface{}),
	}

	if refMeta != nil {
		if st, ok := refMeta["source_tool"]; ok {
			memFile.Source = st
		}
		if p, ok := refMeta["project"]; ok {
			memFile.Project = p
		}
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
				c.parseSimpleYAML(frontmatterLines, memFile)
				continue
			}
			frontmatterLines = append(frontmatterLines, line)
			continue
		}

		contentBuilder.WriteString(line)
		contentBuilder.WriteString("\n")
	}

	memFile.Content = strings.TrimSpace(contentBuilder.String())
	memFile.WordCount = len(strings.Fields(memFile.Content))
	memFile.Links = c.extractLinks(memFile.Content)

	// File modification time
	info, err := os.Stat(path)
	if err == nil {
		memFile.ModifiedAt = info.ModTime()
	}

	return memFile, scanner.Err()
}

// parseSimpleYAML parses simple YAML key: value pairs (same approach as obsidian importer).
func (c *CuratedMemoryImporter) parseSimpleYAML(lines []string, memFile *CuratedMemoryFile) {
	for _, line := range lines {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}

		parts := strings.SplitN(line, ":", 2)
		if len(parts) == 2 {
			key := strings.TrimSpace(parts[0])
			value := strings.TrimSpace(parts[1])
			// Remove surrounding quotes
			value = strings.Trim(value, "\"'")
			memFile.Frontmatter[key] = value
		}
	}
}

// extractLinks extracts [[link]] references from content.
func (c *CuratedMemoryImporter) extractLinks(content string) []string {
	matches := linkRe.FindAllStringSubmatch(content, -1)
	var links []string
	seen := make(map[string]bool)

	for _, match := range matches {
		link := strings.TrimSpace(match[1])
		if !seen[link] {
			seen[link] = true
			links = append(links, link)
		}
	}

	return links
}

// getHomeDir returns the home directory, using the override if set.
func (c *CuratedMemoryImporter) getHomeDir() (string, error) {
	if c.homeDir != "" {
		return c.homeDir, nil
	}
	return os.UserHomeDir()
}
