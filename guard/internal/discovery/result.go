package discovery

import (
	"fmt"
	"os"

	"github.com/danieljustus/symaira-corekit/mcpcfgkit"
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
	kit := mcpcfgkit.ScanAllWithFS(kitFS{fsys}, toKitSources(clientSourcePairs()))
	res := Result{
		Servers:  make([]Server, 0, len(kit.Servers)),
		Findings: make([]Finding, 0, len(kit.Findings)),
	}
	for _, s := range kit.Servers {
		res.Servers = append(res.Servers, fromKitServer(s))
	}
	for _, f := range kit.Findings {
		res.Findings = append(res.Findings, Finding{
			Client:  Client(f.Client),
			Path:    f.Path,
			Status:  Status(f.Status),
			Message: f.Message,
		})
	}
	// The historical contract reports missing files with this exact message.
	for i := range res.Findings {
		if res.Findings[i].Status == StatusUnsupported &&
			containsNotExist(res.Findings[i].Message) {
			res.Findings[i].Message = "config file not found"
		}
	}
	return res
}

var _ = fmt.Sprintf
var _ = os.IsNotExist
