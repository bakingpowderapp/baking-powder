use super::{ROUTE, Reply, rpc};
use sdk::json::Json;
use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

const READ_PATIENCE: Duration = Duration::from_secs(5);
const REQUEST_PATIENCE: Duration = Duration::from_secs(30);
const MOST_BODY: usize = 32 << 20;
const MOST_HEADERS: usize = 64;
const MOST_LINE: usize = 8 << 10;
const JSON: &str = "application/json";

struct Timed<'a> {
    stream: &'a TcpStream,
    until: Instant,
}

impl<'a> Timed<'a> {
    fn new(stream: &'a TcpStream) -> Timed<'a> {
        Timed { stream, until: Instant::now() + REQUEST_PATIENCE }
    }

    fn left(&self) -> std::io::Result<Duration> {
        let left = self.until.saturating_duration_since(Instant::now()).min(READ_PATIENCE);
        if left.is_zero() { Err(ErrorKind::TimedOut.into()) } else { Ok(left) }
    }
}

impl Read for Timed<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.stream.set_read_timeout(Some(self.left()?))?;
        self.stream.read(buffer)
    }
}

impl Write for Timed<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.stream.set_write_timeout(Some(self.left()?))?;
        self.stream.write(bytes)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.stream.flush()
    }
}

struct Request {
    method: String,
    target: String,
    body: String,
}

struct Response {
    status: u16,
    reason: &'static str,
    kind: &'static str,
    allow: bool,
    body: String,
}

impl Response {
    fn json(json: Json) -> Response {
        Response { status: 200, reason: "OK", kind: JSON, allow: false, body: json.write() }
    }

    fn empty(status: u16, reason: &'static str) -> Response {
        Response { status, reason, kind: "text/plain", allow: false, body: String::new() }
    }

    fn plain(status: u16, reason: &'static str, body: &str) -> Response {
        Response { status, reason, kind: "text/plain", allow: false, body: String::from(body) }
    }
}

pub(super) fn answer(stream: TcpStream, asking: bool, forward: &dyn Fn(&str, &Json) -> Reply) {
    let response = match read_request(&stream) {
        Ok(request) => route(&request, asking, forward),
        Err(response) => response,
    };
    let _ = write_response(&mut Timed::new(&stream), &response);
}

fn route(request: &Request, asking: bool, forward: &dyn Fn(&str, &Json) -> Reply) -> Response {
    if request.target.split('?').next() != Some(ROUTE) {
        return Response::plain(404, "Not Found", "Baking Powder answers MCP at /mcp");
    }
    match request.method.as_str() {
        "POST" => match rpc::respond(&request.body, asking, forward) {
            Some(json) => Response::json(json),
            None => Response::empty(202, "Accepted"),
        },
        "DELETE" => Response::empty(200, "OK"),
        _ => Response { allow: true, ..Response::plain(405, "Method Not Allowed", "Baking Powder takes MCP requests by POST and keeps no stream open") },
    }
}

fn read_line(reader: &mut impl BufRead, line: &mut String) -> Result<bool, Response> {
    line.clear();
    let read = reader.by_ref().take(MOST_LINE as u64).read_line(line);
    if line.len() >= MOST_LINE && !line.ends_with('\n') {
        return Err(Response::plain(431, "Request Header Fields Too Large", "A header line is over 8 KB"));
    }
    Ok(read.is_ok())
}

fn read_request(stream: &TcpStream) -> Result<Request, Response> {
    let mut reader = BufReader::new(Timed::new(stream));
    let mut line = String::new();
    if !read_line(&mut reader, &mut line)? || line.trim().is_empty() {
        return Err(Response::plain(400, "Bad Request", "No request line"));
    }
    let mut words = line.split_whitespace();
    let (Some(method), Some(target)) = (words.next(), words.next()) else {
        return Err(Response::plain(400, "Bad Request", "The request line needs a method and a target"));
    };
    let (method, target) = (String::from(method), String::from(target));
    let (mut length, mut foreign, mut json, mut ended) = (0usize, false, false, false);
    for _ in 0..MOST_HEADERS {
        if !read_line(&mut reader, &mut line)? {
            return Err(Response::plain(400, "Bad Request", "The headers would not read"));
        }
        let header = line.trim_end();
        if header.is_empty() {
            ended = true;
            break;
        }
        let Some((name, value)) = header.split_once(':').map(|(name, value)| (name.trim(), value.trim())) else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            length = value.parse().map_err(|_| Response::plain(400, "Bad Request", "Content-Length is not a number"))?;
        }
        foreign |= name.eq_ignore_ascii_case("origin") && !local(value);
        json |= name.eq_ignore_ascii_case("content-type") && value.split(';').next().is_some_and(|kind| kind.trim().eq_ignore_ascii_case(JSON));
    }
    if !ended {
        return Err(Response::plain(431, "Request Header Fields Too Large", "The request holds over 64 headers"));
    }
    if foreign {
        return Err(Response::plain(403, "Forbidden", "Baking Powder takes MCP requests from programs on this machine, not from web pages"));
    }
    if method == "POST" && !json {
        return Err(Response::plain(415, "Unsupported Media Type", "Baking Powder takes MCP requests as application/json"));
    }
    if length > MOST_BODY {
        return Err(Response::plain(413, "Content Too Large", "The body is over 32 MB"));
    }
    let mut body = vec![0u8; length];
    if reader.read_exact(&mut body).is_err() {
        return Err(Response::plain(400, "Bad Request", "The body ended early"));
    }
    let body = String::from_utf8(body).map_err(|_| Response::plain(400, "Bad Request", "The body is not UTF-8"))?;
    Ok(Request { method, target, body })
}

fn local(origin: &str) -> bool {
    let host = origin.strip_prefix("http://").or_else(|| origin.strip_prefix("https://")).unwrap_or_default();
    ["localhost", "127.0.0.1", "[::1]"].iter().filter_map(|name| host.strip_prefix(name)).any(|rest| rest.is_empty() || rest.starts_with(':'))
}

fn write_response(stream: &mut impl Write, response: &Response) -> std::io::Result<()> {
    let allow = if response.allow { "Allow: POST, DELETE\r\n" } else { "" };
    let head = format!("HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\n{allow}Connection: close\r\n\r\n", response.status, response.reason, response.kind, response.body.len());
    stream.write_all(head.as_bytes())?;
    stream.write_all(response.body.as_bytes())?;
    stream.flush()
}
