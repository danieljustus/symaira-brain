use std::sync::{Arc,Mutex};
use symbrain_usage::{Service,Transport,Request,Response,all_providers,needs_go_fallback};
struct Owned(Mutex<Vec<String>>);
impl Transport for Owned{fn request(&self,r:Request)->Result<Response,String>{let auth=r.headers.iter().find(|(n,_)|n.eq_ignore_ascii_case("authorization")).map(|(_,v)|v.clone()).unwrap_or_default();self.0.lock().unwrap().push(auth);Ok(Response{status:401,body:b"{}".to_vec(),headers:Default::default()})}}
fn main(){let id=std::env::args().nth(1).unwrap();let fallback=needs_go_fallback();let p=all_providers().into_iter().find(|p|p.id==id).unwrap();let configured=p.configured;let status=p.auth_status.status.clone();let source=p.auth_status.source.clone().unwrap_or_default();let t=Arc::new(Owned(Mutex::new(Vec::new())));let _=Service::with_transport(vec![p],t.clone()).report();println!("{{\"configured\":{},\"status\":{:?},\"source\":{:?},\"headers\":{:?},\"needs_go\":{}}}",configured,status,source,*t.0.lock().unwrap(),fallback);}
