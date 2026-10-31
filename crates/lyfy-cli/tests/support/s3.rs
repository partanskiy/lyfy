use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

pub type Objects = Arc<Mutex<BTreeMap<String, Vec<u8>>>>;
pub struct Server {
    pub endpoint: String,
    pub objects: Objects,
    pub reject_publication: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn hmac(key: &[u8], bytes: &[u8]) -> Vec<u8> {
    let mut inner = [0x36; 64];
    let mut outer = [0x5c; 64];
    for (index, byte) in key.iter().enumerate() {
        inner[index] ^= byte;
        outer[index] ^= byte;
    }
    let mut digest = Sha256::new();
    digest.update(inner);
    digest.update(bytes);
    let mut final_hash = Sha256::new();
    final_hash.update(outer);
    final_hash.update(digest.finalize());
    final_hash.finalize().to_vec()
}

fn signed(method: &str, target: &str, headers: &BTreeMap<String, String>, body: &[u8]) -> bool {
    let Some(auth) = headers.get("authorization") else {
        return false;
    };
    let Some(date) = headers.get("x-amz-date") else {
        return false;
    };
    let fields: BTreeMap<_, _> = auth
        .trim_start_matches("AWS4-HMAC-SHA256 ")
        .split(", ")
        .filter_map(|p| p.split_once('='))
        .collect();
    let Some(credential) = fields.get("Credential") else {
        return false;
    };
    let Some((key, scope)) = credential.split_once('/') else {
        return false;
    };
    if key != "fixture-access" {
        return false;
    }
    let Some(signed_headers) = fields.get("SignedHeaders") else {
        return false;
    };
    let Some(signature) = fields.get("Signature") else {
        return false;
    };
    let mut canonical_headers = String::new();
    for name in signed_headers.split(';') {
        let Some(value) = headers.get(name) else {
            return false;
        };
        canonical_headers.push_str(&format!(
            "{name}:{}\n",
            value.split_whitespace().collect::<Vec<_>>().join(" ")
        ));
    }
    let payload = hash(body);
    if headers
        .get("x-amz-content-sha256")
        .is_some_and(|value| value != &payload)
    {
        return false;
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut query: Vec<_> = query.split('&').filter(|p| !p.is_empty()).collect();
    query.sort();
    let canonical = format!(
        "{method}\n{path}\n{}\n{canonical_headers}\n{signed_headers}\n{payload}",
        query.join("&")
    );
    let to_sign = format!(
        "AWS4-HMAC-SHA256\n{date}\n{scope}\n{}",
        hash(canonical.as_bytes())
    );
    let mut signing = b"AWS4fixture-secret".to_vec();
    for part in scope.split('/') {
        signing = hmac(&signing, part.as_bytes());
    }
    let calculated: String = hmac(&signing, to_sign.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    calculated == *signature
}

fn response(stream: &mut TcpStream, status: u16, body: &[u8], etag: Option<String>, head: bool) {
    write!(
        stream,
        "HTTP/1.1 {status} fixture\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    )
    .unwrap();
    if let Some(etag) = etag {
        write!(stream, "ETag: \"{etag}\"\r\n").unwrap();
    }
    stream.write_all(b"\r\n").unwrap();
    if !head {
        stream.write_all(body).unwrap();
    }
}

fn handle(mut stream: TcpStream, objects: &Objects, reject: &AtomicBool) {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut request = vec![];
    let end;
    loop {
        let mut byte = [0];
        if stream.read_exact(&mut byte).is_err() {
            return;
        }
        request.push(byte[0]);
        if request.ends_with(b"\r\n\r\n") {
            end = request.len();
            break;
        }
        assert!(request.len() < 65536);
    }
    let text = String::from_utf8(request[..end].to_vec()).unwrap();
    let mut lines = text.split("\r\n");
    let mut first = lines.next().unwrap().split_whitespace();
    let method = first.next().unwrap();
    let target = first.next().unwrap();
    let headers: BTreeMap<_, _> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.to_lowercase(), v.trim().to_owned()))
        .collect();
    if headers.get("expect").is_some_and(|v| v == "100-continue") {
        stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").unwrap();
    }
    let length: usize = headers
        .get("content-length")
        .map(|v| v.parse().unwrap())
        .unwrap_or(0);
    let mut body = vec![0; length];
    stream.read_exact(&mut body).unwrap();
    if !signed(method, target, &headers, &body) {
        response(&mut stream, 403, b"invalid signature", None, false);
        return;
    }
    let url = target.split_once('?').map(|(p, _)| p).unwrap_or(target);
    let key = percent_encoding::percent_decode_str(url.trim_start_matches("/bucket/"))
        .decode_utf8()
        .unwrap()
        .into_owned();
    let mut objects = objects.lock().unwrap();
    if method == "GET" && target.contains("list-type=2") {
        let query: BTreeMap<_, _> = target
            .split_once('?')
            .unwrap()
            .1
            .split('&')
            .filter_map(|p| p.split_once('='))
            .map(|(k, v)| {
                (
                    k,
                    percent_encoding::percent_decode_str(v)
                        .decode_utf8()
                        .unwrap()
                        .into_owned(),
                )
            })
            .collect();
        let prefix = query.get("prefix").unwrap();
        let skip = query
            .get("continuation-token")
            .map(|t| t.parse().unwrap())
            .unwrap_or(0);
        let keys: Vec<_> = objects.keys().filter(|k| k.starts_with(prefix)).collect();
        let mut xml = format!(
            "<ListBucketResult><IsTruncated>{}</IsTruncated>",
            keys.len() > skip + 2
        );
        for key in keys.iter().skip(skip).take(2) {
            xml.push_str(&format!(
                "<Contents><Key>{}</Key></Contents>",
                key.replace('&', "&amp;").replace('<', "&lt;")
            ));
        }
        if keys.len() > skip + 2 {
            xml.push_str(&format!(
                "<NextContinuationToken>{}</NextContinuationToken>",
                skip + 2
            ));
        }
        xml.push_str("</ListBucketResult>");
        response(&mut stream, 200, xml.as_bytes(), None, false);
        return;
    }
    let old = objects.get(&key);
    let etag = old.map(|v| hash(v));
    if headers.get("if-none-match").is_some_and(|v| v == "*") && old.is_some()
        || headers
            .get("if-match")
            .is_some_and(|v| Some(v.trim_matches('"')) != etag.as_deref())
    {
        response(
            &mut stream,
            412,
            b"condition failed",
            None,
            method == "HEAD",
        );
        return;
    }
    match method {
        "GET" | "HEAD" => match old {
            Some(body) => response(&mut stream, 200, body, etag, method == "HEAD"),
            None => response(&mut stream, 404, b"missing", None, method == "HEAD"),
        },
        "PUT" => {
            if key.ends_with("/HEAD") && reject.swap(false, Ordering::SeqCst) {
                response(&mut stream, 412, b"concurrent writer", None, false);
                return;
            }
            let etag = hash(&body);
            objects.insert(key, body);
            response(&mut stream, 200, b"", Some(etag), false);
        }
        "DELETE" => {
            objects.remove(&key);
            response(&mut stream, 204, b"", None, false);
        }
        _ => response(&mut stream, 405, b"unsupported", None, false),
    }
}

impl Server {
    pub fn start() -> Option<Self> {
        let listener = match TcpListener::bind("127.0.0.1:0") {
            Ok(listener) => listener,
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return None,
            Err(e) => panic!("cannot bind fixture: {e}"),
        };
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let objects = Arc::new(Mutex::new(BTreeMap::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let reject = Arc::new(AtomicBool::new(false));
        let (data, done, fault) = (Arc::clone(&objects), Arc::clone(&stop), Arc::clone(&reject));
        let worker = thread::spawn(move || {
            while !done.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => handle(stream, &data, &fault),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(e) => panic!("fixture accept: {e}"),
                }
            }
        });
        Some(Self {
            endpoint,
            objects,
            reject_publication: reject,
            stop,
            thread: Some(worker),
        })
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.thread.take().unwrap().join().unwrap();
    }
}
