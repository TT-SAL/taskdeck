//! A minimal HTTP/1.1 client over `std::net::TcpStream`: one request per
//! connection (`Connection: close`), the whole answer read, every wait under a
//! timeout. Enough to talk to the standby on plain HTTP (relay.md §15.2) and
//! to be the base of the hostile proxy of device.md §20.1.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);
/// An answer larger than this is a fault of the test, not of the relay.
const MAX_ANSWER: usize = 16 * 1024 * 1024;

#[derive(Debug)]
pub struct HttpReply {
    pub status: u16,
    /// Header names as sent; look them up with [`HttpReply::header`].
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpReply {
    /// The first header of this name (case-insensitive).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// `GET path` on `base` (`http://host:port`).
pub fn http_get(base: &str, path: &str, headers: &[(&str, &str)]) -> HttpReply {
    http_request(base, "GET", path, headers, &[])
}

/// One request with a body of known length; the answer is read to its end.
pub fn http_request(base: &str, method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> HttpReply {
    let authority = base.strip_prefix("http://").unwrap_or_else(|| panic!("not a plain http base: {base}"));
    let mut stream = TcpStream::connect(authority).unwrap_or_else(|e| panic!("connect {authority}: {e}"));
    stream.set_read_timeout(Some(TIMEOUT)).unwrap();
    stream.set_write_timeout(Some(TIMEOUT)).unwrap();
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n");
    for (n, v) in headers {
        head.push_str(&format!("{n}: {v}\r\n"));
    }
    if !body.is_empty() || method == "POST" || method == "PUT" {
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).unwrap();
    stream.write_all(body).unwrap();
    let mut raw = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                raw.extend_from_slice(&chunk[..n]);
                assert!(raw.len() <= MAX_ANSWER, "answer too large");
            }
            Err(e) => panic!("read from {authority}: {e}"),
        }
    }
    parse(&raw)
}

fn parse(raw: &[u8]) -> HttpReply {
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").expect("an HTTP head");
    let head = std::str::from_utf8(&raw[..split]).expect("an ASCII head");
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap();
    let status: u16 = status_line.split(' ').nth(1).and_then(|s| s.parse().ok()).unwrap_or_else(|| panic!("status line {status_line:?}"));
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_string(), v.trim().to_string()))
        .collect();
    let body = raw[split + 4..].to_vec();
    let chunked = headers.iter().any(|(n, v)| n.eq_ignore_ascii_case("transfer-encoding") && v.eq_ignore_ascii_case("chunked"));
    HttpReply { status, headers, body: if chunked { dechunk(&body) } else { body } }
}

fn dechunk(mut rest: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let eol = rest.windows(2).position(|w| w == b"\r\n").expect("a chunk size line");
        let size = usize::from_str_radix(std::str::from_utf8(&rest[..eol]).unwrap().split(';').next().unwrap().trim(), 16).expect("a chunk size");
        rest = &rest[eol + 2..];
        if size == 0 {
            return out;
        }
        out.extend_from_slice(&rest[..size]);
        rest = &rest[size + 2..];
    }
}
