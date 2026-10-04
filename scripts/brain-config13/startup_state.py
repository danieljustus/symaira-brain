"""Memory-owned observation. Raw bytes retained; no global byte/SQL adapter."""
from pathlib import Path
import base64,gzip,hashlib,json,os,shutil,sqlite3,struct,datetime,sys

GO_SPACE='\t\n\v\f\r \u0085\u00a0\u1680\u2000\u2001\u2002\u2003\u2004\u2005\u2006\u2007\u2008\u2009\u200a\u2028\u2029\u202f\u205f\u3000'
sha=lambda b:hashlib.sha256(b).hexdigest()
b64=lambda b:base64.b64encode(b).decode()

def trim(b):
    return b.decode('utf8','surrogateescape').strip(GO_SPACE).encode('utf8','surrogateescape')

def retain(data,out):
    dest=out/'raw';dest.mkdir(exist_ok=True)
    p=dest/(sha(data)+'.gz')
    if not p.exists():p.write_bytes(gzip.compress(data,mtime=0))
    assert gzip.decompress(p.read_bytes())==data
    return dict(bytes=len(data),sha256=sha(data),gzip=str(p),gzip_sha256=sha(p.read_bytes()))

def snapshot(root,out):
    result={}
    for path in sorted(root.rglob('*')):
        key=os.fsencode(path.relative_to(root)).hex();mode=oct(path.lstat().st_mode&0o777)
        if path.is_symlink():result[key]=dict(kind='symlink',mode=mode,target=b64(os.fsencode(os.readlink(path))))
        elif path.is_dir():result[key]=dict(kind='directory',mode=mode)
        else:result[key]=dict(kind='file',mode=mode,**retain(path.read_bytes(),out))
    return result

def value(cell):
    if isinstance(cell,bytes):return {'blob_base64':b64(cell)}
    return cell

def semantic_controls(conn):
    """Exercise owned cloned-engine defaults/constraints/FTS; roll back all writes."""
    results={}
    conn.execute('PRAGMA foreign_keys=ON')
    conn.execute('SAVEPOINT owned_semantic')
    try:
        conn.execute("INSERT INTO memories(id,content,scope,metadata,embedding,created_at,updated_at) VALUES ('owned-observer','Owned running cafés','private','{}','[]','2000-01-01','2000-01-01')")
        columns=[r[1]for r in conn.execute('PRAGMA table_xinfo(memories)')]
        results['actual_insert_defaults']=dict(zip(columns,conn.execute("SELECT * FROM memories WHERE id='owned-observer'").fetchone()))
        results['fts_ascii_case_stemming']=[r[0]for r in conn.execute("SELECT id FROM memories_fts WHERE memories_fts MATCH 'RUN'")]
        results['fts_unicode_diacritics']=[r[0]for r in conn.execute("SELECT id FROM memories_fts WHERE memories_fts MATCH 'cafes'")]
        conn.execute("UPDATE memories SET content='owned revised' WHERE id='owned-observer'")
        results['fts_update_old_absent']=conn.execute("SELECT COUNT(*) FROM memories_fts WHERE memories_fts MATCH 'running'").fetchone()[0]
        results['fts_update_new_present']=[r[0]for r in conn.execute("SELECT id FROM memories_fts WHERE memories_fts MATCH 'revised'")]
        conn.execute("DELETE FROM memories WHERE id='owned-observer'")
        results['fts_delete_absent']=conn.execute("SELECT COUNT(*) FROM memories_fts WHERE memories_fts MATCH 'revised'").fetchone()[0]
        results['oplog_insert_update_delete']=conn.execute("SELECT op,memory_id FROM sync_oplog WHERE memory_id='owned-observer' ORDER BY event_id").fetchall()
        conn.execute("INSERT INTO memories(id,content,scope,metadata,embedding,created_at,updated_at) VALUES ('owned-observer-exclude','excluded','private','{\"sync_exclude\":\"true\"}','[]','2000-01-01','2000-01-01')")
        conn.execute("UPDATE memories SET content='excluded revised' WHERE id='owned-observer-exclude'")
        results['oplog_excluded_insert_update']=conn.execute("SELECT op,memory_id FROM sync_oplog WHERE memory_id='owned-observer-exclude' ORDER BY event_id").fetchall()
        conn.execute("DELETE FROM memories WHERE id='owned-observer-exclude'")
        results['oplog_excluded_delete']=conn.execute("SELECT op,memory_id FROM sync_oplog WHERE memory_id='owned-observer-exclude' ORDER BY event_id").fetchall()
        conn.execute("INSERT INTO memories(id,content,scope,metadata,embedding,created_at,updated_at) VALUES ('owned-observer-exclude','excluded','private','{\"sync_exclude\":\"true\"}','[]','2000-01-01','2000-01-01')")
        conn.execute("UPDATE memories SET metadata='{}' WHERE id='owned-observer-exclude'")
        conn.execute("DELETE FROM memories WHERE id='owned-observer-exclude'")
        results['oplog_leave_exclusion_then_delete']=conn.execute("SELECT op,memory_id FROM sync_oplog WHERE memory_id='owned-observer-exclude' ORDER BY event_id").fetchall()
        for name,statement in [
            ('op_check',"INSERT INTO sync_oplog(op,memory_id) VALUES ('invalid','owned')"),
            ('granularity_check',"INSERT INTO activity_segments(id,source,granularity,started_at,ended_at,redacted_summary,expires_at) VALUES ('owned','owned','invalid','2000','2000','owned','2099')"),
            ('confidence_check',"INSERT INTO activity_episodes(id,title,started_at,ended_at,confidence,expires_at) VALUES ('owned','owned','2000','2000',2,'2099')"),
            ('foreign_key',"INSERT INTO entities_aliases(entity_id,alias) VALUES ('absent','owned')"),
        ]:
            try:conn.execute(statement);results[name]='accepted-invalid'
            except sqlite3.IntegrityError as error:results[name]=dict(rejected=True,code=error.sqlite_errorcode)
        results['fts_integrity']=None
        try:conn.execute("INSERT INTO memories_fts(memories_fts) VALUES ('integrity-check')");results['fts_integrity']=True
        except sqlite3.DatabaseError as error:results['fts_integrity']=str(error)
        return results
    finally:
        conn.execute('ROLLBACK TO owned_semantic');conn.execute('RELEASE owned_semantic')

