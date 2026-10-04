#!/usr/bin/env python3
"""Pinned-source projection of prepared redirects, never process/wire proof."""
import base64
import hashlib
import json
from pathlib import Path
from urllib.parse import unquote_to_bytes, urljoin, urlsplit


def identity(url):
    authority = urlsplit(url).netloc
    host = authority.rsplit('@', 1)[-1]
    hostname = host.split(']', 1)[0][1:] if host.startswith('[') else host.split(':', 1)[0]
    return host, hostname


def basic(url):
    authority = urlsplit(url).netloc
    if '@' not in authority:
        return None
    raw = authority.rsplit('@', 1)[0]
    user, _, password = raw.partition(':')
    value = unquote_to_bytes(user) + b':' + unquote_to_bytes(password)
    return b'Basic ' + base64.b64encode(value)


def project(case, mutation=None):
    original = case['remote_template'].format(port=31234, second_port=31235)
    current = original
    token = bytes.fromhex(case['token_hex'])
    explicit = b'Bearer ' + token if token else None
    if mutation == 'cache-url-basic' and explicit is None:
        explicit = basic(original)
    stripped = False
    observed = []
    for location in [None] + case['location_templates']:
        if location is not None:
            current = urljoin(current, location.format(port=31234, second_port=31235))
            ihost, initial = identity(original)
            dhost, destination = identity(current)
            if mutation == 'ascii-case-fold':
                initial, destination = initial.lower(), destination.lower()
            if mutation == 'restore-after-return' and ihost == dhost:
                stripped = False
            if not stripped and ihost != dhost:
                allowed = (destination == initial or
                           (':' not in destination and '%' not in destination and
                            destination.endswith('.' + initial)))
                if not allowed:
                    stripped = True
        auth = explicit if not stripped and explicit is not None else basic(current)
        if mutation == 'cache-url-basic' and explicit is not None and not stripped:
            auth = explicit
        observed.append(None if auth is None else auth.hex())
    return observed


def main():
    root = Path(__file__).resolve().parents[2]
    path = root / 'scripts/memory-sync-oracle/redirect-correction-cases.json'
    raw = path.read_bytes()
    plan = json.loads(raw)
    assert len(plan['cases']) == 64
    records = []
    for case in plan['cases']:
        result = project(case)
        assert result == case['source_projected_authorization_value_hex'], case['id']
        records.append(dict(id=case['id'],authorization_hex=result,source_projection_matches=True))
    controls = []
    for definition in plan['controls']:
        mutation = definition['id'].removeprefix('REDIRECT-CONTROL-')
        selected = [c for c in plan['cases'] if c['id'] in definition['case_ids']]
        assert len(selected) == 4
        differences = [dict(id=c['id'],mutated_projection=project(c, mutation),
                            expected_projection=c['source_projected_authorization_value_hex'])
                       for c in selected if project(c, mutation) != c['source_projected_authorization_value_hex']]
        assert len(differences) == 4
        controls.append(dict(id=definition['id'],projection_differences=differences,
                             actual_process_control_executed=False))
    print(json.dumps(dict(status='SDK_SOURCE_PROJECTION_ONLY_NOT_ACCEPTANCE',
                          plan_sha256=hashlib.sha256(raw).hexdigest(),records=records,
                          mutation_projections=controls,Go_or_Rust_executable_runs=0,
                          compiled_tests=0,HTTP_requests=0,ports=0), indent=2))


if __name__ == '__main__':
    main()
