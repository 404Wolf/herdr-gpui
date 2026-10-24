//! A Gitea stand-in for tests: a local HTTP/1.1 server answering canned JSON
//! per method and path, recording each request it served.

use super::registry::Host;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
    thread,
};

/// One canned answer: method, path with query, status, extra headers, body.
pub type Route = (&'static str, String, u16, Vec<(String, String)>, String);

/// What the server was asked: method, path with query, authorization, body.
pub type Seen = (String, String, Option<String>, String);

pub struct Server {
    pub base: String,
    pub seen: Arc<Mutex<Vec<Seen>>>,
}

impl Server {
    /// Serves `routes` until the test process ends; an unknown path is a 404.
    pub fn start(routes: Vec<Route>) -> std::io::Result<Self> {
        Self::start_with(|_| routes)
    }

    /// Serves the routes `routes` builds from the server's own base address,
    /// for answers that name it, such as a pull request's web address.
    pub fn start_with(routes: impl FnOnce(&str) -> Vec<Route>) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let base = format!("http://{}", listener.local_addr()?);
        let routes = routes(&base);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = seen.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(match stream.try_clone() {
                    Ok(clone) => clone,
                    Err(_) => continue,
                });
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                let mut parts = line.split_whitespace();
                let method = parts.next().unwrap_or_default().to_owned();
                let path = parts.next().unwrap_or_default().to_owned();
                let mut length = 0usize;
                let mut authorization = None;
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
                        break;
                    }
                    if let Some((name, value)) = header.split_once(':') {
                        let value = value.trim().to_owned();
                        match name.trim().to_ascii_lowercase().as_str() {
                            "content-length" => length = value.parse().unwrap_or(0),
                            "authorization" => authorization = Some(value),
                            _ => {}
                        }
                    }
                }
                let mut body = vec![0; length];
                let _ = reader.read_exact(&mut body);
                if let Ok(mut seen) = record.lock() {
                    seen.push((
                        method.clone(),
                        path.clone(),
                        authorization,
                        String::from_utf8_lossy(&body).into_owned(),
                    ));
                }
                let (status, headers, body) = routes
                    .iter()
                    .find(|(m, p, ..)| *m == method && *p == path)
                    .map(|(_, _, status, headers, body)| (*status, headers.clone(), body.clone()))
                    .unwrap_or((404, Vec::new(), r#"{"message":"not found"}"#.to_owned()));
                let mut reply = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
                    body.len()
                );
                for (name, value) in headers {
                    reply.push_str(&format!("{name}: {value}\r\n"));
                }
                reply.push_str("\r\n");
                reply.push_str(&body);
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        Ok(Self { base, seen })
    }

    /// A host served by this server, named `host`.
    pub fn host(&self, host: &str) -> Host {
        Host {
            host: host.to_owned(),
            names: vec![host.to_owned()],
            base: self.base.clone(),
            secure: false,
            token_command: Vec::new(),
            ca_file: None,
        }
    }

    pub fn requests(&self) -> Vec<Seen> {
        self.seen
            .lock()
            .map(|seen| seen.clone())
            .unwrap_or_default()
    }
}

/// A 200 route answering `body`.
pub fn ok(method: &'static str, path: &str, body: serde_json::Value) -> Route {
    (method, path.to_owned(), 200, Vec::new(), body.to_string())
}
