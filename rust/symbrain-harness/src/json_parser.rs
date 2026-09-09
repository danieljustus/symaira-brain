use crate::HarnessError;

const MAX_NESTING_DEPTH: usize = 128;
const MAX_VALUES: usize = 100_000;
const MAX_STRING_BYTES: usize = 1 << 20;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Object(Object),
    Array(Vec<Self>),
    String(String),
    Number(String),
    Bool(bool),
    Null,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Object {
    pub keys: Vec<String>,
    pub values: std::collections::HashMap<String, Value>,
}

impl Object {
    pub fn new() -> Self {
        Self {
            keys: Vec::new(),
            values: std::collections::HashMap::new(),
        }
    }
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }
    pub fn set(&mut self, key: &str, value: Value) {
        if !self.values.contains_key(key) {
            self.keys.push(key.to_owned());
        }
        self.values.insert(key.to_owned(), value);
    }
    pub fn remove(&mut self, key: &str) -> bool {
        if self.values.remove(key).is_none() {
            return false;
        }
        self.keys.retain(|item| item != key);
        true
    }
}

pub(crate) fn parse(input: &[u8]) -> Result<Value, HarnessError> {
    if input.len() > crate::write::MAX_CONFIG_BYTES {
        return Err(HarnessError::Json(
            "input exceeds maximum configuration size".into(),
        ));
    }
    if input.iter().all(u8::is_ascii_whitespace) {
        return Ok(Value::Object(Object::new()));
    }
    let text =
        std::str::from_utf8(input).map_err(|_| HarnessError::Json("input is not UTF-8".into()))?;
    let mut parser = Parser {
        input: text.as_bytes(),
        position: 0,
        values: 0,
    };
    let value = parser.value()?;
    parser.whitespace();
    if parser.position != parser.input.len() {
        return Err(HarnessError::Json(
            "unexpected trailing content after the top-level value".into(),
        ));
    }
    if matches!(value, Value::Object(_)) {
        Ok(value)
    } else {
        Err(HarnessError::Json(
            "top-level JSON value is not an object".into(),
        ))
    }
}

