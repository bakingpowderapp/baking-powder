use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
    Raw(Arc<str>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        self.as_f64().filter(|n| n.fract() == 0.0).map(|n| n as i64)
    }

    pub fn as_i32(&self) -> Option<i32> {
        self.as_i64().and_then(|n| i32::try_from(n).ok())
    }

    pub fn as_index(&self, limit: usize) -> Option<usize> {
        self.as_i64().and_then(|n| usize::try_from(n).ok()).filter(|index| *index < limit)
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn write(&self) -> String {
        let mut out = self.write_at(0);
        out.push('\n');
        out
    }

    pub fn write_at(&self, depth: usize) -> String {
        let mut out = String::new();
        self.write_into(&mut out, depth);
        out
    }

    fn write_into(&self, out: &mut String, depth: usize) {
        if let Some(line) = self.line() {
            out.push_str(&line);
            return;
        }
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Number(n) => out.push_str(&number(*n)),
            Json::String(s) => write_string(out, s),
            Json::Raw(text) => out.push_str(text),
            Json::Array(items) => {
                if items.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    newline(out, depth + 1);
                    item.write_into(out, depth + 1);
                }
                newline(out, depth);
                out.push(']');
            }
            Json::Object(fields) => {
                if fields.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push('{');
                for (i, (key, value)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    newline(out, depth + 1);
                    write_string(out, key);
                    out.push_str(": ");
                    value.write_into(out, depth + 1);
                }
                newline(out, depth);
                out.push('}');
            }
        }
    }
}

impl From<&str> for Json {
    fn from(text: &str) -> Json {
        Json::String(String::from(text))
    }
}

impl From<String> for Json {
    fn from(text: String) -> Json {
        Json::String(text)
    }
}

impl Json {
    fn scalar(&self) -> bool {
        match self {
            Json::Null | Json::Bool(_) | Json::Number(_) | Json::String(_) => true,
            Json::Array(items) => items.is_empty(),
            Json::Object(fields) => fields.is_empty(),
            Json::Raw(_) => false,
        }
    }

    pub fn changed(&self, other: &Json) -> Option<String> {
        match (self, other) {
            (Json::Object(fields), Json::Object(_)) => fields.iter().find_map(|(key, value)| {
                let rest = value.changed(other.get(key)?)?;
                Some(if rest.is_empty() || rest.starts_with('[') { format!("{key}{rest}") } else { format!("{key}.{rest}") })
            }),
            (Json::Array(items), Json::Array(others)) => items.iter().zip(others).enumerate().find_map(|(at, (item, other))| {
                let rest = item.changed(other)?;
                Some(if rest.is_empty() || rest.starts_with('[') { format!("[{at}]{rest}") } else { format!("[{at}].{rest}") })
            }),
            _ => (self != other).then(String::new),
        }
    }

    fn line(&self) -> Option<String> {
        let mut line = String::new();
        match self {
            Json::Array(items) if !items.is_empty() && items.iter().all(Json::scalar) => {
                line.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        line.push_str(", ");
                    }
                    item.write_into(&mut line, 0);
                    if line.len() > LINE {
                        return None;
                    }
                }
                line.push(']');
            }
            Json::Object(fields) if !fields.is_empty() && fields.iter().all(|(_, value)| value.scalar()) => {
                line.push('{');
                for (i, (key, value)) in fields.iter().enumerate() {
                    if i > 0 {
                        line.push_str(", ");
                    }
                    write_string(&mut line, key);
                    line.push_str(": ");
                    value.write_into(&mut line, 0);
                    if line.len() > LINE {
                        return None;
                    }
                }
                line.push('}');
            }
            _ => return None,
        }
        (line.len() <= LINE).then_some(line)
    }
}

const LINE: usize = 120;

