"""Small owned regular-input fixtures; never invokes an SDK or product."""
from contextlib import ExitStack
import json
from unittest.mock import patch
import sdk


def prepare(root):
    value = {name: root / name for name in ("go", "rust", "runner")}
    value["runner"].mkdir()
    for role, names in (("go", ["bin/go", "src/fmt/doc.go", "go.env", "pkg/include/textflag.h"]),
                        ("rust", ["bin/cargo", "bin/rustc", "lib/runtime.so"])):
        for name in names:
            path = value[role] / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"owned source bytes\n")
    value["expected"] = {role: sdk.inventory(value[role]) for role in ("go", "rust")}
    value["plan"] = {"sdk_platforms": {"linux-x86_64": {
        "go": {"files": {"bin/go": sdk.sha(value["go"] / "bin/go")},
               "source_files": {"src/fmt/doc.go": sdk.sha(value["go"] / "src/fmt/doc.go")}},
        "rust": {"files": {name: sdk.sha(value["rust"] / name)
                           for name in ("bin/cargo", "bin/rustc")}}}}}
    (value["runner"] / "sdk-inputs.json").write_text(json.dumps({
        "kind": "memory760-whole-sdk-v1", "platforms": {"linux-x86_64": value["expected"]}}))
    return value


def selected(binding, value):
    stack = ExitStack()
    for control in (patch.object(binding, "HERE", value["runner"]),
                    patch.object(binding, "trusted", return_value=value["plan"]),
                    patch.object(binding.platform, "system", return_value="Linux"),
                    patch.object(binding.platform, "machine", return_value="x86_64")):
        stack.enter_context(control)
    return stack
