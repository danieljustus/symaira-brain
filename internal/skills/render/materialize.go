package render

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"

	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

// RenderAll writes target-specific skill folders under outDir and returns the
// successfully rendered items along with any per-target errors.
func RenderAll(bundle *skill.Bundle, outDir string, targets []Target, meta ...RenderMeta) ([]Rendered, []error) {
	if len(targets) == 0 {
		targets = DefaultTargets()
	}
	dstRoot, err := openDestinationRoot(outDir)
	if err != nil {
		return nil, []error{err}
	}
	defer dstRoot.Close()
	// The source tree hash is a per-bundle property; compute it once and
	// reuse it for every target instead of walking the tree per target.
	treeHash := sourceTreeHash(bundle)
	var rendered []Rendered
	var errs []error
	for _, target := range targets {
		item, err := RenderTarget(bundle, target, meta...)
		if err != nil {
			errs = append(errs, fmt.Errorf("target %s: %w", target, err))
			continue
		}
		rel := filepath.Join(string(target), item.Name)
		if err := writeRenderedRoot(bundle, dstRoot, rel, item, target, treeHash); err != nil {
			errs = append(errs, fmt.Errorf("target %s: %w", target, err))
			continue
		}
		item.Path = filepath.Join(outDir, rel)
		rendered = append(rendered, item)
	}
	return rendered, errs
}

// stagingMkdirTemp is the temporary-directory factory used by StagingRender.
// It is a variable so tests can observe and control staging directories.
var stagingMkdirTemp = os.MkdirTemp

// StagingRender renders the bundle into a fresh temporary directory and
// returns the rendered items together with a cleanup function that removes
// that directory. Comparison renders (diff, and later sync) must use this
// helper instead of writing into RenderDir, which is only written when a
// result is actually committed: in symlink install mode RenderDir is the
// live installed artifact, and rewriting it would destroy the very state a
// comparison wants to read. The cleanup function must be called (typically
// via defer) on every exit path, including error paths; when no target
// produced output the staging directory is removed before the first render
// error is returned. The source_hash short-circuit never fires in a fresh
// staging directory (no marker present), so staging renders always do full
// work.
func StagingRender(bundle *skill.Bundle, targets []Target, meta ...RenderMeta) ([]Rendered, func(), error) {
	dir, err := stagingMkdirTemp("", "symskills-staging-")
	if err != nil {
		return nil, func() {}, err
	}
	rendered, errs := RenderAll(bundle, dir, targets, meta...)
	if len(rendered) == 0 {
		_ = os.RemoveAll(dir)
		if len(errs) > 0 {
			return nil, func() {}, errs[0]
		}
		return nil, func() {}, fmt.Errorf("no render output for the requested targets")
	}
	return rendered, func() { _ = os.RemoveAll(dir) }, nil
}

// CachedStagingRender reuses a persistent comparison render when the source
// bundle fingerprint and renderer version are unchanged. The cache is never
// used for installs; it only avoids repeating the render pipeline during
// read-only status scans.
func CachedStagingRender(bundle *skill.Bundle, targets []Target, cacheRoot string, meta ...RenderMeta) ([]Rendered, func(), error) {
	if cacheRoot == "" {
		return StagingRender(bundle, targets, meta...)
	}
	cacheDirRoot, err := openDestinationRoot(cacheRoot)
	if err != nil {
		return nil, func() {}, err
	}
	defer cacheDirRoot.Close()
	if len(targets) == 0 {
		targets = DefaultTargets()
	}
	fingerprint, err := sourceFingerprint(bundle)
	if err != nil {
		return nil, func() {}, err
	}
	const cacheVersion = "status-render-v1"
	var rendered []Rendered
	var errs []error
	for _, target := range targets {
		keyHash := sha256.Sum256([]byte(cacheVersion + "\x00" + bundle.Root + "\x00" + string(target)))
		key := hex.EncodeToString(keyHash[:])
		dstRel := filepath.Join("status-render", key)
		dst := filepath.Join(cacheRoot, dstRel)
		metaRel := dstRel + ".json"
		// The persistent sidecar is only a cache hint. Render the expected
		// output and let writeRenderedRoot verify the complete tree before it
		// reuses anything; a poisoned marker or modified file rebuilds it.
		item, rerr := RenderTarget(bundle, target, meta...)
		if rerr != nil {
			errs = append(errs, fmt.Errorf("target %s: %w", target, rerr))
			continue
		}
		if err := writeRenderedRoot(bundle, cacheDirRoot, dstRel, item, target, fingerprint); err != nil {
			errs = append(errs, fmt.Errorf("target %s: %w", target, err))
			continue
		}
		cacheData, merr := json.Marshal(struct {
			Fingerprint string `json:"fingerprint"`
			Target      Target `json:"target"`
			Name        string `json:"name"`
		}{fingerprint, target, item.Name})
		if merr != nil {
			errs = append(errs, fmt.Errorf("target %s: %w", target, merr))
			continue
		}
		if err := writeMaterializedFile(cacheDirRoot, filepath.ToSlash(metaRel), append(cacheData, '\n'), 0o644); err != nil {
			errs = append(errs, fmt.Errorf("target %s: %w", target, err))
			continue
		}
		item.Path = dst
		rendered = append(rendered, item)
	}
	if len(rendered) == 0 {
		if len(errs) > 0 {
			return nil, func() {}, errs[0]
		}
		return nil, func() {}, fmt.Errorf("no render output for the requested targets")
	}
	return rendered, func() {}, nil
}
