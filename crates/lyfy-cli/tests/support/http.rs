use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

pub struct Request {
    pub method: String,
    pub path: String,
    pub query: BTreeMap<String, String>,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}
pub struct Response {
    pub code: u16,
    pub body: Vec<u8>,
    pub headers: Vec<(String, String)>,
}
pub struct Server {
    pub endpoint: String,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Response {
    pub fn json(code: u16, value: serde_json::Value) -> Self {
        Self {
            code,
            body: serde_json::to_vec(&value).unwrap(),
            headers: vec![],
        }
    }
    pub fn bytes(code: u16, body: Vec<u8>) -> Self {
        Self {
            code,
            body,
            headers: vec![],
        }
    }
}

fn read(stream: &mut TcpStream) -> Option<Request> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut header = vec![];
    loop {
        let mut byte = [0];
        stream.read_exact(&mut byte).ok()?;
        header.push(byte[0]);
        assert!(header.len() <= 65536);
        if header.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let header = String::from_utf8(header).unwrap();
    let mut lines = header.split("\r\n");
    let mut first = lines.next().unwrap().split_whitespace();
    let method = first.next().unwrap().to_owned();
    let target = first.next().unwrap();
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let decode = |s: &str| {
        percent_encoding::percent_decode_str(&s.replace('+', " "))
            .decode_utf8()
            .unwrap()
            .into_owned()
    };
    let query = query
        .split('&')
        .filter_map(|s| s.split_once('='))
        .map(|(k, v)| (decode(k), decode(v)))
        .collect();
    let headers: BTreeMap<String, String> = lines
        .filter_map(|s| s.split_once(':'))
        .map(|(k, v)| (k.to_ascii_lowercase(), v.trim().into()))
        .collect();
    if headers.get("expect").is_some_and(|v| v == "100-continue") {
        stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").unwrap();
    }
    let length: usize = headers
        .get("content-length")
        .map(|s| s.parse().unwrap())
        .unwrap_or(0);
    assert!(length <= 16 * 1024 * 1024);
    let mut body = vec![0; length];
    stream.read_exact(&mut body).ok()?;
    Some(Request {
        method,
        path: decode(path),
        query,
        headers,
        body,
    })
}

impl Server {
    pub fn start(
        handler: impl Fn(Request, &str) -> Option<Response> + Send + 'static,
    ) -> Option<Self> {
        let listener = match TcpListener::bind("127.0.0.1:0") {
            Ok(l) => l,
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return None,
            Err(e) => panic!("fixture listener: {e}"),
        };
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let done = Arc::clone(&stop);
        let base = endpoint.clone();
        let thread = thread::spawn(move || {
            while !done.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if let Some(request) = read(&mut stream)
                            && let Some(response) = handler(request, &base)
                        {
                            write!(stream, "HTTP/1.1 {} fixture\r\nContent-Length: {}\r\nConnection: close\r\n", response.code, response.body.len()).unwrap();
                            for (key, value) in response.headers {
                                write!(stream, "{key}: {value}\r\n").unwrap();
                            }
                            stream.write_all(b"\r\n").unwrap();
                            stream.write_all(&response.body).unwrap();
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1))
                    }
                    Err(e) => panic!("fixture accept: {e}"),
                }
            }
        });
        Some(Self {
            endpoint,
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.thread.take().unwrap().join().unwrap();
    }
}
