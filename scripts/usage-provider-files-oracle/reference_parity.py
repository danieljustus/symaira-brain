"""Narrow #930 exception for the immutable, pre-fix Go oracle's output."""
import base64
import json


def normalize_reference_resolution(native):
    """Undo only reference-failure promotions, preserving all other bytes."""
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
                    return None
                actual = actual.replace(before, after, 1)
    if original == actual:
        return None
    return {**native, 'stdout_b64': base64.b64encode(actual).decode()}


def reference_resolution_divergence(go, native):
    """Compare all remaining bytes after the narrow promotion reversal."""
    normalized = normalize_reference_resolution(native)
    return normalized is not None and go == normalized
