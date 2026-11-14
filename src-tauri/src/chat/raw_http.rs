//! Minimal hand-written HTTPS client for Kick.
//!
//! Kick's Cloudflare rules return 403 for requests with lower-case header names
//! (what libraries built on the `http` crate send) or an Accept-Encoding header,
//! so the request is written by hand with Title-Case headers. HTTP/1.0 keeps
//! the response un-chunked.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use serde_json::Value;

use super::USER_AGENT;

pub struct Body<'a> {
    pub content_type: &'a str,
    pub data: String,
}

/// Returns (status, body text).
pub fn request(
    host: &str,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<Body>,
) -> Result<(u16, String), String> {
    let tcp = TcpStream::connect((host, 443)).map_err(|e| e.to_string())?;
    tcp.set_read_timeout(Some(Duration::from_secs(15))).map_err(|e| e.to_string())?;
    let connector = native_tls::TlsConnector::new().map_err(|e| e.to_string())?;
    let mut tls = connector.connect(host, tcp).map_err(|e| e.to_string())?;

    let mut req = format!("{method} {path} HTTP/1.0\r\nHost: {host}\r\nUser-Agent: {USER_AGENT}\r\nAccept: application/json\r\n");
    for (k, v) in headers {
        req += &format!("{k}: {v}\r\n");
    }
    let data = match body {
        Some(b) => {
            req += &format!("Content-Type: {}\r\nContent-Length: {}\r\n", b.content_type, b.data.len());
            b.data
        }
        None => String::new(),
    };
    req += "\r\n";
    req += &data;
    tls.write_all(req.as_bytes()).map_err(|e| e.to_string())?;

    let mut raw = Vec::new();
    // Servers may close without TLS close_notify; keep whatever was read.
    let _ = tls.read_to_end(&mut raw);
    let text = String::from_utf8_lossy(&raw);
    let (head, payload) = text.split_once("\r\n\r\n").ok_or("empty response")?;
    let status = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    Ok((status, payload.to_string()))
}

/// GET/POST JSON, failing on non-2xx.
pub fn json(host: &str, method: &str, path: &str, headers: &[(&str, &str)], body: Option<&Value>) -> Result<Value, String> {
    let body = body.map(|b| Body { content_type: "application/json", data: b.to_string() });
    let (status, text) = request(host, method, path, headers, body)?;
    if !(200..300).contains(&status) {
        return Err(format!("http status: {status} {}", text.chars().take(200).collect::<String>()));
    }
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&text).map_err(|e| e.to_string())
}
