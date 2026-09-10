use std::{collections::HashSet, fmt, path::PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TestKind {
    Parse,
    SyntaxNegative,
    Negative,
    Run,
}

impl TestKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Parse => "parse",
            Self::SyntaxNegative => "syntax-negative",
            Self::Negative => "negative",
            Self::Run => "run",
        }
    }

    fn from_keyword(value: &str) -> Result<Self, ManifestError> {
        match value {
            "parse" => Ok(Self::Parse),
            "syntax-negative" => Ok(Self::SyntaxNegative),
            "negative" => Ok(Self::Negative),
            "run" => Ok(Self::Run),
            other => Err(ManifestError::semantic(format!(
                "unknown conformance test kind :{other}"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TestCase {
    pub path: PathBuf,
    pub kind: TestKind,
    pub expected: Option<String>,
    pub exit: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct Suite {
    pub name: String,
    pub version: i64,
    pub active_kinds: Vec<TestKind>,
    pub tests: Vec<TestCase>,
}

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Nil,
    Bool(bool),
    Integer(i64),
    String(String),
    Keyword(String),
    Symbol(String),
    Vector(Vec<Value>),
    Map(Vec<(Value, Value)>),
    Tagged(String, Box<Value>),
}

#[derive(Debug, Clone)]
pub struct ManifestError {
    offset: Option<usize>,
    message: String,
}

impl ManifestError {
    fn at(offset: usize, message: impl Into<String>) -> Self {
        Self {
            offset: Some(offset),
            message: message.into(),
        }
    }

    fn semantic(message: impl Into<String>) -> Self {
        Self {
            offset: None,
            message: message.into(),
        }
    }
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.offset {
            Some(offset) => write!(f, "byte {offset}: {}", self.message),
            None => write!(f, "{}", self.message),
        }
    }
}

impl std::error::Error for ManifestError {}

pub fn parse_suite(input: &str) -> Result<Suite, ManifestError> {
    let mut parser = Parser::new(input);
    let root = parser.parse_document()?;
    suite_from_value(root)
}

fn suite_from_value(value: Value) -> Result<Suite, ManifestError> {
    let map = expect_map(&value, "suite root")?;
    validate_keyword_map_keys(
        map,
        &["suite", "version", "active-kinds", "tests"],
        "suite root",
    )?;

    let name = expect_keyword(required(map, "suite")?, ":suite")?.to_owned();
    let version = expect_integer(required(map, "version")?, ":version")?;

    let active_values = expect_vector(required(map, "active-kinds")?, ":active-kinds")?;
    let mut active_kinds = Vec::with_capacity(active_values.len());
    for value in active_values {
        let keyword = expect_keyword(value, ":active-kinds entry")?;
        let kind = TestKind::from_keyword(keyword)?;
        if !active_kinds.contains(&kind) {
            active_kinds.push(kind);
        }
    }

    let test_values = expect_vector(required(map, "tests")?, ":tests")?;
    let mut tests = Vec::with_capacity(test_values.len());

    for value in test_values {
        let test = expect_map(value, "test entry")?;
        validate_keyword_map_keys(test, &["path", "kind", "expect", "exit"], "test entry")?;

        let path = expect_path(required(test, "path")?, ":path")?;
        let kind = TestKind::from_keyword(expect_keyword(required(test, "kind")?, ":kind")?)?;
        let expected = optional(test, "expect")
            .map(|value| expect_keyword(value, ":expect").map(ToOwned::to_owned))
            .transpose()?;
        let exit = optional(test, "exit")
            .map(|value| {
                let value = expect_integer(value, ":exit")?;
                i32::try_from(value).map_err(|_| ManifestError::semantic(":exit does not fit i32"))
            })
            .transpose()?;

        tests.push(TestCase {
            path,
            kind,
            expected,
            exit,
        });
    }

    if active_kinds.is_empty() {
        return Err(ManifestError::semantic(
            ":active-kinds must contain at least one test kind",
        ));
    }

    Ok(Suite {
        name,
        version,
        active_kinds,
        tests,
    })
}

fn validate_keyword_map_keys(
    map: &[(Value, Value)],
    allowed: &[&str],
    description: &str,
) -> Result<(), ManifestError> {
    let mut seen = HashSet::new();
    for (key, _) in map {
        let Value::Keyword(name) = key else {
            return Err(ManifestError::semantic(format!(
                "{description} keys must be FDN keywords"
            )));
        };
        if !allowed.contains(&name.as_str()) {
            return Err(ManifestError::semantic(format!(
                "unknown {description} key :{name}"
            )));
        }
        if !seen.insert(name.as_str()) {
            return Err(ManifestError::semantic(format!(
                "duplicate {description} key :{name}"
            )));
        }
    }
    Ok(())
}

fn required<'a>(map: &'a [(Value, Value)], key: &str) -> Result<&'a Value, ManifestError> {
    optional(map, key).ok_or_else(|| ManifestError::semantic(format!("missing required :{key}")))
}

fn optional<'a>(map: &'a [(Value, Value)], key: &str) -> Option<&'a Value> {
    map.iter().find_map(|(candidate, value)| match candidate {
        Value::Keyword(name) if name == key => Some(value),
        _ => None,
    })
}

