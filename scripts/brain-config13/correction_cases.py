"""Additive prepared regressions; original 102/152/174 corpora are untouched."""
import os
from process import FIELDS
from values import VALID

PREFIXES = [("fffe", b"\xff\xfe"), ("feff", b"\xfe\xff"), ("utf8-bom", b"\xef\xbb\xbf")]


def loader_cases():
    for name, prefix in PREFIXES:
        for stage in ("global", "project"):
            for field, literal, _environment in VALID:
                yield dict(id=f"prefix-{name}-{stage}-{field}",
                           **{stage + "_bytes": prefix + f"{field}={literal}\n".encode()})
    for name, prefix in PREFIXES[:2]:
        for stage in ("global", "project"):
            for field, content in FIELDS:
                yield dict(id=f"type-{name}-{stage}-{field}",
                           **{stage + "_bytes": prefix + content})
            for suffix, content in (("syntax", b"[bad config"),
                                    ("remaining-utf8", b"#\xff\n"),
                                    ("repeated-marker", prefix + b"audit.enabled=false\n"),
                                    ("utf16-content", b"a\x00u\x00d\x00i\x00t\x00")):
                yield dict(id=f"negative-{name}-{stage}-{suffix}",
                           **{stage + "_bytes": prefix + content})


def consumer_cases():
    for name, prefix in PREFIXES:
        for stage in ("global", "project"):
            for harness in ("claude", "codex"):
                for dry in (False, True):
                    yield dict(id=f"prefix-{name}-{stage}-{harness}-{dry}", command="install",
                               harness=harness, dry=dry, positive=True,
                               **{stage + "_bytes": prefix + b'default_profile="owned"\n'})
    for name, prefix in PREFIXES[:2]:
        for stage in ("global", "project"):
            for field, content in FIELDS:
                for command in ("install", "mcp"):
                    yield dict(id=f"type-{name}-{stage}-{field}-{command}", command=command,
                               harness="codex", dry=False, fatal=True,
                               **{stage + "_bytes": prefix + content})
            for suffix, content in (("syntax", b"[bad config"), ("utf8", b"#\xff\n")):
                for command in ("install", "mcp"):
                    yield dict(id=f"negative-{name}-{stage}-{suffix}-{command}",
                               command=command, harness="codex", dry=False, fatal=True,
                               **{stage + "_bytes": prefix + content})
    profiles = [("replacement-rune", "owned�".encode()),
                ("unicode-js-html", "ownedé\u2028<>&'".encode()),
                ("quote-controls", b'owned"\\\n\t\x01\x7f')]
    if os.name == "posix":
        profiles += [("ff", b"owned\xff"), ("incomplete", b"owned\xe2\x82"),
                     ("combined", b"owned\xff\xe2\x82"),
                     ("mixed-literal", "owned�".encode() + b"\xe2\x82")]
    for name, raw in profiles:
        for harness in ("claude", "codex"):
            for dry in (False, True):
                yield dict(id=f"profile-{name}-{harness}-{dry}", command="install",
                           harness=harness, dry=dry, positive=True, profile_bytes=raw)
        if name == "mixed-literal":
            for harness in ("claude", "codex"):
                yield dict(id=f"collateral-{harness}", command="install", harness=harness,
                           dry=True, positive=True, existing=True, profile_bytes=raw)
    if os.name == "posix":
        for raw in (False, True):
            for state in ("different", "lexical-type", "physical-type"):
                yield dict(id=f"vault-pwd-{state}-{raw}", command="vault",
                           pwd_case=state, raw_link=raw, positive=True)
        for state in ("relative-ignored", "mismatch-ignored"):
            yield dict(id="vault-pwd-" + state, command="vault", pwd_case=state, positive=True)
    else:
        yield dict(id="vault-windows-pwd-ignored", command="vault",
                   pwd_case="mismatch-ignored", positive=True)
