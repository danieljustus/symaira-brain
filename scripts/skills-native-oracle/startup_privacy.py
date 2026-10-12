"""Native Windows owner/DACL observations, not POSIX-mode approximations."""
import hashlib
import json
import ntpath
import os
from pathlib import Path
import subprocess
from startup_descriptor import descriptor_fields, strict_hex

# Native .NET ACL APIs return numeric ACE kinds, flags, masks and SIDs; no
# localized icacls text or account-name matching participates in the verdict.
ACL_SCRIPT = r'''
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$user = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$rows = @()
foreach ($path in (ConvertFrom-Json $env:SYMBRAIN_STARTUP_ACL_PATHS)) {
    $acl = Microsoft.PowerShell.Security\Get-Acl -LiteralPath $path
    $binary = $acl.GetSecurityDescriptorBinaryForm()
    $raw = [System.Security.AccessControl.RawSecurityDescriptor]::new($binary, 0)
    $aces = @()
    if ($null -ne $raw.DiscretionaryAcl) {
        foreach ($ace in $raw.DiscretionaryAcl) {
            $aces += [ordered]@{
                type = [int]$ace.AceType
                flags = [int]$ace.AceFlags
                mask = [long]$ace.AccessMask
                sid = $ace.SecurityIdentifier.Value
            }
        }
    }
    $rows += [ordered]@{
        path = $path
        owner_sid = $raw.Owner.Value
        user_sid = $user
        control_flags = [int]$raw.ControlFlags
        dacl_present = ($null -ne $raw.DiscretionaryAcl)
        aces = @($aces)
        descriptor_hex = ([BitConverter]::ToString($binary)).Replace('-', '').ToLowerInvariant()
    }
}
ConvertTo-Json -Depth 6 -Compress -InputObject @($rows)
'''


def capture(record, root, paths):
    """Retain native metadata before policy validation, including probe failure."""
    if os.name != "nt":
        return
    tool = Path(os.environ["SystemRoot"]) / "System32/WindowsPowerShell/v1.0/powershell.exe"
    observation = {"mechanism": "windows-native-owner-dacl", "paths": sorted(paths), "root": str(root),
                   "tool": str(tool), "tool_sha256": hashlib.sha256(tool.read_bytes()).hexdigest()}
    record["startup_privacy"] = observation
    env = {key: os.environ[key] for key in ("SystemRoot", "windir", "ComSpec", "SystemDrive") if key in os.environ}
    env.update(HOME=str(root / "home"), USERPROFILE=str(root / "home"),
               TEMP=str(root / "tmp"), TMP=str(root / "tmp"),
               PATH=str(tool.parent), PSModulePath=str(tool.parent / "Modules"))
    env["SYMBRAIN_STARTUP_ACL_PATHS"] = json.dumps([str(root / path) for path in sorted(paths)])
    try:
        result = subprocess.run([str(tool), "-NoProfile", "-NonInteractive", "-Command", ACL_SCRIPT],
                                env=env, stdin=subprocess.DEVNULL, capture_output=True,
                                timeout=20, check=False)
    except subprocess.TimeoutExpired as error:
        observation.update(timed_out=True, stdout_hex=(error.stdout or b"").hex(),
                           stderr_hex=(error.stderr or b"").hex())
        raise
    observation.update(exit=result.returncode, stdout_hex=result.stdout.hex(), stderr_hex=result.stderr.hex())
    assert result.returncode == 0 and not result.stderr, "native startup ACL observation failed"
    rows = json.loads(result.stdout)
    assert isinstance(rows, list) and len(rows) == len(paths), "incomplete native ACL inventory"
    assert [row["path"] for row in rows] == [str(root / path) for path in sorted(paths)]
    observation["entries"] = dict(zip(sorted(paths), rows))


def validate_windows(row):
    """Fail closed on broad grants, null DACLs and unreviewed ACE types."""
    fields = descriptor_fields(row["descriptor_hex"])
    claims = {key: row[key] for key in fields}
    assert json.dumps(claims, sort_keys=True) == json.dumps(fields, sort_keys=True), "descriptor/ACL claims mismatch"
    assert row["owner_sid"] == row["user_sid"], "startup owner is not the current user"
    assert row["dacl_present"] is True and row["control_flags"] & 4, "missing/null startup DACL"
    trusted = {row["user_sid"], "S-1-5-18", "S-1-5-32-544"}  # owner, SYSTEM, Administrators
    owner_read = False
    assert isinstance(row["aces"], list) and row["aces"], "empty startup DACL"
    for ace in row["aces"]:
        assert type(ace["type"]) is int and ace["type"] in (0, 1), "unreviewed startup ACE type"
        assert type(ace["flags"]) is int and 0 <= ace["flags"] <= 31, "invalid startup ACE flags"
        assert type(ace["mask"]) is int and ace["mask"] != 0, "empty/invalid startup ACE mask"
        assert isinstance(ace["sid"], str) and ace["sid"].startswith("S-1-"), "invalid startup ACE SID"
        if ace["type"] == 1 and not ace["flags"] & 8:
            # Conservatively reject applicable read denials for the owner or
            # a broad group. An owner ALLOW must not conceal an earlier DENY.
            assert not (ace["mask"] & (1 | 0x10000000 | 0x80000000)), "startup DACL denies read/list access"
        if ace["type"] == 0:
            # Also reject broad inherit-only grants: a private directory must
            # not authorize disclosure by descendants created under it.
            assert ace["sid"] in trusted, "startup DACL grants a non-private principal"
            if ace["sid"] == row["user_sid"] and not ace["flags"] & 8:
                owner_read |= bool(ace["mask"] & (1 | 0x10000000 | 0x80000000))
    assert owner_read, "startup DACL does not grant owner read/list access"


def private(record, path):
    if os.name == "nt":
        proof = record.get("startup_privacy", {})
        assert proof.get("mechanism") == "windows-native-owner-dacl", "native ACL proof required"
        assert type(proof.get("exit")) is int and proof["exit"] == 0 and proof.get("stderr_hex") == "", "ACL probe did not complete"
        rows = json.loads(strict_hex(proof["stdout_hex"]))
        paths = proof["paths"]
        assert isinstance(paths, list) and paths == sorted(set(paths)), "invalid ACL path inventory"
        assert isinstance(rows, list) and len(rows) == len(paths), "incomplete raw ACL inventory"
        assert set(proof["entries"]) == set(paths), "ACL entry inventory mismatch"
        assert isinstance(proof["root"], str) and proof["root"], "ACL root missing"
        assert ntpath.normpath(record["cwd"]) == ntpath.join(proof["root"], "project"), "ACL root/process binding mismatch"
        for relative, row in zip(paths, rows):
            assert row["path"] == ntpath.join(proof["root"], *relative.split("/")), "ACL path binding mismatch"
            assert json.dumps(row, sort_keys=True) == json.dumps(proof["entries"][relative], sort_keys=True), "raw ACL/entry mismatch"
            validate_windows(row)
        validate_windows(proof["entries"][path])
    else:
        assert record["filesystem"]["files"][path]["mode"] & 0o077 == 0, f"non-private startup path: {path}"
