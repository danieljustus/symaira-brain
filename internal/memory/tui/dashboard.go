package tui

import (
	"bytes"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"strings"
	"time"

	"github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

// noStyle returns a styled lipgloss value when colors are enabled, or an empty
// string when no-color mode is active.
type noColorFlag struct{ enabled bool }

func (n noColorFlag) styled(style lipgloss.Style, text string) string {
	if n.enabled {
		return text
	}
	return style.Render(text)
}

func (n noColorFlag) scopeBadge(scope string) string {
	if n.enabled {
		return " " + strings.ToUpper(scope) + " "
	}
	color := "#89B4FA"
	switch scope {
	case "global":
		color = "#A6E3A1"
	case "project":
		color = "#F9E2AF"
	case "agent":
		color = "#CBA6F7"
	case "user":
		color = "#F38BA8"
	case "session":
		color = "#94E2D5"
	}
	return lipgloss.NewStyle().
		Bold(true).
		Foreground(lipgloss.Color("#1E1E2E")).
		Background(lipgloss.Color(color)).
		Padding(0, 1).
		Render(" " + strings.ToUpper(scope) + " ")
}

// Styling tokens (only used when colors are enabled)
var (
	titleStyle = lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("#A2EEEF")).
			Background(lipgloss.Color("#1E1E2E")).
			Padding(1, 2).
			MarginBottom(1)

	selectedStyle = lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("#A2EEEF")).
			PaddingLeft(2)

	normalStyle = lipgloss.NewStyle().
			Foreground(lipgloss.Color("#CDD6F4")).
			PaddingLeft(2)

	metaStyle = lipgloss.NewStyle().
			Foreground(lipgloss.Color("#7F849C")).
			Italic(true)

	statsStyle = lipgloss.NewStyle().
			Border(lipgloss.RoundedBorder()).
			BorderForeground(lipgloss.Color("#45475A")).
			Padding(1, 2).
			Width(40)

	footerStyle = lipgloss.NewStyle().
			Foreground(lipgloss.Color("#585B70")).
			MarginTop(1)
)

type model struct {
	db         *db.DB
	memories   []*db.Memory
	candidates []*db.Memory // staged review queue (#485)
	reviewMode bool         // when true, the view shows staged candidates
	selected   int
	scope      string
	search     string
	searching  bool
	err        error

	dbPath          string
	ollamaURL       string
	ollamaModel     string
	ollamaReachable bool
	httpPort        int

	noColorFlag
}

// InitialModel configures state. The noColor parameter disables lipgloss styling
// when true (used when --no-color flag or NO_COLOR env var is set).
func InitialModel(database *db.DB, dbPath, ollamaURL, ollamaModel string, httpPort int, noColor bool) model {
	ollamaReachable := checkOllamaReachable(ollamaURL, ollamaModel)
	m := model{
		db:              database,
		scope:           "",
		dbPath:          dbPath,
		ollamaURL:       ollamaURL,
		ollamaModel:     ollamaModel,
		ollamaReachable: ollamaReachable,
		httpPort:        httpPort,
		noColorFlag:     noColorFlag{enabled: noColor},
	}
	m.loadMemories()
	return m
}