struct Parser<'a> {
    input: &'a [u8],
    position: usize,
    values: usize,
}
impl Parser<'_> {
    fn whitespace(&mut self) {
        while self
            .input
            .get(self.position)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.position += 1;
        }
    }
    fn take(&mut self, expected: u8) -> Result<(), HarnessError> {
        self.whitespace();
        if self.input.get(self.position) == Some(&expected) {
            self.position += 1;
            Ok(())
        } else {
            Err(HarnessError::Json(format!(
                "expected `{}` at byte {}",
                expected as char, self.position
            )))
        }
    }
    fn value(&mut self) -> Result<Value, HarnessError> {
        self.value_at_depth(0)
    }
    fn value_at_depth(&mut self, depth: usize) -> Result<Value, HarnessError> {
        self.values = self.values.saturating_add(1);
        if self.values > MAX_VALUES {
            return Err(HarnessError::Json(
                "maximum JSON value count exceeded".into(),
            ));
        }
        if depth > MAX_NESTING_DEPTH {
            return Err(HarnessError::Json(format!(
                "maximum nesting depth of {MAX_NESTING_DEPTH} exceeded"
            )));
        }
        self.whitespace();
        match self.input.get(self.position).copied() {
            Some(b'{') => self.object(depth + 1),
            Some(b'[') => self.array(depth + 1),
            Some(b'"') => self.string().map(Value::String),
            Some(b't') => self.literal(b"true", Value::Bool(true)),
            Some(b'f') => self.literal(b"false", Value::Bool(false)),
            Some(b'n') => self.literal(b"null", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(HarnessError::Json(format!(
                "unexpected byte at {}",
                self.position
            ))),
            None => Err(HarnessError::Json("unexpected end of input".into())),
        }
    }
    fn object(&mut self, depth: usize) -> Result<Value, HarnessError> {
        self.take(b'{')?;
        let mut object = Object::new();
        self.whitespace();
        if self.input.get(self.position) == Some(&b'}') {
            self.position += 1;
            return Ok(Value::Object(object));
        }
        loop {
            let key = self.string()?;
            self.take(b':')?;
            let value = self.value_at_depth(depth)?;
            object.set(&key, value);
            self.whitespace();
            match self.input.get(self.position) {
                Some(b',') => {
                    self.position += 1;
                }
                Some(b'}') => {
                    self.position += 1;
                    break;
                }
                _ => return Err(HarnessError::Json("expected `,` or `}` in object".into())),
            }
        }
        Ok(Value::Object(object))
    }
    fn array(&mut self, depth: usize) -> Result<Value, HarnessError> {
        self.take(b'[')?;
        let mut values = Vec::new();
        self.whitespace();
        if self.input.get(self.position) == Some(&b']') {
            self.position += 1;
            return Ok(Value::Array(values));
        }
        loop {
            values.push(self.value_at_depth(depth)?);
            self.whitespace();
            match self.input.get(self.position) {
                Some(b',') => {
                    self.position += 1;
                }
                Some(b']') => {
                    self.position += 1;
                    break;
                }
                _ => return Err(HarnessError::Json("expected `,` or `]` in array".into())),
            }
        }
        Ok(Value::Array(values))
    }
    fn string(&mut self) -> Result<String, HarnessError> {
        self.whitespace();
        let start = self.position;
        self.take(b'"')?;
        while let Some(byte) = self.input.get(self.position).copied() {
            self.position += 1;
            if self.position.saturating_sub(start) > MAX_STRING_BYTES {
                return Err(HarnessError::Json(
                    "maximum JSON string size exceeded".into(),
                ));
            }
            match byte {
                b'\\' => {
                    self.position += 1;
                    if self.input.get(self.position).is_none() {
                        break;
                    }
                }
                b'"' => {
                    let raw = std::str::from_utf8(&self.input[start..self.position])
                        .map_err(|_| HarnessError::Json("invalid UTF-8 string".into()))?;
                    return serde_json::from_str(raw)
                        .map_err(|_| HarnessError::Json("invalid JSON string".into()));
                }
                _ => {}
            }
        }
        Err(HarnessError::Json("unterminated JSON string".into()))
    }
    fn number(&mut self) -> Result<Value, HarnessError> {
        self.whitespace();
        let start = self.position;
        if self.input.get(self.position) == Some(&b'-') {
            self.position += 1;
        }
        match self.input.get(self.position) {
            Some(b'0') => self.position += 1,
            Some(b'1'..=b'9') => {
                while self
                    .input
                    .get(self.position)
                    .is_some_and(u8::is_ascii_digit)
                {
                    self.position += 1;
                }
            }
            _ => return Err(HarnessError::Json("invalid JSON number".into())),
        }
        if self.input.get(self.position) == Some(&b'.') {
            self.position += 1;
            let before = self.position;
            while self
                .input
                .get(self.position)
                .is_some_and(u8::is_ascii_digit)
            {
                self.position += 1;
            }
            if before == self.position {
                return Err(HarnessError::Json("invalid JSON fraction".into()));
            }
        }
        if self
            .input
            .get(self.position)
            .is_some_and(|b| *b == b'e' || *b == b'E')
        {
            self.position += 1;
            if self
                .input
                .get(self.position)
                .is_some_and(|b| *b == b'+' || *b == b'-')
            {
                self.position += 1;
            }
            let before = self.position;
            while self
                .input
                .get(self.position)
                .is_some_and(u8::is_ascii_digit)
            {
                self.position += 1;
            }
            if before == self.position {
                return Err(HarnessError::Json("invalid JSON exponent".into()));
            }
        }
        Ok(Value::Number(
            String::from_utf8(self.input[start..self.position].to_vec()).expect("number is ASCII"),
        ))
    }
    fn literal(&mut self, literal: &[u8], value: Value) -> Result<Value, HarnessError> {
        self.whitespace();
        if self.input.get(self.position..self.position + literal.len()) == Some(literal) {
            self.position += literal.len();
            Ok(value)
        } else {
            Err(HarnessError::Json("invalid JSON literal".into()))
        }
    }
}