fn expect_map<'a>(
    value: &'a Value,
    description: &str,
) -> Result<&'a [(Value, Value)], ManifestError> {
    match value {
        Value::Map(values) => Ok(values),
        _ => Err(ManifestError::semantic(format!(
            "{description} must be an FDN map"
        ))),
    }
}

fn expect_vector<'a>(value: &'a Value, description: &str) -> Result<&'a [Value], ManifestError> {
    match value {
        Value::Vector(values) => Ok(values),
        _ => Err(ManifestError::semantic(format!(
            "{description} must be an FDN vector"
        ))),
    }
}

fn expect_keyword<'a>(value: &'a Value, description: &str) -> Result<&'a str, ManifestError> {
    match value {
        Value::Keyword(value) => Ok(value),
        _ => Err(ManifestError::semantic(format!(
            "{description} must be an FDN keyword"
        ))),
    }
}

fn expect_integer(value: &Value, description: &str) -> Result<i64, ManifestError> {
    match value {
        Value::Integer(value) => Ok(*value),
        _ => Err(ManifestError::semantic(format!(
            "{description} must be an integer"
        ))),
    }
}

fn expect_path(value: &Value, description: &str) -> Result<PathBuf, ManifestError> {
    match value {
        Value::Tagged(tag, inner) if tag == "path" => match inner.as_ref() {
            Value::String(path) => Ok(PathBuf::from(path)),
            _ => Err(ManifestError::semantic(format!(
                "{description} #path payload must be a string"
            ))),
        },
        _ => Err(ManifestError::semantic(format!(
            "{description} must use #path \"...\""
        ))),
    }
}

