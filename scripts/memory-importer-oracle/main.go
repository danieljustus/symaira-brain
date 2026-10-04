// Prepared constructor oracle. Build only inside a private COPY of the frozen
// module. It imports the real importer owners; no production Go is rewritten.
package main

import (
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"sort"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
	"github.com/danieljustus/symaira-brain/internal/memory/importer/aider"
	"github.com/danieljustus/symaira-brain/internal/memory/importer/codexmemory"
	"github.com/danieljustus/symaira-brain/internal/memory/importer/curatedmemory"
	"github.com/danieljustus/symaira-brain/internal/memory/importer/obsidian"
	"github.com/danieljustus/symaira-brain/internal/memory/importer/shellhistory"
)

type inputCase struct {
	ID, Family, RootHex, PathHex, Since                                             string
	AllowedHex, DeniedHex, FiltersHex, TagsHex, ExcludedFoldersHex, ExcludedTagsHex []string
	Direct                                                                          bool
}

func raw(encoded string) string {
	b, err := hex.DecodeString(encoded)
	if err != nil {
		panic(err)
	}
	return string(b)
}

func stringsFromHex(values []string) []string {
	out := make([]string, len(values))
	for i, value := range values {
		out[i] = raw(value)
	}
	return out
}

func hexString(value string) string { return hex.EncodeToString([]byte(value)) }

func metadata(values map[string]string) any {
	if values == nil {
		return nil
	}
	keys := make([]string, 0, len(values))
	for key := range values {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	out := make([][2]string, 0, len(keys))
	for _, key := range keys {
		out = append(out, [2]string{hexString(key), hexString(values[key])})
	}
	return out
}

func refWire(ref importer.SessionRef) any {
	return map[string]any{"tool_hex": hexString(ref.Tool), "id_hex": hexString(ref.SessionID),
		"path_hex": hexString(ref.Path), "modified": ref.ModifiedAt.Format(time.RFC3339Nano), "metadata_hex": metadata(ref.Metadata)}
}

func factWire(fact importer.ImportedFact) any {
	var evidence []any
	for _, ext := range fact.Evidence {
		evidence = append(evidence, map[string]any{"source_id_hex": hexString(ext.Source.ID), "source_kind_hex": hexString(ext.Source.Kind),
			"text_hex": hexString(ext.Text), "evidence_hex": hexString(ext.EvidenceText), "start": ext.Span.Start, "end": ext.Span.End,
			"alignment": ext.AlignmentStatus})
	}
	return map[string]any{"content_hex": hexString(fact.Content), "source_hex": hexString(fact.Source), "id_hex": hexString(fact.SessionID),
		"timestamp": fact.Timestamp.Format(time.RFC3339Nano), "metadata_hex": metadata(fact.Metadata), "evidence": evidence}
}

func traits(imp importer.SessionImporter) map[string]any {
	out := map[string]any{"category": nil, "privacy": nil, "pii_guard": nil, "transcript": nil, "staged": nil, "untrusted": nil, "incremental": false}
	if v, ok := imp.(importer.Categorizable); ok {
		out["category"] = v.Category()
	}
	if v, ok := imp.(importer.PrivacyAware); ok {
		out["privacy"] = v.PrivacyLevel()
		out["pii_guard"] = v.RequiresPIIGuard()
	}
	if v, ok := imp.(importer.TranscriptImporter); ok {
		out["transcript"] = v.IsTranscript()
	}
	if v, ok := imp.(importer.StagedImporter); ok {
		out["staged"] = v.StageImportedFacts()
	}
	if v, ok := imp.(importer.UntrustedContentImporter); ok {
		out["untrusted"] = v.ContentIsUntrusted()
	}
	_, out["incremental"] = imp.(importer.IncrementalImporter)
	return out
}

func owner(c inputCase) importer.SessionImporter {
	root := raw(c.RootHex)
	switch c.Family {
	case "codex-memory":
		return codexmemory.NewCodexMemoryImporter(root, codexmemory.ApplicationPolicy{Allowed: stringsFromHex(c.AllowedHex), Denied: stringsFromHex(c.DeniedHex)})
	case "curated-memory":
		return curatedmemory.NewCuratedMemoryImporter(root)
	case "aider":
		return aider.NewAiderImporter([]string{root})
	case "shell-history":
		return shellhistory.NewShellHistoryImporter(root, true, stringsFromHex(c.FiltersHex))
	case "obsidian":
		return obsidian.NewObsidianImporter(root, "unused-folder", stringsFromHex(c.TagsHex), stringsFromHex(c.ExcludedFoldersHex), stringsFromHex(c.ExcludedTagsHex))
	default:
		panic("unsupported oracle family: " + c.Family)
	}
}

func execute(c inputCase) any {
	imp := owner(c)
	var since time.Time
	if c.Since != "" {
		var err error
		since, err = time.Parse(time.RFC3339Nano, c.Since)
		if err != nil {
			panic(err)
		}
	}
	refs, discoverErr := imp.DiscoverSessions(since)
	if c.Direct {
		refs = []importer.SessionRef{{Tool: imp.Name(), SessionID: "direct:" + c.ID, Path: raw(c.PathHex), Metadata: map[string]string{}}}
		discoverErr = nil
	}
	var wireRefs []any
	if refs != nil {
		wireRefs = make([]any, 0, len(refs))
	}
	var imports []any
	for _, ref := range refs {
		wireRefs = append(wireRefs, refWire(ref))
		for repeat := 0; repeat < 2; repeat++ {
			facts, err := imp.ImportSession(ref)
			var rows []any
			if facts != nil {
				rows = make([]any, 0, len(facts))
			}
			for _, fact := range facts {
				rows = append(rows, factWire(fact))
			}
			message := ""
			if err != nil {
				message = err.Error()
			}
			imports = append(imports, map[string]any{"repeat": repeat, "id_hex": hexString(ref.SessionID), "facts": rows, "error": message})
		}
	}
	message := ""
	if discoverErr != nil {
		message = discoverErr.Error()
	}
	return map[string]any{"id": c.ID, "family": c.Family, "traits": traits(imp), "sessions": wireRefs, "imports": imports, "error": message}
}

func main() {
	if len(os.Args) != 2 {
		panic("usage: owned-importer-oracle INPUT.json")
	}
	file, err := os.Open(os.Args[1])
	if err != nil {
		panic(err)
	}
	defer file.Close()
	var cases []inputCase
	if err = json.NewDecoder(file).Decode(&cases); err != nil {
		panic(err)
	}
	if len(cases) == 0 {
		panic("zero cases refused")
	}
	encoder := json.NewEncoder(os.Stdout)
	for _, c := range cases {
		if err = encoder.Encode(execute(c)); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
	}
}
