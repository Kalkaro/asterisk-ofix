//! Valve's text KeyValues format (config.vdf, *.acf, compatibilitytool.vdf, ...).

use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Str(String),
    Obj(Obj),
}

/// A parsed object; keys keep their order and may repeat.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Obj(pub Vec<(String, Value)>);

static EMPTY: Obj = Obj(Vec::new());

impl Obj {
    /// The first value under `key`, compared case-insensitively as Steam does.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v)
    }

    /// The string under `key`, or "".
    pub fn str(&self, key: &str) -> &str {
        match self.get(key) {
            Some(Value::Str(s)) => s,
            _ => "",
        }
    }

    /// The object under `key`, or an empty one.
    pub fn obj(&self, key: &str) -> &Obj {
        match self.get(key) {
            Some(Value::Obj(o)) => o,
            _ => &EMPTY,
        }
    }

    pub fn path(&self, keys: &[&str]) -> &Obj {
        keys.iter().fold(self, |o, k| o.obj(k))
    }
}

pub fn parse(src: &str) -> Result<Obj, String> {
    Parser { src: src.as_bytes(), pos: 0 }.object(true)
}

/// Reads and parses a file; a missing or broken file is an empty object.
pub fn read(path: &Path) -> Obj {
    std::fs::read_to_string(path).ok().and_then(|s| parse(&s).ok()).unwrap_or_default()
}

enum Token {
    Str(String),
    Open,
    Close,
    End,
}

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn object(&mut self, top: bool) -> Result<Obj, String> {
        let mut o = Obj::default();
        loop {
            let key = match self.next() {
                Token::End if top => return Ok(o),
                Token::End => return Err("vdf: unexpected end of input".into()),
                Token::Close if !top => return Ok(o),
                Token::Str(s) => s,
                _ => return Err("vdf: expected key".into()),
            };
            match self.next() {
                Token::Open => {
                    let child = self.object(false)?;
                    o.0.push((key, Value::Obj(child)));
                }
                Token::Str(s) => o.0.push((key, Value::Str(s))),
                _ => return Err(format!("vdf: expected value for {key}")),
            }
        }
    }

    fn next(&mut self) -> Token {
        let src = self.src;
        while self.pos < src.len() {
            match src[self.pos] {
                b' ' | b'\t' | b'\r' | b'\n' => self.pos += 1,
                b'/' if src.get(self.pos + 1) == Some(&b'/') => {
                    while self.pos < src.len() && src[self.pos] != b'\n' {
                        self.pos += 1;
                    }
                }
                // Conditionals like [$WIN32].
                b'[' => {
                    while self.pos < src.len() && src[self.pos] != b']' {
                        self.pos += 1;
                    }
                    self.pos += 1;
                }
                _ => break,
            }
        }
        let Some(&c) = src.get(self.pos) else { return Token::End };
        match c {
            b'{' => {
                self.pos += 1;
                Token::Open
            }
            b'}' => {
                self.pos += 1;
                Token::Close
            }
            b'"' => {
                self.pos += 1;
                let mut out = Vec::new();
                while self.pos < src.len() && src[self.pos] != b'"' {
                    if src[self.pos] == b'\\' && self.pos + 1 < src.len() {
                        self.pos += 1;
                        out.push(match src[self.pos] {
                            b'n' => b'\n',
                            b't' => b'\t',
                            other => other,
                        });
                    } else {
                        out.push(src[self.pos]);
                    }
                    self.pos += 1;
                }
                self.pos += 1;
                Token::Str(String::from_utf8_lossy(&out).into_owned())
            }
            _ => {
                let start = self.pos;
                while self.pos < src.len() && !b" \t\r\n{}\"".contains(&src[self.pos]) {
                    self.pos += 1;
                }
                Token::Str(String::from_utf8_lossy(&src[start..self.pos]).into_owned())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_objects_comments_and_escapes() {
        let o = parse("\"a\" { // c\n \"b\" \"x\\\\y\" c { d e } }").unwrap();
        assert_eq!(o.path(&["A"]).str("b"), r"x\y");
        assert_eq!(o.path(&["a", "c"]).str("d"), "e");
    }

    #[test]
    fn rejects_truncated_input() {
        assert!(parse("\"a\" { \"b\" \"c\"").is_err());
    }
}
