package managed

import (
	"bufio"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"strings"
)

// downloadURL constructs the GitHub release asset download URL.
func downloadURL(repo, asset string) string {
	return fmt.Sprintf("https://github.com/%s/releases/latest/download/%s", repo, asset)
}

// downloadFile downloads a URL to the given path. It returns the number
// of bytes written and any error encountered.
func downloadFile(ctx context.Context, url, dest string) (int64, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return 0, fmt.Errorf("download: create request: %w", err)
	}
	req.Header.Set("Accept", "application/octet-stream")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return 0, fmt.Errorf("download: %s: %w", url, err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return 0, fmt.Errorf("download: %s: HTTP %d", url, resp.StatusCode)
	}

	f, err := os.Create(dest)
	if err != nil {
		return 0, fmt.Errorf("download: create %s: %w", dest, err)
	}
	defer func() {
		f.Close()
		// Clean up partial file on error
		if err != nil {
			os.Remove(dest)
		}
	}()

	n, err := io.Copy(f, resp.Body)
	if err != nil {
		return 0, fmt.Errorf("download: write %s: %w", dest, err)
	}
	return n, nil
}

// verifyChecksum checks a file against a SHA-256 checksum string.
func verifyChecksum(path, expected string) error {
	f, err := os.Open(path)
	if err != nil {
		return fmt.Errorf("checksum: open %s: %w", path, err)
	}
	defer f.Close()

	h := sha256.New()
	if _, err := io.Copy(h, f); err != nil {
		return fmt.Errorf("checksum: hash %s: %w", path, err)
	}
	actual := hex.EncodeToString(h.Sum(nil))
	if actual != strings.ToLower(strings.TrimSpace(expected)) {
		return fmt.Errorf("checksum: mismatch for %s: got %s, want %s", path, actual, expected)
	}
	return nil
}

// findChecksumInFile extracts the SHA-256 checksum for a given asset
// filename from a checksums.txt file. The file format is one line per
// asset: "<hex>  <filename>" or "<hex> *<filename>".
func findChecksumInFile(checksumsPath, assetName string) (string, error) {
	f, err := os.Open(checksumsPath)
	if err != nil {
		return "", fmt.Errorf("checksums: open: %w", err)
	}
	defer f.Close()

	scanner := bufio.NewScanner(f)
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		// Format: "<hex>  <filename>" or "<hex> *<filename>"
		parts := strings.Fields(line)
		if len(parts) < 2 {
			continue
		}
		if parts[len(parts)-1] == assetName {
			return parts[0], nil
		}
	}
	return "", fmt.Errorf("checksums: asset %q not found in %s", assetName, checksumsPath)
}

// verifyCosign checks a cosign signature (.sig) and certificate (.pem)
// against an artifact. Returns nil if cosign is not available (graceful
// degradation per the issue's acceptance criteria).
func verifyCosign(ctx context.Context, artifactPath string) error {
	// Check if cosign is available
	cosignPath, err := exec.LookPath("cosign")
	if err != nil {
		// cosign not installed — skip verification gracefully
		return nil
	}

	sigPath := artifactPath + ".sig"
	pemPath := artifactPath + ".pem"

	// Check if signature files exist
	if _, err := os.Stat(sigPath); os.IsNotExist(err) {
		return nil // no signature to verify
	}

	args := []string{"verify-blob", artifactPath,
		"--signature", sigPath,
		"--certificate", pemPath,
		"--certificate-identity-regexp", ".*",
		"--certificate-oidc-issuer-regexp", ".*",
	}

	cmd := exec.CommandContext(ctx, cosignPath, args...)
	output, err := cmd.CombinedOutput()
	if err != nil {
		return fmt.Errorf("cosign verify: %s: %w (output: %s)", artifactPath, err, string(output))
	}
	return nil
}
