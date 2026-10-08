"""Owned loopback TLS server; records complete bytes without public network."""
import json
import socket
import ssl
import threading

class Peer:
    def __init__(self, root, cases, api, web):
        self.rows=[];self.errors=[];self.cases={row['id']:row for row in cases};self.api=api;self.web=web
        context=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);context.load_cert_chain(root/'leaf.pem',root/'key.pem')
        self.context=context;self.sock=socket.socket();self.sock.bind(('127.0.0.1',0));self.sock.listen();self.sock.settimeout(.1)
        self.address='127.0.0.1:'+str(self.sock.getsockname()[1]);self.stop=threading.Event();self.phase='go';self.workers=[]
        self.thread=threading.Thread(target=self.serve);self.thread.start()
    def serve(self):
        while not self.stop.is_set():
            try: sock,_=self.sock.accept()
            except TimeoutError:continue
            except OSError:break
            worker=threading.Thread(target=self.handle,args=(sock,self.phase));worker.start();self.workers.append(worker)
    def handle(self,sock,phase):
        try:
            sock.settimeout(10)
            with self.context.wrap_socket(sock,server_side=True) as stream:
                raw=b''
                while b'\r\n\r\n' not in raw:
                    chunk=stream.recv(8192)
                    assert chunk,'premature close';raw+=chunk;assert len(raw)<262144,'owned request bound'
                head,body=raw.split(b'\r\n\r\n',1);lines=head.split(b'\r\n');headers={}
                for line in lines[1:]:
                    k,v=line.split(b':',1);headers.setdefault(k.decode('ascii').lower(),[]).append(v.strip(b' \t'))
                size=int(headers.get('content-length',[b'0'])[0]);assert size<65537
                while len(body)<size:body+=stream.recv(size-len(body))
                identity=headers['x-owned-case'][0].decode();index=int(headers['x-owned-index'][0]);entry=self.cases[identity];status=entry['responses'][index]
                payload=b'{}'
                if entry['kind']=='transport-bound':payload=b'x'*entry['length']
                elif status==200 and 'fixture' in entry:
                    payload=self.fixtures[entry['fixture']]
                    if entry['kind']=='malformed':payload=b'broken'
                    if entry['kind']=='empty':payload=b''
                elif status==200:
                    payload=self.web if lines[0].startswith(b'POST ') else self.api
                    if entry['kind']=='cli-invalid-json':payload=b'broken'
                response=f'HTTP/1.1 {status} Owned\r\nContent-Length: {len(payload)}\r\nContent-Type: application/json\r\nConnection: close\r\n'.encode()
                if status==429:
                    values=entry.get('retry_after_values_hex',[entry.get('retry_after','3').encode().hex()])
                    for value in values:response+=b'Retry-After: '+bytes.fromhex(value)+b'\r\n'
                response+=b'\r\n'+payload
                self.rows.append(dict(phase=phase,id=identity,index=index,request_hex=(head+b'\r\n\r\n'+body).hex(),request_line_hex=lines[0].hex(),headers_hex={k:[v.hex() for v in values] for k,values in headers.items()},body_hex=body.hex(),response_hex=response.hex(),tls_version=stream.version(),cipher=stream.cipher()))
                stream.sendall(response)
        except Exception as error:self.errors.append(repr(error))
        finally:sock.close()
    def close(self):
        self.stop.set();self.sock.close();self.thread.join(3)
        for thread in self.workers:thread.join(12)
        assert not self.thread.is_alive() and all(not thread.is_alive()for thread in self.workers),'owned peer teardown'
        assert not self.errors,self.errors
