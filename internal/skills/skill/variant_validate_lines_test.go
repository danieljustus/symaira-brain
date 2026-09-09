package skill

import (
	"fmt"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/variant"
)

func TestValidateReportsFileRelativeLineForMarkers(t *testing.T) {
	root := filepath.Join(t.TempDir(), "marked")
	content := `---
name: marked
description: A skill with an unclosed region below the frontmatter.
category: developer-tools
---

<!-- symskills:block worker -->
Never closed.
`
	writeFile(t, filepath.Join(root, "SKILL.md"), content)

	wantLine := 0
	for i, line := range strings.Split(content, "\n") {
		if strings.Contains(line, "symskills:block") {
			wantLine = i + 1
			break
		}
	}
	issue, ok := issueFor(Validate(loadForValidation(t, root)), variant.CodeBlockUnclosed)
	if !ok {
		t.Fatal("expected an unclosed-block error")
	}
	if !strings.Contains(issue.Message, fmt.Sprintf("line %d:", wantLine)) {
		t.Errorf("expected line %d in %q", wantLine, issue.Message)
	}
}
