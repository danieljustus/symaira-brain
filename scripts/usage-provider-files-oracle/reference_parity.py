"""Narrow #930 exception for the immutable, pre-fix Go oracle's output."""
import base64
import json


def reference_resolution_divergence(go, native):
    """Undo only reference-failure promotions, then compare all remaining bytes."""
    if {k: v for k, v in go.items() if k != 'stdout_b64'} != {
        k: v for k, v in native.items() if k != 'stdout_b64'
    }:
        return False
    expected = base64.b64decode(go['stdout_b64'])
    actual = base64.b64decode(native['stdout_b64'])
    original = actual
    try:
        report = json.loads(actual)
    except (ValueError, UnicodeError):
        # Human output changes only the status column, never the diagnostic.
        actual = b''.join(
            line.replace(b'\terror\tcredential resolution failed: ',
                         b'\tnot configured\tcredential resolution failed: ', 1)
            for line in actual.splitlines(keepends=True)
        )
    else:
        for provider in report.get('providers', []):
            detail = provider.get('auth_status', {}).get('detail', '')
            if (detail.startswith('credential resolution failed: ')
                    and provider.get('configured') is True
                    and provider.get('error') == detail):
                before = json.dumps(provider, ensure_ascii=False, separators=(',', ':')).encode()
                provider['configured'] = False
                del provider['error']
                after = json.dumps(provider, ensure_ascii=False, separators=(',', ':')).encode()
                if actual.count(before) != 1:
                    return False
                actual = actual.replace(before, after, 1)
    return original != actual and expected == actual
