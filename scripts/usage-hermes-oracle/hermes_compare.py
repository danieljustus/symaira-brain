"""Narrow, shared allowances for intentional Hermes oracle divergences."""
import base64
import json


def hermes_login_divergence(go, native, mode, file_present):
    """#933: missing/expired Hermes files have a specific login diagnostic.

    The immutable Go evidence remains unchanged. Admit only this auth-state
    replacement; all other bytes/fields, exit status and stderr must agree.
    """
    if not file_present or go['exit'] != native['exit'] or go['stderr_b64'] != native['stderr_b64']:
        return False
    old = 'No Nous Portal credentials found — sign in with the Hermes CLI'
    new = 'Hermes Nous login missing/expired — sign in again with the Hermes CLI'
    try:
        expected = base64.b64decode(go['stdout_b64']).decode()
        actual = base64.b64decode(native['stdout_b64']).decode()
        if mode == 'table':
            return old in expected and expected.replace(old, new) == actual
        expected = json.loads(expected)
        actual = json.loads(actual)
        providers = [p for p in expected['providers'] if p['id'] == 'nous']
        if len(providers) != 1 or providers[0]['configured'] or providers[0]['auth_status'] != {'status': 'missing', 'detail': old}:
            return False
        providers[0]['auth_status'] = {'status': 'expired', 'detail': new, 'source': 'file'}
        return expected == actual
    except (ValueError, UnicodeError, KeyError, TypeError):
        return False


def filesystem_matches(go, native, file_present):
    """Keep timeout parity strict outside the separately asserted FIFO case."""
    if go['timed_out'] or native['timed_out']:
        return False
    return go == native or hermes_login_divergence(go, native, 'json', file_present)
