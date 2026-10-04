"""Owned Memory startup fixtures. Every key is synthetic; no host secret source."""
from pathlib import Path
import os

PROFILE=b'''name="owned"
[servers.vault]
enabled=false
[servers.operate]
enabled=false
[servers.scope]
enabled=false
[servers.memory]
enabled=false
[servers.skills]
enabled=false
[servers.usage]
enabled=false
[audit]
enabled=false
'''

def cases():
    import process as original
    for case in original.cases():
        if case['id']in('plain-pointer-false','unknown-scalar-struct','bool-aliases'):
            yield dict(id='original-'+case['id'],kind='generated',original_case=case)
    yield dict(id='generated-default',kind='generated')
    yield dict(id='generated-repeat',kind='generated')
    yield dict(id='environment-literal',kind='literal',env={'JWT_SECRET_KEY':'owned-env-key'})
    yield dict(id='configured-literal-before-env',kind='literal',config=b'[jwt]\nsecret="owned-config-key"\n',env={'JWT_SECRET_KEY':'owned-env-key'})
    yield dict(id='environment-reference',kind='literal',config=b'[jwt]\nsecret="env://OWNED_JWT"\n',env={'OWNED_JWT':'owned-reference-key'})
    yield dict(id='missing-reference-before-env',kind='failure',config=b'[jwt]\nsecret="env://OWNED_ABSENT"\n',env={'JWT_SECRET_KEY':'owned-never-fallback'})
    yield dict(id='vault-failure-raw-fallback',kind='literal',config=b'[jwt]\nsecret="vault://owned/path"\n',env={'JWT_SECRET_KEY':'owned-vault-fallback'})
    for name,response,stderr,code in [('success',b' \xc2\xa0owned-provider-key\xe2\x80\x83\n',b'',0),('empty',b' \t\n',b'',0),('failure',b'unused-output',b' owned failure \n',7)]:
        yield dict(id='vault-provider-'+name,kind='literal',config=b'[jwt]\nsecret="symvault://owned/path"\n',provider=dict(stdout=response,stderr=stderr,code=code),resolved=b'owned-provider-key')
    yield dict(id='vault-invalid-before-provider',kind='failure',config=b'[jwt]\nsecret="symvault://-owned"\n',provider=dict(stdout=b'never-read',stderr=b'',code=0),no_provider=True)
    yield dict(id='file-unicode-trim-readonly',kind='file',secret=b' \xe2\x80\x83owned-file-key\xc2\xa0\n')
    yield dict(id='empty-existing-file-preserves-mode',kind='generated',secret=b'')
    yield dict(id='custom-secret-relative',kind='generated',config=b'[jwt]\nsecret_path="custom-key.secret"\n')
    yield dict(id='custom-database-relative',kind='generated',config=b'[database]\npath="custom-db/default.db"\n')
    yield dict(id='project-secret-over-global',kind='literal',config=b'[jwt]\nsecret="global-key"\n',project=b'[jwt]\nsecret="project-key"\n')
    yield dict(id='environment-secret-over-project',kind='literal',project=b'[jwt]\nsecret="project-key"\n',env={'SYMMEMORY_JWT_SECRET':'environment-key'})
    yield dict(id='invalid-unrelated-config-defaults',kind='generated',config=b'[database]\npath="unused.db"\n[conflict]\nenabled=[]\n')
    yield dict(id='legacy-data-and-secret',kind='generated',legacy=True)
    yield dict(id='database-parent-file-first',kind='failure',blocked_db=True,env={'SYMMEMORY_JWT_SECRET':'env://OWNED_ABSENT'})
    yield dict(id='secret-parent-file-after-database',kind='failure',blocked_secret=True)
    yield dict(id='corrupt-short-rotation-warning',kind='file',secret=b'owned-file-key\n',rotation=b'bad')
    for name,rotation in [('empty-array',b'[]'),('nil',b'null'),('valid',b'[{"secret":"owned-fallback","expires_at":"2099-01-01T00:00:00.1234Z"}]'),('expired',b'[{"secret":"owned-old","expires_at":"2000-01-01T00:00:00Z"}]'),('mixed',b'[{"secret":"owned-old","expires_at":"2000-01-01T00:00:00Z"},{"Secret":"owned-active","Expires_At":"2099-01-01T00:00:00Z","unknown":1e400}]'),('surrogate',b'[{"secret":"\\ud800","expires_at":"2099-01-01T00:00:00Z"}]')]:
        yield dict(id='legacy-rotation-'+name,kind='file',secret=b'owned-file-key\n',rotation=rotation)
    for name,expiry in [('single-hour','2099-01-01T0:00:00Z'),('comma-fraction','2099-01-01T00:00:00,1234Z'),('long-fraction','2099-01-01T00:00:00.123456789123Z'),('zone-hour24','2099-01-01T00:00:00+24:00'),('zone-minute60','2099-01-01T00:00:00+00:60')]:
        yield dict(id='legacy-time-'+name,kind='file',secret=b'owned-file-key\n',rotation=('{"secret":"owned-fallback","expires_at":"'+expiry+'"}').join(['[',']']).encode())
    if os.name!='nt':
        yield dict(id='vault-raw-selector-and-output',kind='literal',env={'SYMMEMORY_JWT_SECRET':'symvault://owned-\udcff\udce2\udc82'},provider=dict(stdout=b' \xc2\xa0owned-\xff\xe2\x82\t',stderr=b'',code=0),resolved=b'owned-\xff\xe2\x82')
        yield dict(id='vault-raw-error',kind='failure',env={'SYMMEMORY_JWT_SECRET':'symvault://owned-\udcff'},provider=dict(stdout=b'',stderr=b' \xe2\x80\x83owned-\xff\xe2\x82\t',code=7))
        yield dict(id='raw-environment-key',kind='literal',env={'JWT_SECRET_KEY':'owned-\udcff\udce2\udc82'})
        yield dict(id='raw-referenced-key',kind='literal',config=b'[jwt]\nsecret="env://OWNED_JWT"\n',env={'OWNED_JWT':'owned-\udcff\udce2\udc82'})
        yield dict(id='file-raw-bytes-readonly',kind='file',secret=b' \xe2\x80\x83owned-\xff\xe2\x82\t')