def program_inventory(schemas):
    """Only trigger/view formatting is canonicalized; literals and tokens stay.

    Keep full raw DDL separately. No table/row/default/error SQL is rewritten.
    An unknown lexical form fails rather than silently omitting a program.
    """
    import re
    token=re.compile(r"--[^\n]*(?:\n|$)|/\*.*?\*/|'(?:''|[^'])*'|\"(?:\"\"|[^\"])*\"|`(?:``|[^`])*`|\[[^]]*\]|[A-Za-z_][A-Za-z_0-9]*|[0-9]+(?:\.[0-9]+)?|[^\s]",re.S)
    result={}
    for kind,name,table,ddl in schemas:
        if kind not in ('trigger','view'):continue
        assert isinstance(ddl,str) and ddl, (kind,name,'missing body')
        parts=[]
        for match in token.finditer(ddl):
            text=match.group()
            if text.startswith(('--','/*')):continue
            if text[0] in "'\"`[":parts.append(text)
            else:parts.append(text.lower())
        assert (kind,name) not in result,('duplicate program',kind,name)
        result[(kind,name)]=(table,parts)
    # A JSON-friendly stable inventory retains original identity and order.
    return [[kind,name,table,parts] for (kind,name),(table,parts)in sorted(result.items())]

def sql_state(database,owned):
    if not database.is_file():return None
    raw=database.read_bytes()
    if not raw.startswith(b'SQLite format 3\0'):return dict(not_sqlite=True,sha256=sha(raw))
    owned.mkdir()
    copied=owned/'default.db';shutil.copyfile(database,copied)
    for suffix in ['-wal','-shm']:
        source=Path(str(database)+suffix)
        if source.is_file():shutil.copyfile(source,Path(str(copied)+suffix))
    conn=sqlite3.connect(copied)
    try:
        schemas=conn.execute("SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name").fetchall()
        tables={}
        for kind,name,table,ddl in schemas:
            if kind!='table':continue
            quoted='"'+name.replace('"','""')+'"'
            columns=conn.execute('PRAGMA table_xinfo('+quoted+')').fetchall()
            rows=conn.execute('SELECT * FROM '+quoted).fetchall()
            # Only migration applied_at is a live-owner timestamp. Keep every
            # original cell and derive a separate interval proof later.
            tables[name]=dict(columns=columns,rows=[[value(c)for c in row]for row in rows],foreign_keys=conn.execute('PRAGMA foreign_key_list('+quoted+')').fetchall(),indexes=conn.execute('PRAGMA index_list('+quoted+')').fetchall())
        indexes={name:conn.execute('PRAGMA index_xinfo("'+name.replace('"','""')+'")').fetchall()for kind,name,_,_ in schemas if kind=='index'}
        fts={}
        for kind,name,_,ddl in schemas:
            if kind=='table'and ddl and 'USING fts' in ddl:
                quoted='"'+name.replace('"','""')+'"'
                fts[name]=conn.execute('SELECT rowid FROM '+quoted+' WHERE '+quoted+" MATCH 'owned'").fetchall()
        connection_pragmas={p:conn.execute('PRAGMA '+p).fetchall()for p in ['foreign_keys','busy_timeout','secure_delete']}
        semantics=semantic_controls(conn)
        return dict(raw_schema=schemas,program_inventory=program_inventory(schemas),tables=tables,indexes=indexes,fts_queries=fts,semantic_controls=semantics,integrity=conn.execute('PRAGMA integrity_check').fetchall(),foreign_key_check=conn.execute('PRAGMA foreign_key_check').fetchall(),persistent_pragmas={p:conn.execute('PRAGMA '+p).fetchall()for p in ['journal_mode','user_version','application_id','encoding','page_size']},observer_connection_pragmas=connection_pragmas,observer='owned SQLite file+WAL+SHM copies only; semantic writes roll back on this clone; observer pragma is not a live-owner pragma claim')
    finally:conn.close();shutil.rmtree(owned)