pub fn number(n: f64) -> String {
    let n = tidy(n);
    if !n.is_finite() {
        String::from("null")
    } else if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

pub fn single(value: f32) -> f64 {
    value.to_string().parse().unwrap_or_else(|_| f64::from(value))
}

pub fn tidy(value: f64) -> f64 {
    if value == 0.0 || !value.is_finite() {
        return value;
    }
    let magnitude = value.abs().log10().floor();
    let scale = 10f64.powi(11 - magnitude as i32);
    if magnitude.abs() > 300.0 || !scale.is_finite() {
        return value;
    }
    (value * scale).round() / scale
}

fn newline(out: &mut String, depth: usize) {
    out.push('\n');
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn write_string(out: &mut String, s: &str) {
    out.push('"');
    let mut rest = s;
    while let Some(at) = find(rest.as_bytes(), |byte| matches!(byte, b'"' | b'\\') | (byte < 0x20)) {
        out.push_str(&rest[..at]);
        match rest.as_bytes()[at] {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\t' => out.push_str("\\t"),
            b'\r' => out.push_str("\\r"),
            control => out.push_str(&format!("\\u{control:04x}")),
        }
        rest = &rest[at + 1..];
    }
    out.push_str(rest);
    out.push('"');
}

const LANES: usize = 8;

fn find(bytes: &[u8], test: impl Fn(u8) -> bool) -> Option<usize> {
    let (blocks, _) = bytes.as_chunks::<LANES>();
    let clear = blocks.iter().take_while(|block| !block.iter().fold(false, |hit, &byte| hit | test(byte))).count() * LANES;
    bytes[clear..].iter().position(|&byte| test(byte)).map(|at| clear + at)
}

const DEPTH: usize = 64;

pub fn parse(text: &str) -> Option<Json> {
    let mut parser = Parser { text, at: 0, depth: 0 };
    let value = parser.value()?;
    parser.skip_ws();
    (parser.at == parser.text.len()).then_some(value)
}

struct Parser<'a> {
    text: &'a str,
    at: usize,
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.at).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.at += 1;
            return true;
        }
        false
    }

    fn word(&mut self, word: &str) -> bool {
        if self.text.as_bytes()[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            return true;
        }
        false
    }

    fn value(&mut self) -> Option<Json> {
        self.skip_ws();
        match self.peek()? {
            b'{' => self.nested(Self::object),
            b'[' => self.nested(Self::array),
            b'"' => self.string().map(Json::String),
            b't' => self.word("true").then_some(Json::Bool(true)),
            b'f' => self.word("false").then_some(Json::Bool(false)),
            b'n' => self.word("null").then_some(Json::Null),
            _ => self.number(),
        }
    }

    fn nested(&mut self, parse: fn(&mut Self) -> Option<Json>) -> Option<Json> {
        self.depth += 1;
        let value = if self.depth > DEPTH { None } else { parse(self) };
        self.depth -= 1;
        value
    }

    fn object(&mut self) -> Option<Json> {
        self.at += 1;
        let mut fields = Vec::new();
        self.skip_ws();
        if self.eat(b'}') {
            return Some(Json::Object(fields));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            if !self.eat(b':') {
                return None;
            }
            let value = self.value()?;
            fields.push((key, value));
            self.skip_ws();
            if self.eat(b',') {
                continue;
            }
            return self.eat(b'}').then_some(Json::Object(fields));
        }
    }

    fn array(&mut self) -> Option<Json> {
        self.at += 1;
        let mut items = Vec::new();
        self.skip_ws();
        if self.eat(b']') {
            return Some(Json::Array(items));
        }
        loop {
            items.push(self.value()?);
            self.skip_ws();
            if self.eat(b',') {
                continue;
            }
            return self.eat(b']').then_some(Json::Array(items));
        }
    }

    fn string(&mut self) -> Option<String> {
        if !self.eat(b'"') {
            return None;
        }
        let mut out = String::new();
        loop {
            let byte = self.peek()?;
            self.at += 1;
            match byte {
                b'"' => return Some(out),
                b'\\' => {
                    let escaped = self.peek()?;
                    self.at += 1;
                    match escaped {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b't' => out.push('\t'),
                        b'r' => out.push('\r'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'u' => {
                            let mut code = self.hex4()?;
                            if (0xD800..0xDC00).contains(&code) {
                                let low = (self.eat(b'\\') && self.eat(b'u')).then(|| self.hex4()).flatten()?;
                                if !(0xDC00..0xE000).contains(&low) {
                                    return None;
                                }
                                code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                            }
                            out.push(char::from_u32(code)?);
                        }
                        _ => return None,
                    }
                }
                _ => {
                    let start = self.at - 1;
                    self.at += find(&self.text.as_bytes()[self.at..], |byte| matches!(byte, b'"' | b'\\'))?;
                    out.push_str(&self.text[start..self.at]);
                }
            }
        }
    }

    fn hex4(&mut self) -> Option<u32> {
        let hex = self.text.get(self.at..self.at + 4)?;
        self.at += 4;
        u32::from_str_radix(hex, 16).ok()
    }

    fn number(&mut self) -> Option<Json> {
        let start = self.at;
        while self.peek().is_some_and(|b| b.is_ascii_digit() || matches!(b, b'-' | b'+' | b'.' | b'e' | b'E')) {
            self.at += 1;
        }
        self.text[start..self.at].parse::<f64>().ok().filter(|n| n.is_finite()).map(Json::Number)
    }
}
