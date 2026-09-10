package usage

import (
	"regexp"
	"strconv"
	"strings"
)

// antigravityServerCandidate is one discovered language-server process.
type antigravityServerCandidate struct {
	pid       int
	csrfToken string
}

// antigravityParseCandidates extracts Antigravity language-server
// processes from ps output. Recognizes the app/IDE language_server*
// binaries (scoped to Antigravity by --app_data_dir antigravity or an
// antigravity path) and the agy CLI binary (which needs no CSRF token).
func antigravityParseCandidates(processList string) []antigravityServerCandidate {
	var candidates []antigravityServerCandidate
	for _, line := range strings.Split(processList, "\n") {
		trimmed := strings.TrimLeft(line, " ")
		idx := strings.IndexByte(trimmed, ' ')
		if idx < 0 {
			continue
		}
		pidStr := trimmed[:idx]
		pid, err := strconv.Atoi(pidStr)
		if err != nil {
			continue
		}
		command := strings.TrimLeft(trimmed[idx:], " ")
		lower := strings.ToLower(command)

		isAppOrIDEServer := (strings.Contains(lower, "language_server") || strings.Contains(lower, "language-server")) &&
			(strings.Contains(lower, "antigravity") ||
				(strings.Contains(lower, "--app_data_dir") && strings.Contains(command, "antigravity")))
		isCLI := strings.Contains(lower, "/agy") || strings.Contains(lower, "antigravity-cli") || strings.Contains(lower, "antigravity_cli")
		if !isAppOrIDEServer && !isCLI {
			continue
		}

		var csrfToken string
		if tokenIdx := strings.Index(command, "--csrf_token"); tokenIdx >= 0 {
			after := strings.TrimLeft(command[tokenIdx+len("--csrf_token"):], " ")
			fields := strings.SplitN(after, " ", 2)
			if len(fields) > 0 && fields[0] != "" {
				csrfToken = fields[0]
			}
		}
		candidates = append(candidates, antigravityServerCandidate{pid: pid, csrfToken: csrfToken})
	}
	return candidates
}

var antigravityPortPattern = regexp.MustCompile(`:([0-9]{1,5})(?:\s|$)`)

// antigravityParsePorts extracts listening TCP ports from
// lsof -iTCP -sTCP:LISTEN output. The NAME column
// (TCP 127.0.0.1:34567 (LISTEN)) contains spaces, so the whole line is
// scanned for :port tokens.
func antigravityParsePorts(portList string) []int {
	var ports []int
	seen := map[int]bool{}
	for _, line := range strings.Split(portList, "\n") {
		if !strings.Contains(line, "(LISTEN)") {
			continue
		}
		match := antigravityPortPattern.FindStringSubmatch(line)
		if len(match) < 2 {
			continue
		}
		port, err := strconv.Atoi(match[1])
		if err != nil || seen[port] {
			continue
		}
		seen[port] = true
		ports = append(ports, port)
	}
	return ports
}
