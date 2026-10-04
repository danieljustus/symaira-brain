"""Additive six-finding cases; the original41 case plans remain byte-identical."""
import os
from pathlib import Path
import startup_cases as original


def cases():
    yield from original.cases()
    for name, expiry in [
        ('lower-t', b'2099-01-01t00:00:00Z'),
        ('lower-z', b'2099-01-01T00:00:00z'),
        ('space', b'2099-01-01 00:00:00Z'),
        ('leap-second', b'2099-01-01T00:00:60Z'),
        ('json-escaped-year', br'\u0032099-01-01T00:00:00Z'),
        ('invalid-raw-byte', b'2099-01-01T00:00:00\xff'),
        ('one-hour-comma', b'2099-01-01T0:00:00,123456789123Z'),
        ('zone-hour-minute-boundary', b'2099-01-01T00:00:00+24:60'),
        ('zone-minute-normalized', b'2099-01-01T00:00:00+00:60'),
        ('zone-range-precedence', b'2099-01-01T00:00:00?25:61'),
        ('day-after-extra', b'2099-02-30T00:00:00Zextra'),
    ]:
        yield dict(id='corrected-auth-time-'+name, kind='file', secret=b'owned-file-key\n',
                   encrypted_plaintext=b'[{"secret":"owned-fallback","expires_at":"'+expiry+b'"}]')
    for name, raw in [
        ('number-secret', b'[{"secret":7}]'),
        ('object-secret', b'[{"secret":{}}]'),
        ('numeric-time', b'[{"expires_at":7}]'),
        ('saved-type-before-time', b'[{"secret":7,"expires_at":false}]'),
        ('time-before-saved-type', b'[{"expires_at":false,"secret":7}]'),
        ('syntax-before-types', b'[{"secret":7,"expires_at":false,}]'),
        ('wrong-root', b'{}'), ('wrong-record', b'[7]'),
        ('null-fields', b'[{"secret":"owned-fallback","secret":null,"expires_at":null}]'),
        ('duplicate-time', b'[{"expires_at":"2099-01-01T0:00:00Z","Expires_At":"2099-01-01T00:00:00,1Z"}]'),
        ('unknown-overflow', b'[{"unknown":1e400}]'),
        ('syntax-escape', br'[{"unknown":"\q"}]'),
        ('syntax-number', b'[{"unknown":1e+}]'),
        ('depth-admitted', b'[{"unknown":'+b'['*9998+b']'*9998+b'}]'),
        ('depth-rejected', b'[{"unknown":'+b'['*9999+b']'*9999+b'}]'),
    ]:
        yield dict(id='corrected-auth-json-'+name, kind='file', secret=b'owned-file-key\n', encrypted_plaintext=raw)
    yield dict(id='corrected-secret-nested-blocker', kind='failure', nested_secret_blocker=True,
               config=b'[jwt]\nsecret_path="blocked/new/jwt.secret"\n')
    yield dict(id='corrected-database-migration-phase', kind='failure', corrupt_database=True)
    for name, response, resolved, env in [
        ('success-key-retention', b' \xc2\xa0owned-provider-key\xe2\x80\x83\n', b'owned-provider-key', {}),
        ('raw-key-retention', b' \xe2\x80\x83owned-\xff\xe2\x82\t', b'owned-\xff\xe2\x82', {}),
        ('empty-fallback-retention', b' \t\n', b'owned-vault-fallback', {'JWT_SECRET_KEY':'owned-vault-fallback'}),
    ]:
        yield dict(id='corrected-provider-'+name, kind='literal', env=env,
                   config=b'[jwt]\nsecret="symvault://owned/path"\n',
                   secret=b'owned-readonly-unused-file-key\n', resolved=resolved,
                   provider=dict(stdout=response,stderr=b'',code=0),
                   encrypted_plaintext=b'[{"secret":"owned-active-fallback","expires_at":"2099-01-01T00:00:00Z"}]')
    if os.name != 'nt':
        yield dict(id='corrected-secret-nested-raw-blocker', kind='failure', raw_secret_blocker=True,
                   config=b'[jwt]\nsecret_path="blocked/new/jwt.secret"\n')
        for signal in ['TERM', 'PIPE', 'KILL', 'QUIT']:
            for fallback in [False, True]:
                env = {'JWT_SECRET_KEY':'owned-vault-fallback'} if fallback else {}
                case=dict(id='corrected-provider-signal-'+signal+'-'+str(fallback), kind='literal', env=env,
                           config=b'[jwt]\nsecret="symvault://owned/path"\n',
                           provider=dict(stdout=b'', stderr=b' owned status tail \xff\xe2\x82\n', code=0, signal=signal),
                           resolved=b'owned-vault-fallback')
                if fallback:
                    case.update(secret=b'owned-readonly-unused-file-key\n',
                                encrypted_plaintext=b'[{"secret":"owned-active-fallback","expires_at":"2099-01-01T00:00:00Z"}]')
                yield case
    else:
        for code in [65536, -1073741819]:
            yield dict(id='corrected-provider-windows-exit-'+str(code), kind='literal',
                       config=b'[jwt]\nsecret="symvault://owned/path"\n',
                       provider=dict(stdout=b'',stderr=b'owned status tail',code=code))


def fixture(root, case):
    paths = original.fixture(root, case)
    if case.get('nested_secret_blocker'):
        (root/'project/blocked').write_bytes(b'owned immutable blocker')
        paths['secret']=str(root/'project/blocked/new/jwt.secret')
        paths['rotation']=str(root/'project/blocked/new/jwt.secrets')
    if case.get('raw_secret_blocker'):
        block = root/'project'/os.fsdecode(b'blocked-\xff\xe2\x82')
        from startup_fixture_admission import create_raw_blocker
        create_raw_blocker(block)
        config = root/'config/symmemory/config.toml'
        config.write_bytes(b'[jwt]\nsecret_path="unused"\n')
        case.setdefault('env', {})['SYMMEMORY_JWT_SECRET_PATH'] = str(block/'new/jwt.secret')
        paths['secret'] = str(block/'new/jwt.secret')
        paths['rotation'] = str(block/'new/jwt.secrets')
    if case.get('corrupt_database'):
        import sqlite3
        path = Path(paths['database']); path.parent.mkdir(parents=True, exist_ok=True)
        connection = sqlite3.connect(path)
        connection.executescript('CREATE TABLE memories(id TEXT PRIMARY KEY); CREATE TABLE rules(id TEXT PRIMARY KEY); CREATE TABLE schema_migrations(version TEXT PRIMARY KEY);')
        connection.close()
    return paths