def fixture(root, case):
    if 'original_case'in case:
        import process as original
        original.fixture(root,case['original_case'])
        return dict(secret=str(root/'config/symbrain/memory/jwt.secret'),rotation=str(root/'config/symbrain/memory/jwt.secrets'),database=str(root/'data/symbrain/memory/default.db'))
    for relative in ('home','project','config/symbrain','data','cache'):
        (root/relative).mkdir(parents=True)
    (root/'profile.toml').write_bytes(PROFILE)
    (root/'config/symbrain/config.toml').write_bytes(b'[audit]\nenabled=false\n')
    if 'config' in case:
        (root/'config/symmemory').mkdir()
        (root/'config/symmemory/config.toml').write_bytes(case['config'])
    if 'project' in case:
        (root/'project/.symmemory.toml').write_bytes(case['project'])
    config_dir=root/('config/symmemory' if 'config' in case or case.get('legacy') else 'config/symbrain/memory')
    data_dir=root/('data/symmemory' if case.get('legacy') else 'data/symbrain/memory')
    if case.get('legacy'):
        config_dir.mkdir(parents=True,exist_ok=True);data_dir.mkdir(parents=True)
    if 'secret' in case:
        config_dir.mkdir(parents=True,exist_ok=True)
        (config_dir/'jwt.secret').write_bytes(case['secret']);(config_dir/'jwt.secret').chmod(0o640)
    if 'rotation' in case:
        (config_dir/'jwt.secrets').write_bytes(case['rotation']);(config_dir/'jwt.secrets').chmod(0o640)
    if case.get('blocked_db'):
        (root/'data/symbrain').write_bytes(b'owned unchanged blocker')
    if case.get('blocked_secret'):
        (root/'config/symbrain/memory').write_bytes(b'owned unchanged blocker')
    secret=root/'project/custom-key.secret'if case['id']=='custom-secret-relative'else config_dir/'jwt.secret'
    database=root/'project/custom-db/default.db'if case['id']=='custom-database-relative'else data_dir/'default.db'
    return dict(secret=str(secret),rotation=str(secret.with_suffix('.secrets')),database=str(database))
