"""Doctor fixtures; no historical Go production source or fixture is changed."""
from dataclasses import dataclass
import json

@dataclass(frozen=True)
class Case:
    name: str
    args: tuple[str, ...] = ("doctor", "--fix")
    version: bytes | None = None
    provenance: bytes | str | None = None
    config: str | None = None
    project_config: str | None = None
    env: tuple[tuple[str, str], ...] = ()
    probe_exit: int = 0
    probe_wait: int = 0
    all_missing: bool = False
    nonexecutable: bool = False
    verifier: bool = False
    signal: str | None = None
    raw_home: bool = False
    home_mode: str | None = None
    home_fallback: bool = False


def cases():
    result = [Case("correct"), Case("missing-all", all_missing=True),
              Case("wrong-version", version=b'{"version":"0.0.0"}'),
              Case("failed-probe", version=b'{"version":"0.0.0"}', probe_exit=42,
                   provenance=b'{"source":"brain-source"}'),
              Case("timeout", probe_wait=10000, provenance=b'{"source":"brain-source"}'),
              Case("provenance-directory", version=b'{"version":"0.0.0"}', provenance="directory")]
    if __import__('os').name != 'nt':
        result.append(Case("nonexecutable", nonexecutable=True, provenance=b'{"source":"brain-source"}'))
        for signal in ("TERM", "INT", "KILL"):
            result.append(Case(f"signal-{signal}",signal=signal,provenance=b'{"source":"brain-source"}'))
    else:
        for code in (65536,0xc0000005):
            result.append(Case(f"large-probe-exit-{code}",probe_exit=code,provenance=b'{"source":"brain-source"}'))
    for mode in ("empty","unset"):
        result.append(Case("home-"+mode,all_missing=True,home_mode=mode))
        result.append(Case("home-before-config-"+mode,all_missing=True,home_mode=mode,config="[bad config"))
        if __import__('os').name=='nt':
            result.append(Case("userprofile-fallback-refused-"+mode,all_missing=True,home_mode=mode,home_fallback=True))
    if __import__('os').name!='nt':
        result.extend((
            Case("raw-home-correct",raw_home=True),
            Case("raw-home-source",raw_home=True,version=b'{"version":"0.0.0"}',provenance=b'{"source":"brain-source"}'),
            Case("raw-home-directory",raw_home=True,version=b'{"version":"0.0.0"}',provenance="directory"),
            Case("raw-home-force-source",raw_home=True,args=("doctor","--fix","--force-release"),version=b'{"version":"0.0.0"}',provenance=b'{"source":"brain-source"}',verifier=True),
            Case("raw-home-force-directory",raw_home=True,args=("doctor","--fix","--force-release"),version=b'{"version":"0.0.0"}',provenance="directory",verifier=True),
            Case("raw-home-force-corrupt",raw_home=True,args=("doctor","--fix","--force-release"),version=b'{"version":"0.0.0"}',provenance=b'{bad',verifier=True)))
    payloads = [b'', b'{bad', b'null', b'{}', b'[]', b'1', b'true', b'"str"',
                b'{"source":"brain-source"}', b'{"SOURCE":"brain-source","receiver_commit":"fixture commit=123"}',
                '{"ſOURCE":"brain-source","RECEIVER_COMMIT":"unicode"}'.encode(),
                b'{"source":"brain-source","source":null}',
                b'{"source":"release","SOURCE":"brain-source"}',
                b'{"source":"brain-source","source":"release"}',
                b'{"source":1}', b'{"source":false}', b'{"source":[]}', b'{"source":{}}',
                b'{"source":"brain-source","version":42,"version":"ok"}',
                b'{"source":"brain-source","built_at":42}',
                b'{"source":"brain-source","receiver_commit":"\\ud800"}',
                b'{"source":"brain-source","receiver_commit":"bad\xff\xe2\x82"}',
                b'{"source":"brain-source","unknown":1e1000}',
                b'{"source":"brain-source","unknown":' + b'['*200+b'0'+b']'*200+b'}',
                b'{"source":"brain-source","built_at":"2026-10-03T12:00:00\\u005a"}',
                b'{"source":"brain-source","built_at":"\\ud800"}',
                b'{"source":"brain-source","built_at":"bad\xff\xe2\x82"}',
                b'{"source":"brain-source",}', b'{"source" "x"}',
                b'{"source":"\\x"}', b'{"source":"\\u0x00"}',
                b'{"source":tru}', b'{"source":fa}', b'{"source":nulx}',
                b'{"source":-x}', b'{"source":1.}', b'{"source":1e+}',
                b'{"source":01}', b'{"source":[1,]}', b'{"source":[1}}',
                b'{"source":"x"}[]', b'{"source":"bad\x01"}',
                b'{"version":1,"built_at":"bad"}',
                b'{"built_at":"bad","version":1}',
                b'-',b'1.',b'1e',b'1e+',b'"\\',b'"\\u0',b'nu',b'"abc',b'{',b'[1,']
    for timestamp in ("0000-01-01T00:00:00Z", "2024-02-29T00:00:00Z", "2026-02-29T00:00:00Z",
                      "2026-00-01T00:00:00Z", "2026-13-01T00:00:00Z", "2026-01-00T00:00:00Z",
                      "2026-01-32T00:00:00Z", "2026-10-03T2:00:00Z", "2026-10-03T12:00:00,123Z",
                      "2026-10-03T12:00:00.123456789123456789Z", "2026-10-03T12:00:00+24:00",
                      "2026-10-03T12:00:00+01:60", "2026-10-03T12:00:00+25:00",
                      "2026-10-03T12:00:00+01:61", "2026-10-03T24:00:00Z", "2026-10-03T12:60:00Z",
                      "2026-10-03T12:00:60Z", "2026-10-03T12:0:00Z", "2026-10-03T12:00:0Z",
                      "2026-10-03T12:00:00.Z", "2026-10-03t12:00:00z", "bad", "",
                      "2026-10-03T12:00:00Ztail"):
        payloads.append(json.dumps({"source":"brain-source","built_at":timestamp}).encode())
    for index, payload in enumerate(payloads):
        result.append(Case(f"provenance-{index}", version=b'{"version":"0.0.0"}', provenance=payload))
        result.append(Case(f"force-provenance-{index}", args=("doctor","--fix","--force-release"),
                           version=b'{"version":"0.0.0"}', provenance=payload))
    for index, payload in enumerate((
        b'', b'{bad', b'null', b'{}', b'[]', b'1', b'true', b'"str"',
        b'{"VERSION":"0.12.2"}', b'{"version":"0.12.2","version":null}',
        b'{"version":"0.0.0","VERSION":"0.12.2"}',
        b'{"version":1,"version":"0.12.2"}', b'{"version":[]}', b'{"version":{}}',
        b'{"version":"\\ud800"}', b'{"version":"bad\xff\xe2\x82"}',
        b'{"version":"0.12.2","unknown":1e1000}',
        b'{"version":"0.12.2","unknown":' + b'['*200+b'0'+b']'*200+b'}',
        b'{"version":"0.12.2","version":"0.0.0"}',
        b'{"version":"0.12.2"}x', b'{"version":false}',
        b'-',b'1.',b'1e',b'1e+',b'"\\',b'"\\u0',b'nu',b'"abc',b'{',b'[1,',
    )):
        result.append(Case(f"version-{index}",version=payload,provenance=b'{"source":"brain-source"}'))
    for flag in ("fix", "force-release", "json"):
        for value in ("1","t","T","TRUE","True","true","0","f","F","FALSE","False","false","","yes"):
            args = ["doctor","--fix"]
            if flag == "fix" and value in ("0","f","F","FALSE","False","false"):
                # Normal doctor probes MCP handshakes; already covered by its
                # native suite, rather than pretending they are repair cases.
                continue
            args.append(f"--{flag}={value}")
            result.append(Case(f"flag-{flag}-{value}", args=tuple(args)))
    for index, text in enumerate(('[modules]\nbrowse = true', '[modules]\nbrowse = false',
                                  '[modules]\nbrowse = "TRUE"', '[modules]\nbrowse = 0',
                                  '[modules]\nbrowse = ""', 'modules = 1')):
        result.append(Case(f"config-{index}",config=text))
    for value in ("true","TRUE","1","false","FALSE","0",""):
        result.append(Case(f"env-browse-{value}",env=(("SYMBRAIN_MODULES_BROWSE",value),)))
    result.append(Case("nonzero-config-merge",config='[modules]\nbrowse = true',project_config='[modules]\nbrowse = false'))
    result.extend((Case("force-false",args=("doctor","--fix","--force-release=false"), version=b'{"version":"0.0.0"}',provenance=b'{"source":"brain-source"}'),
                   Case("terminator",args=("doctor","--fix","--","--force-release")),
                   Case("positional",args=("doctor","--fix","positional","--force-release")),
                   Case("legacy-single-dash",args=("doctor","-fix","-force-release=false"))))
    for name,args in (
        ("excess-dash-fix",("doctor","----fix")),
        ("excess-dash-json",("doctor","--fix","----json")),
        ("excess-dash-force",("doctor","--fix","----force-release")),
        ("bad-flag-syntax",("doctor","--fix","--=bad")),
        ("valid-triple-force",("doctor","--fix","---force-release"))):
        result.append(Case(name,args=args,version=b'{"version":"0.0.0"}',
                           provenance=b'{"source":"brain-source"}',verifier=True))
    if __import__('os').name!='nt':
        raw=__import__('os').fsdecode(b'bad\xff\xe2\x82')
        for flag in ("fix","force-release","json"):
            result.append(Case("raw-bool-"+flag,args=("doctor","--fix","--"+flag+"="+raw)))
        result.append(Case("raw-unknown-flag",args=("doctor","--fix","--"+raw)))
    result.extend((
        Case("verified-repair-missing", all_missing=True, verifier=True),
        Case("verified-repair-mismatch", version=b'{"version":"0.0.0"}', verifier=True),
        Case("verified-force-source",args=("doctor","--fix","--force-release"),version=b'{"version":"0.0.0"}',
             provenance=b'{"source":"brain-source","receiver_commit":"kept until explicit force"}',verifier=True),
        Case("verified-force-corrupt",args=("doctor","--fix","--force-release"),version=b'{"version":"0.0.0"}',
             provenance=b'{bad',verifier=True)))
    return result
