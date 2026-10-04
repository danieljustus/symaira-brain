"""Additive four-finding cases; all previous81/69 case inputs stay exact."""
import startup_correction_cases as previous


def cases():
    yield from previous.cases()
    for scope in ('global', 'project'):
        for name, prefix in [('fffe', b'\xff\xfe'), ('feff', b'\xfe\xff'), ('utf8', b'\xef\xbb\xbf')]:
            document = prefix + b'[database]\npath="owned-selected.db"\n[jwt]\nsecret="owned-synthetic-key"\n'
            yield dict(id='final-memory-marker-'+scope+'-'+name, kind='literal',
                **{('config' if scope == 'global' else 'project'): document},
                resolved=b'owned-synthetic-key', selected_database='owned-selected.db',
                secret=b'owned-readonly-unused-file-key\n',
                encrypted_plaintext=b'[{"secret":"owned-active-fallback","expires_at":"2099-01-01T00:00:00Z"}]')
    for name, expiry in [
        ('hour-range-before-minute-error', b'2099-01-01T00:00:00+25:xx'),
        ('hour-range-before-partial-minute', b'2099-01-01T00:00:00+25:x1'),
        ('hour-range-before-sign-minute-error', b'2099-01-01T00:00:00?25:xx'),
        ('hour24-still-minute-error', b'2099-01-01T00:00:00+24:xx'),
        ('invalid-hour-before-minute-range', b'2099-01-01T00:00:00+x5:61'),
    ]:
        yield dict(id='final-auth-time-'+name, kind='file', secret=b'owned-file-key\n',
            encrypted_plaintext=b'[{"secret":"owned-fallback","expires_at":"'+expiry+b'"}]')
    yield dict(id='final-auth-json-wide-100000', kind='file', secret=b'owned-file-key\n',
        encrypted_plaintext=b'['+b','.join([b'{}']*100_000)+b']')
    yield dict(id='final-secret-nested-unicode-blocker', kind='failure', unicode_secret_blocker=True,
        config='[jwt]\nsecret_path="blocked-ä/new/jwt.secret"\n'.encode('utf8'))


def fixture(root, case):
    paths = previous.fixture(root, case)
    if case.get('selected_database'):
        paths['database'] = str(root/'project'/case['selected_database'])
    if case.get('unicode_secret_blocker'):
        (root/'project/blocked-ä').write_bytes(b'owned immutable Unicode blocker')
        paths['secret'] = str(root/'project/blocked-ä/new/jwt.secret')
        paths['rotation'] = str(root/'project/blocked-ä/new/jwt.secrets')
    return paths