struct Parser<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            pos: 0,
        }
    }

    fn parse_document(&mut self) -> Result<Value, ManifestError> {
        self.skip_trivia()?;
        let value = self.parse_value()?;
        self.skip_trivia()?;
        if self.pos != self.input.len() {
            return Err(self.error("unexpected trailing input"));
        }
        Ok(value)
    }

    fn parse_value(&mut self) -> Result<Value, ManifestError> {
        self.skip_trivia()?;
        match self.peek() {
            Some(b'{') => self.parse_map(),
            Some(b'[') => self.parse_vector(),
            Some(b'"') => self.parse_string().map(Value::String),
            Some(b':') => self.parse_keyword(),
            Some(b'#') => self.parse_tagged(),
            Some(_) => self.parse_scalar(),
            None => Err(self.error("expected FDN value")),
        }
    }

    fn parse_map(&mut self) -> Result<Value, ManifestError> {
        self.expect(b'{')?;
        let mut values = Vec::new();
        loop {
            self.skip_trivia()?;
            if self.consume(b'}') {
                break;
            }
            let key = self.parse_value()?;
            self.skip_trivia()?;
            if self.peek() == Some(b'}') {
                return Err(self.error("map key is missing a value"));
            }
            let value = self.parse_value()?;
            values.push((key, value));
        }
        Ok(Value::Map(values))
    }

    fn parse_vector(&mut self) -> Result<Value, ManifestError> {
        self.expect(b'[')?;
        let mut values = Vec::new();
        loop {
            self.skip_trivia()?;
            if self.consume(b']') {
                break;
            }
            values.push(self.parse_value()?);
        }
        Ok(Value::Vector(values))
    }

    fn parse_keyword(&mut self) -> Result<Value, ManifestError> {
        self.expect(b':')?;
        let token = self.read_token()?;
        if token.is_empty() {
            return Err(self.error("empty keyword"));
        }
        Ok(Value::Keyword(token))
    }

    fn parse_tagged(&mut self) -> Result<Value, ManifestError> {
        self.expect(b'#')?;
        if self.peek() == Some(b'{') {
            return Err(self.error("FDN sets are not needed by suite.fdn yet"));
        }
        let tag = self.read_token()?;
        if tag.is_empty() {
            return Err(self.error("empty reader tag"));
        }
        self.skip_trivia()?;
        let value = self.parse_value()?;
        Ok(Value::Tagged(tag, Box::new(value)))
    }

    fn parse_scalar(&mut self) -> Result<Value, ManifestError> {
        let token = self.read_token()?;
        match token.as_str() {
            "nil" => Ok(Value::Nil),
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => match token.parse::<i64>() {
                Ok(value) => Ok(Value::Integer(value)),
                Err(_) => Ok(Value::Symbol(token)),
            },
        }
    }

    fn parse_string(&mut self) -> Result<String, ManifestError> {
        self.expect(b'"')?;
        let mut bytes = Vec::new();
        loop {
            let byte = self
                .next()
                .ok_or_else(|| self.error("unterminated string"))?;
            match byte {
                b'"' => break,
                b'\\' => {
                    let escaped = self
                        .next()
                        .ok_or_else(|| self.error("unterminated string escape"))?;
                    match escaped {
                        b'n' => bytes.push(b'\n'),
                        b'r' => bytes.push(b'\r'),
                        b't' => bytes.push(b'\t'),
                        b'0' => bytes.push(0),
                        b'\\' => bytes.push(b'\\'),
                        b'"' => bytes.push(b'"'),
                        other => {
                            return Err(self.error(format!(
                                "unsupported string escape \\{}",
                                char::from(other)
                            )));
                        }
                    }
                }
                other => bytes.push(other),
            }
        }
        String::from_utf8(bytes).map_err(|_| self.error("string is not valid UTF-8"))
    }

    fn read_token(&mut self) -> Result<String, ManifestError> {
        let start = self.pos;
        while let Some(byte) = self.peek() {
            if is_delimiter(byte) {
                break;
            }
            self.pos += 1;
        }
        let bytes = &self.input[start..self.pos];
        String::from_utf8(bytes.to_vec()).map_err(|_| self.error("token is not valid UTF-8"))
    }

    fn skip_trivia(&mut self) -> Result<(), ManifestError> {
        loop {
            while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n' | b',')) {
                self.pos += 1;
            }

            if self.peek() == Some(b';') {
                self.skip_line_comment();
                continue;
            }

            if self.starts_with(b"//") {
                self.pos += 2;
                self.skip_line_comment();
                continue;
            }

            if self.starts_with(b"/*") {
                self.pos += 2;
                while !self.starts_with(b"*/") {
                    if self.next().is_none() {
                        return Err(self.error("unterminated block comment"));
                    }
                }
                self.pos += 2;
                continue;
            }

            return Ok(());
        }
    }

    fn skip_line_comment(&mut self) {
        while let Some(byte) = self.next() {
            if byte == b'\n' {
                break;
            }
        }
    }

    fn starts_with(&self, pattern: &[u8]) -> bool {
        self.input[self.pos..].starts_with(pattern)
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let value = self.peek()?;
        self.pos += 1;
        Some(value)
    }

    fn consume(&mut self, expected: u8) -> bool {
        if self.peek() == Some(expected) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: u8) -> Result<(), ManifestError> {
        if self.consume(expected) {
            Ok(())
        } else {
            Err(self.error(format!("expected '{}'", char::from(expected))))
        }
    }

    fn error(&self, message: impl Into<String>) -> ManifestError {
        ManifestError::at(self.pos, message)
    }
}

fn is_delimiter(byte: u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b',' | b'{' | b'}' | b'[' | b']' | b'"' | b';')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_conformance_manifest_shape() {
        let suite = parse_suite(
            r#"{
                :suite :forge/conformance
                :version 1
                :active-kinds [:parse :syntax-negative]
                :tests [
                    {:path #path "parse/a.fg" :kind :parse}
                    {:path #path "syntax-negative/s.fg" :kind :syntax-negative :expect :syntax/rejected}
                    {:path #path "negative/b.fg" :kind :negative :expect :type/mismatch}
                    {:path #path "run/c.fg" :kind :run :exit 0}
                ]
            }"#,
        )
        .expect("manifest should parse");

        assert_eq!(suite.name, "forge/conformance");
        assert_eq!(suite.version, 1);
        assert_eq!(
            suite.active_kinds,
            vec![TestKind::Parse, TestKind::SyntaxNegative]
        );
        assert_eq!(suite.tests.len(), 4);
        assert_eq!(suite.tests[1].expected.as_deref(), Some("syntax/rejected"));
        assert_eq!(suite.tests[2].expected.as_deref(), Some("type/mismatch"));
        assert_eq!(suite.tests[3].exit, Some(0));
    }

    #[test]
    fn supports_comments_and_optional_commas() {
        let suite = parse_suite(
            r#"{
                ; FDN comment
                :suite :forge/conformance,
                :version 1,
                :active-kinds [:parse],
                :tests []
            }"#,
        )
        .expect("manifest should parse");

        assert_eq!(suite.active_kinds, vec![TestKind::Parse]);
    }

    #[test]
    fn rejects_unknown_and_duplicate_keys() {
        assert!(parse_suite(
            r#"{:suite :forge/conformance :version 1 :active-kinds [:parse] :tests [] :typo true}"#
        )
        .is_err());
        assert!(parse_suite(
            r#"{:suite :forge/conformance :version 1 :version 1 :active-kinds [:parse] :tests []}"#
        )
        .is_err());
    }
}
