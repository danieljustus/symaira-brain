"""Fail-closed source qualification for inherited early output errors.

This evidence never substitutes for actual parent process parity. Profiles bind
complete reviewed owner bodies. An unfamiliar body requires another review even
when its proposed change looks unrelated to output.
"""
import hashlib

CLI = "rust/symbrain-cli/src/lib.rs"
MAIN = "rust/symbrain-cli/src/main.rs"
CORE = "rust/symbrain-core/src/lib.rs"
OUTPUT = "rust/symbrain-core/src/output.rs"
EXIT = "rust/symbrain-core/src/exit.rs"
OWNER_PATHS = (CLI, MAIN, CORE, OUTPUT, EXIT)
PARENT_REFS = (
    "01f41906e2e021db3c693ec97617bb701e4dad8a",
    "faa8f12f6c77aad694003c9fa8c5e66d94919ba0",
)
PARENT_HASHES = {
    CLI: "decf6229c8fd9295893c456ca15dc8ab20ab79390988de051591bc80f21aa9ca",
    MAIN: "e6dd8093173873f5eddab3d213368e90aacad77092d03c37d5b5b1ff3426a7a6",
    CORE: "9e9e2d32f3cd59f939544e213a7cd05a585513d4ea8962016ca8b3cdb40d6cc4",
    OUTPUT: "a8e01e7d19d7eadc8f71d1634166691294b0223959248b0a8fc50b1d8264856a",
    EXIT: "7f5efe594eb3bc7f5957a12bfd9642e07846fc71cff181647884d781ae66d2ea",
}
MEMORY_HASHES = {
    **PARENT_HASHES,
    CLI: "7ba14c445953dc3a9d8267562853cf2bfb924c2e90892413a28b8a7b1b9bd6dd",
    MAIN: "3e94f3f39e073b32151350707bc0416a3d01bba19903a0e5ad4a2cb47cf4fff7",
}
GUARD_CORE_HASH = "4df524358123e41fb3a7959de3dd3064e477dd738bf8b36faf4cec961dc77d75"
SOURCE_HASHES = {
    **MEMORY_HASHES,
    CLI: "b4fa7a9df8312bb72cca213bb26b867537f8f916fa30ea6c140e446a57c3f2e2",
    CORE: GUARD_CORE_HASH,
}
BRAIN_HASHES = {
    CLI: 'b4fa7a9df8312bb72cca213bb26b867537f8f916fa30ea6c140e446a57c3f2e2',
    MAIN: '3e94f3f39e073b32151350707bc0416a3d01bba19903a0e5ad4a2cb47cf4fff7',
    CORE: 'a6e887f6f1228fb340164202f8bcc099afaed61115991e8fa6be40d518f29d68',
    OUTPUT: 'a8e01e7d19d7eadc8f71d1634166691294b0223959248b0a8fc50b1d8264856a',
    EXIT: '7f5efe594eb3bc7f5957a12bfd9642e07846fc71cff181647884d781ae66d2ea',
}
PROFILES = {
    "brain765-main78f": BRAIN_HASHES,
    "source806-d07": SOURCE_HASHES,
    "original-parent": PARENT_HASHES,
    "memory803-5a4": MEMORY_HASHES,
    "guard805-cdb": {**PARENT_HASHES, CORE: GUARD_CORE_HASH},
    "memory803-reviewed-guard-integration": {**MEMORY_HASHES, CORE: GUARD_CORE_HASH},
}


def hashes(sources):
    if set(sources) != set(OWNER_PATHS):
        raise ValueError("inherited output owner source set differs")
    return {name: hashlib.sha256(sources[name]).hexdigest() for name in OWNER_PATHS}


def section(source, start, end):
    # Not a general Rust parser: whole-body digest checks precede extraction.
    if source.count(start) != 1:
        raise ValueError("inherited output owner section is ambiguous")
    offset = source.index(start)
    limit = source.find(end, offset + len(start))
    if limit < 0:
        raise ValueError("inherited output owner section is missing")
    return source[offset:limit]


def early_blocks(source, memory):
    entry = b"fn run_native(\n" if memory else b"pub fn run_in_process(\n"
    function = section(source, entry, b"    if normalized.is_empty()")
    return {
        "early_output_return": section(function, b"    let peeked =", b"\n\n"),
        "peek_command": section(source, b"fn peek_command(", b"\n}\n"),
        "is_output_command": section(source, b"fn is_output_command(", b"\n}\n"),
    }


def qualify(reference, parent, current):
    if reference not in PARENT_REFS or hashes(parent) != PARENT_HASHES:
        raise ValueError("inherited output parent owner is not the immutable reviewed source")
    current_hashes = hashes(current)
    profiles = [name for name, expected in PROFILES.items() if expected == current_hashes]
    if len(profiles) != 1:
        raise ValueError("inherited output current owner needs explicit source review")
    parent_blocks = early_blocks(parent[CLI], False)
    current_blocks = early_blocks(current[CLI], current_hashes[CLI] in (MEMORY_HASHES[CLI], SOURCE_HASHES[CLI]))
    if parent_blocks != current_blocks:
        raise ValueError("inherited early output path differs from actual parent source")
    return {
        "qualification": profiles[0],
        "parent_owner_sources_sha256": PARENT_HASHES,
        "current_owner_sources_sha256": current_hashes,
        "unchanged_early_blocks_sha256": {
            name: hashlib.sha256(body).hexdigest() for name, body in parent_blocks.items()
        },
        "scope": "only inherited-global cases; actual parent exit/stdout/stderr and state still required",
        "go_parity_claim": False,
    }
