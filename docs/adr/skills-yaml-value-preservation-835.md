# Accepted target: lossless Skills YAML values (#835)

The current Go and Rust Skills encoders must preserve frontmatter string values, keys, lists, nested metadata and opaque body bytes. The frozen Go oracle and its original observations remain unchanged.

## Explicit legacy differences

1. The pinned yaml.v3 v3.0.1 literal-emitter path loses one initial LF for `"\n"`, `"\n\n"` and `"\n\nX"`. The approved target selects double-quoted strings for leading-LF values instead of copying this loss. The original Frontmatter normalization also marshaled and reparsed values; current Go normalization modifies only the scalar nodes and decodes directly, avoiding another lossy emitter pass.
2. yaml.v3 emits the literal metadata key `<<` as a YAML merge key and rejects its own output when the value is not a merge mapping. The additionally approved target double-quotes only that metadata key, including nested keys. Scalar values equal to `<<` remain on the unchanged legacy path.

The 740-case source-bound scalar corpus preserves the original legacy bytes in `yaml835-shared-encoder-740.jsonl`. Separate captured target fixtures identify twelve leading-LF rows and two merge-key rows. Every other row remains an exact-byte comparison. Go regressions execute the unchanged library paths separately to assert the original loss/rejection before asserting target preservation.

## Non-waivers

- No general YAML normalization, output-byte rewriting, fixture overwrite or whole-subtree exclusion.
- The modified library/backend is private to Skills encoding. Parsing and other workspace YAML users retain the existing dependencies and policy.
- Ordinary string styles, types, sequences, metadata ordering, floats, document assembly, body bytes, caches, marker files and digests remain subject to strict executable comparisons.
- Scalar success is not typed-document, caller, CLI/MCP, native-platform or release acceptance. Linux, macOS and Windows gates, strict lint, independent review and exact-head CI remain required before PR #813 merges.

Both target decisions were explicitly approved by the user. This document records the target, not acceptance or issue closure.