def ledger_interval(state,start,end):
    if not state:return None
    table=state['tables'].get('schema_migrations')
    if not table:return None
    columns=[c[1]for c in table['columns']]
    if 'applied_at'not in columns:return None
    at=columns.index('applied_at');versions=[];checks=[]
    for row in table['rows']:
        text=row[at]
        try:
            parsed=datetime.datetime.fromisoformat(text.replace('Z','+00:00'))
            if parsed.tzinfo is None:parsed=parsed.replace(tzinfo=datetime.timezone.utc)
            stamp=parsed.timestamp();valid=int(start)<=stamp<=end+1
        except (ValueError,TypeError):stamp=None;valid=False
        checks.append(dict(raw=text,timestamp=stamp,within_actual_process_interval=valid))
        versions.append(row[:at]+row[at+1:])
    return dict(column=at,checks=checks,versions=versions)

def wal_state(database,out):
    p=Path(str(database)+'-wal')
    if not p.is_file():return None
    raw=p.read_bytes();record=retain(raw,out)
    if len(raw)<32:return dict(**record,valid=False)
    magic,version,pagesize,seq,salt1,salt2,c1,c2=struct.unpack('>8I',raw[:32]);page=65536 if pagesize==1 else pagesize
    valid=magic in(0x377f0682,0x377f0683)and version==3007000 and page>0 and (len(raw)-32)%(page+24)==0
    def checksum(data,seed=(0,0)):
        s1,s2=seed;words=struct.unpack(('>' if magic&1 else '<')+str(len(data)//4)+'I',data)
        for at in range(0,len(words),2):
            s1=(s1+words[at]+s2)&0xffffffff;s2=(s2+words[at+1]+s1)&0xffffffff
        return s1,s2
    header_checksum=checksum(raw[:24])==(c1,c2)
    frames=[];rolling=(c1,c2)
    if valid:
        for offset in range(32,len(raw),page+24):
            number,commit,s1,s2,f1,f2=struct.unpack('>6I',raw[offset:offset+24]);rolling=checksum(raw[offset:offset+8]+raw[offset+24:offset+24+page],rolling)
            frames.append(dict(page=number,commit_pages=commit,salt_equal=(s1,s2)==(salt1,salt2),checksum_valid=rolling==(f1,f2)))
        valid=header_checksum and all(f['salt_equal']and f['checksum_valid']for f in frames)
    return dict(**record,valid=valid,magic=hex(magic),version=version,page_size=page,checkpoint_sequence=seq,salt_base64=b64(raw[16:24]),frames=frames,scope='raw engine role, page/header/frame/salt facts; SQLite integrity and full logical rows separately inspected on copies')

def shm_state(database,out):
    p=Path(str(database)+'-shm')
    if not p.is_file():return None
    raw=p.read_bytes();record=retain(raw,out);wal=Path(str(database)+'-wal')
    if len(raw)<96 or not wal.is_file():return dict(**record,valid=False)
    endian='<'if sys.byteorder=='little'else'>'
    header=struct.unpack(endian+'3I2BH8I',raw[:48])
    version,unused,change,initialized,big_checksum,page,mx_frame,pages,fc1,fc2,salt1,salt2,c1,c2=header
    words=struct.unpack(endian+'10I',raw[:40]);a=b=0
    for at in range(0,10,2):
        a=(a+words[at]+b)&0xffffffff;b=(b+words[at+1]+a)&0xffffffff
    wal_raw=wal.read_bytes();wal_page=struct.unpack('>I',wal_raw[8:12])[0]if len(wal_raw)>=32 else 0
    wal_page=65536 if wal_page==1 else wal_page;shm_page=65536 if page==1 else page
    count=(len(wal_raw)-32)//(wal_page+24)if wal_page else -1
    frame=wal_raw[32+(mx_frame-1)*(wal_page+24):32+mx_frame*(wal_page+24)]if mx_frame else b''
    checksum_matches=bool(frame)and(fc1,fc2)==struct.unpack('>2I',frame[16:24])
    valid=(version==3007000 and initialized==1 and raw[:48]==raw[48:96]and(a,b)==(c1,c2)
           and shm_page==wal_page and raw[32:40]==wal_raw[16:24]and 0<mx_frame<=count and checksum_matches)
    return dict(**record,valid=valid,version=version,page_size=shm_page,committed_frame=mx_frame,database_pages=pages,matching_wal_salt=raw[32:40]==wal_raw[16:24],matching_committed_checksum=checksum_matches,scope='Owned current-platform WAL-index header copies/checksum/page/salt/committed-frame; lock/readmark entropy remains raw retained, not byte equality')
