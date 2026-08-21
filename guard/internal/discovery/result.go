package discovery

import (
	"fmt"
	"os"
)

// Status describes how completely a client's MCP configuration source was
// mapped during a scan.
type Status string

const (
	// StatusExact means the source was fully mapped to server entries.
	StatusExact Status = "exact"

	// StatusApproximate means the source was mapped with assumptions.
	StatusApproximate Status = "approximate"

	// StatusManual means the source requires manual review.
	StatusManual Status = "manual"

	// StatusUnsupported means the source could not be mapped at all.
	StatusUnsupported Status = "unsupported"
)

// Finding reports a client config source (or an entry within it) that could
// not be mapped exactly during a scan. It names the client, the source path,
// a status, and a human-readable message.
type Finding struct {
	Client  Client
	Path    string
	Status  Status
	Message string
}

// Result is the outcome of a discovery scan: the normalised servers that
// could be mapped, plus findings for every source or entry that could not.
type Result struct {
	Servers  []Server
	Findings []Finding
}

// ScanAll scans all supported clients and returns a Result. Every client
// source that cannot be scanned — missing file, unreadable file, or parse
// failure — is reported as a [Finding]; nothing is skipped silently.
func ScanAll() Result {
	return ScanAllWithFS(osFS{})
}

// ScanAllWithFS is like [ScanAll] but uses the provided [FS] for file access,
// making it straightforward to test.
func ScanAllWithFS(fsys FS) Result {
	var res Result
	for _, src := range clientSources() {
		servers, findings := scanSourceWithFS(fsys, src.Client, src.Path)
		res.Servers = append(res.Servers, servers...)
		res.Findings = append(res.Findings, findings...)
	}
	return res
}

// scanSourceWithFS scans a single client source and returns its servers and
// findings. Errors never propagate: they become unsupported findings.
func scanSourceWithFS(fsys FS, client Client, path string) ([]Server, []Finding) {
	data, err := fsys.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, []Finding{{
				Client:  client,
				Path:    path,
				Status:  StatusUnsupported,
				Message: "config file not found",
			}}
		}
		return nil, []Finding{{
			Client:  client,
			Path:    path,
			Status:  StatusUnsupported,
			Message: fmt.Sprintf("read config: %v", err),
		}}
	}

	servers, findings, err := parseClientData(client, path, data)
	if err != nil {
		findings = append(findings, Finding{
			Client:  client,
			Path:    path,
			Status:  StatusUnsupported,
			Message: fmt.Sprintf("parse config: %v", err),
		})
		return servers, findings
	}
	return servers, findings
}

// parseClientData parses a client config payload into servers and findings.
// The error is returned unwrapped so each caller can add its own context.
func parseClientData(client Client, path string, data []byte) ([]Server, []Finding, error) {
	switch client {
	case ClientHermes, ClientClaudeDesktop, ClientCursor, ClientVSCode:
		return parseMCPserversFormat(client, path, data)
	case ClientOpenCode:
		return parseOpenCodeFormat(client, path, data)
	default:
		return nil, nil, fmt.Errorf("unsupported client %q", client)
	}
}