func checkOllamaReachable(url, model string) bool {
	if url == "" || model == "" {
		return false
	}

	body, err := json.Marshal(map[string]string{
		"model":  model,
		"prompt": "symmemory health test",
	})
	if err != nil {
		return false
	}

	client := &http.Client{Timeout: 2 * time.Second}
	resp, err := client.Post(url, "application/json", bytes.NewReader(body))
	if err != nil {
		return false
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusOK {
		return false
	}

	var result struct {
		Embedding []float32 `json:"embedding"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return false
	}
	return len(result.Embedding) > 0
}

func (m *model) loadMemories() {
	mems, err := m.db.ListMemoriesLite(m.scope, 0, 1000)
	if err != nil {
		m.err = err
		return
	}

	if m.search != "" {
		// Basic keyword search inside current scope
		var filtered []*db.Memory
		q := strings.ToLower(m.search)
		for _, mem := range mems {
			if strings.Contains(strings.ToLower(mem.Content), q) {
				filtered = append(filtered, mem)
			}
		}
		m.memories = filtered
	} else {
		m.memories = mems
	}

	if m.selected >= len(m.memories) {
		m.selected = len(m.memories) - 1
	}
	if m.selected < 0 {
		m.selected = 0
	}
}

func (m *model) loadCandidates() {
	cands, err := m.db.ListStagedMemories(100)
	if err != nil {
		m.err = err
		return
	}
	m.candidates = cands
	if m.selected >= len(m.candidates) {
		m.selected = len(m.candidates) - 1
	}
	if m.selected < 0 {
		m.selected = 0
	}
}

func (m model) Init() tea.Cmd {
	return nil
}

func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.KeyMsg:
		// Search mode routing
		if m.searching {
			switch msg.String() {
			case "enter":
				m.searching = false
			case "esc":
				m.search = ""
				m.searching = false
				m.loadMemories()
			case "backspace":
				if len(m.search) > 0 {
					m.search = m.search[:len(m.search)-1]
					m.loadMemories()
				}
			default:
				if len(msg.String()) == 1 {
					m.search += msg.String()
					m.loadMemories()
				}
			}
			return m, nil
		}

		// Standard dashboard keys
		switch msg.String() {
		case "q", "ctrl+c":
			return m, tea.Quit

		case "r":
			// Toggle review mode: browse staged candidates (#485)
			m.reviewMode = !m.reviewMode
			m.selected = 0
			if m.reviewMode {
				m.loadCandidates()
			} else {
				m.loadMemories()
			}
			return m, nil

		case "esc":
			if m.reviewMode {
				m.reviewMode = false
				m.selected = 0
				m.loadMemories()
				return m, nil
			}

		case "up", "k":
			if m.selected > 0 {
				m.selected--
			}

		case "down", "j":
			if m.reviewMode {
				if m.selected < len(m.candidates)-1 {
					m.selected++
				}
			} else if m.selected < len(m.memories)-1 {
				m.selected++
			}

		case "/":
			if !m.reviewMode {
				m.searching = true
			}

		case "p":
			// In review mode: promote the selected candidate
			if m.reviewMode && len(m.candidates) > 0 {
				target := m.candidates[m.selected]
				if err := m.promoteCandidate(target.ID); err == nil {
					m.loadCandidates()
				}
			} else {
				m.scope = "project"
				m.selected = 0
				m.loadMemories()
			}

		case "x":
			// In review mode: reject (delete) the selected candidate
			if m.reviewMode && len(m.candidates) > 0 {
				target := m.candidates[m.selected]
				if err := m.rejectCandidate(target.ID); err == nil {
					m.loadCandidates()
				}
			}

		case "d", "backspace":
			if !m.reviewMode && len(m.memories) > 0 {
				target := m.memories[m.selected]
				_ = m.db.DeleteMemory(target.ID)
				m.loadMemories()
			}

		// Filter scope triggers (disabled in review mode; there 'p' promotes)
		case "g":
			if !m.reviewMode {
				m.scope = "global"
				m.selected = 0
				m.loadMemories()
			}
		case "a":
			if !m.reviewMode {
				m.scope = "agent"
				m.selected = 0
				m.loadMemories()
			}
		case "u":
			if !m.reviewMode {
				m.scope = "user"
				m.selected = 0
				m.loadMemories()
			}
		case "s":
			if !m.reviewMode {
				m.scope = "session"
				m.selected = 0
				m.loadMemories()
			}
		case "*", "c":
			if !m.reviewMode {
				m.scope = ""
				m.selected = 0
				m.loadMemories()
			}
		}
	}
	return m, nil
}

// promoteCandidate approves a staged candidate from the TUI (#485).
func (m *model) promoteCandidate(id string) error {
	mem, err := m.db.GetMemory(id)
	if err != nil {
		return err
	}
	if mem == nil {
		return fmt.Errorf("memory not found: %s", id)
	}
	if err := m.db.SetMemoryReviewStatus(id, db.ReviewApproved); err != nil {
		return err
	}
	_ = m.db.LogAudit("promote", id, mem.Scope, mem.CreatedSession, mem.CreatedBy, "")
	return nil
}

// rejectCandidate discards a staged candidate from the TUI (#485).
func (m *model) rejectCandidate(id string) error {
	mem, err := m.db.GetMemory(id)
	if err != nil {
		return err
	}
	if mem == nil {
		return fmt.Errorf("memory not found: %s", id)
	}
	_ = m.db.LogAudit("reject", id, mem.Scope, mem.CreatedSession, mem.CreatedBy, "")
	return m.db.DeleteMemory(id)
}

func (m model) View() string {
	var s strings.Builder

	// Top banner
	s.WriteString(m.styled(titleStyle, "⚡ SYMAIRA MEMORY (symmemory) — CONSOLE") + "\n")

	if m.err != nil {
		return fmt.Sprintf("Error loading memory console: %v\n", m.err)
	}

	// Dynamic stats column
	activeFilter := "ALL SCOPES"
	if m.scope != "" {
		activeFilter = strings.ToUpper(m.scope)
	}

	ollamaStatus := "down"
	if m.ollamaReachable {
		ollamaStatus = "up"
	}
	httpStatus := "stdio only"
	if m.httpPort > 0 {
		httpStatus = fmt.Sprintf(":%d", m.httpPort)
	}
	statsText := fmt.Sprintf("Active Filter: %s\nTotal Memories: %d\nDB: %s\nOllama: %s (%s %s)\nHTTP: %s",
		activeFilter, len(m.memories), m.dbPath,
		m.ollamaModel, ollamaStatus, m.ollamaURL,
		httpStatus)

	s.WriteString(m.styled(statsStyle, statsText) + "\n\n")

	// Search Indicator
	if m.searching {
		searchStyle := lipgloss.NewStyle().Foreground(lipgloss.Color("#F9E2AF"))
		s.WriteString(m.styled(searchStyle, "🔍 Search Keyword: "+m.search+"_") + "\n\n")
	} else if m.search != "" {
		searchStyle := lipgloss.NewStyle().Foreground(lipgloss.Color("#A6E3A1"))
		s.WriteString(m.styled(searchStyle, "🔍 Active Search: "+m.search+" (Press '/' to edit, 'esc' to clear)") + "\n\n")
	}

	// Main scrollable list: staged candidates in review mode, memories otherwise
	if m.reviewMode {
		s.WriteString("📥 Staged Candidates (review queue):\n")
		s.WriteString("========================================\n")
		if len(m.candidates) == 0 {
			emptyStyle := lipgloss.NewStyle().Foreground(lipgloss.Color("#A6E3A1")).PaddingLeft(4)
			s.WriteString(m.styled(emptyStyle, "No staged candidates awaiting review.") + "\n")
		} else {
			for i, cand := range m.candidates {
				kind := cand.Kind
				if kind == "" {
					kind = "unclassified"
				}
				badge := m.scopeBadge(cand.Scope)
				line := fmt.Sprintf("%s [%s] %s", badge, kind, cand.Content)
				if i == m.selected {
					s.WriteString(m.styled(selectedStyle, "👉 "+line) + "\n")
					s.WriteString(m.styled(metaStyle.PaddingLeft(6), fmt.Sprintf("ID: %s | Saved: %s", cand.ID, cand.CreatedAt.Format("2006-01-02 15:04"))) + "\n")
				} else {
					s.WriteString(m.styled(normalStyle, "   "+line) + "\n")
				}
				s.WriteString("\n")
			}
		}
		s.WriteString(m.styled(footerStyle,
			"Review controls: [j/k/↑/↓] Navigate | [p] Promote | [x] Reject | [esc] Back | [q] Exit",
		) + "\n")
		return s.String()
	}

	s.WriteString("Persistent Memory Elements:\n")
	s.WriteString("========================================\n")

	if len(m.memories) == 0 {
		emptyStyle := lipgloss.NewStyle().Foreground(lipgloss.Color("#F38BA8")).PaddingLeft(4)
		s.WriteString(m.styled(emptyStyle, "No memories match current scope filters.") + "\n")
	} else {
		for i, mem := range m.memories {
			badge := m.scopeBadge(mem.Scope)

			// Highlight selected memory
			if i == m.selected {
				s.WriteString(m.styled(selectedStyle, fmt.Sprintf("👉 %s  %s", badge, mem.Content)) + "\n")
				s.WriteString(m.styled(metaStyle.PaddingLeft(6), fmt.Sprintf("ID: %s | Saved: %s", mem.ID, mem.CreatedAt.Format("2006-01-02 15:04"))) + "\n")
			} else {
				s.WriteString(m.styled(normalStyle, fmt.Sprintf("   %s  %s", badge, mem.Content)) + "\n")
			}
			s.WriteString("\n")
		}
	}

	// Keyboard Controls Footer
	s.WriteString(m.styled(footerStyle,
		"Controls: [j/k/↑/↓] Navigate | [d/backspace] Delete | [/] Filter Keyword | [r] Review staged candidates | [g] Global | [p] Project | [a] Agent | [u] User | [s] Session | [*] All Scopes | [q] Exit",
	) + "\n")

	return s.String()
}

// RunDashboard launches the Bubble Tea console.
func RunDashboard(database *db.DB, dbPath, ollamaURL, ollamaModel string, httpPort int, noColor bool) error {
	defer func() {
		if r := recover(); r != nil {
			fmt.Fprintf(os.Stderr, "TUI panic recovered: %v\n", r)
		}
	}()
	p := tea.NewProgram(InitialModel(database, dbPath, ollamaURL, ollamaModel, httpPort, noColor), tea.WithAltScreen())
	_, err := p.Run()
	return err
}
