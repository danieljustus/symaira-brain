"""Probe the owned filename domain before launching either product.

Only Darwin's observed EILSEQ92 for invalid UTF-8 is an unavailable input.
Other fixture errors stay fatal. No unavailable observation is product parity.
"""
import hashlib
import os
from pathlib import Path
import platform
import sys
import tempfile


def invalid_utf8(component):
    try:
        component.decode('utf-8')
        return False
    except UnicodeDecodeError:
        return True


def classify(component, error, system):
    if system == 'darwin' and invalid_utf8(component) and error.errno == 92:
        return 'unavailable-darwin-EILSEQ92'
    raise error


def probe(component, parent, journal=None):
    """Attempt exact bytes, retain the full exception and prove cleanup."""
    parent = Path(parent)
    record = dict(component_hex=component.hex(), component_sha256=hashlib.sha256(component).hexdigest(),
                  system=sys.platform, platform=platform.platform(), os_name=os.name,
                  python=sys.version, kernel=platform.uname()._asdict(),
                  invalid_utf8=invalid_utf8(component), product_children_started=0)
    error = None
    created = False
    with tempfile.TemporaryDirectory(prefix='owned-kernel-admission-', dir=parent) as owned:
        root = Path(owned)
        path = root/os.fsdecode(component)
        record.update(owned_root_bytes_hex=os.fsencode(root).hex(),
                      attempted_path_bytes_hex=os.fsencode(path).hex(), before_entries=[])
        try:
            path.mkdir()
            created = True
            record.update(admitted=True, disposition='admitted',
                          created_name_bytes_hex=[os.fsencode(p.name).hex() for p in root.iterdir()])
            assert record['created_name_bytes_hex'] == [component.hex()], 'kernel changed filename bytes'
        except OSError as caught:
            error = caught
            record.update(admitted=False, exception_type=type(caught).__name__,
                          exception_repr=repr(caught), exception_str=str(caught),
                          errno=caught.errno, winerror=getattr(caught, 'winerror', None),
                          filename_bytes_hex=None if caught.filename is None else os.fsencode(caught.filename).hex(),
                          filename2_bytes_hex=None if caught.filename2 is None else os.fsencode(caught.filename2).hex())
        finally:
            record['entries_before_cleanup'] = [os.fsencode(p.name).hex() for p in root.iterdir()]
            # An unavailable byte name can also make lstat fail with EILSEQ.
            # Only remove an entry that this probe actually created.
            if created:
                path.rmdir()
            record['entries_after_cleanup'] = [os.fsencode(p.name).hex() for p in root.iterdir()]
            assert not record['entries_after_cleanup'], 'owned admission probe leaked entries'
    record['owned_root_removed'] = not root.exists()
    assert record['owned_root_removed'], 'owned admission probe root remains'
    if error is not None:
        try:
            record['disposition'] = classify(component, error, sys.platform)
        except OSError:
            record['disposition'] = 'fatal-fixture-error'
            if journal:
                journal.event('kernel-admission', observation=record)
            raise
    if journal:
        journal.event('kernel-admission', observation=record)
    return record


def unavailable(case_id, admission):
    assert not admission['admitted']
    return dict(id=case_id, disposition='unexecuted-kernel-unavailable',
                admission=admission, product_children_started=0,
                go=None, native=None, parity=False)


def accounting(requested, observations, unavailable_rows, control_ids, controls, unavailable_controls):
    """Fail closed if a requested case/control disappears or is double-counted."""
    def verify(expected, completed, excluded, control=False):
        assert len(expected) == len(set(expected)), 'duplicate requested IDs'
        done = [row['id'] for row in completed]
        missing = [row['id'] for row in excluded]
        assert len(done + missing) == len(set(done + missing)), 'duplicate or overlapping observations'
        assert set(done + missing) == set(expected), 'unrecorded exclusion or unexpected observation'
        for row in excluded:
            proof = row['admission']
            assert row['product_children_started'] == 0 and row['parity'] is False
            assert row['disposition'] == 'unexecuted-kernel-unavailable'
            assert proof['system'] == 'darwin' and proof['errno'] == 92 and proof['invalid_utf8']
            assert invalid_utf8(bytes.fromhex(proof['component_hex']))
            assert proof['disposition'] == 'unavailable-darwin-EILSEQ92' and not proof['admitted']
            assert proof['owned_root_removed'] and proof['entries_after_cleanup'] == []
        if control:
            assert all(row['rejected'] is True for row in completed), 'admitted control was not rejected'
    verify(requested, observations, unavailable_rows)
    verify(control_ids, controls, unavailable_controls, True)
    return dict(original_requested_domain_complete=not (unavailable_rows or unavailable_controls),
                platform_admitted_complete=not any(row.get('disposition', '').startswith('failed') for row in observations), requested_case_ids=requested,
                executed_case_ids=[row['id'] for row in observations],
                unavailable_case_ids=[row['id'] for row in unavailable_rows],
                requested_control_ids=control_ids, executed_control_ids=[row['id'] for row in controls],
                unavailable_control_ids=[row['id'] for row in unavailable_controls])


def accounting_control(requested, observations, excluded, control_ids, controls, excluded_controls):
    """Corrupt a genuine ledger; no substituted product rejection is claimed."""
    if observations:
        corrupted, missing = observations[:-1], excluded
    else:
        assert excluded, 'accounting control needs a requested case'
        corrupted, missing = observations, excluded[:-1]
    try:
        accounting(requested, corrupted, missing, control_ids, controls, excluded_controls)
    except AssertionError as error:
        assert 'unrecorded exclusion' in str(error), 'incidental accounting failure'
        return dict(id='drop-accounting-row', rejected=True, diagnostic=str(error),
                    mutation='remove one complete requested-case observation', product_control=False)
    raise AssertionError('accepted unrecorded exclusion')
