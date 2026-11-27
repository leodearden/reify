//! An expected value in the catalogue's answer notation (`8 mm`, `120 mm^2`,
//! `true`, `undefined`, `Plate{t: 3 mm}`, `[...]`), parsed when an entry
//! loads into a type of the runner's own: no engine type appears in an
//! expectation (docs/prds/instances-on-the-core.md sections 5.8 and 15).
//! How a value is compared with an answer is the first comparing leaf's.

use serde::Deserialize;

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(try_from = "String")]
pub enum ExpectedValue {
    Undefined,
    Bool(bool),
    Number {
        magnitude: f64,
        unit: Option<String>,
    },
    List(Vec<ExpectedValue>),
    Structure {
        type_name: String,
        fields: Vec<(String, ExpectedValue)>,
    },
}

impl TryFrom<String> for ExpectedValue {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        parse(&text)
    }
}

/// Parses one value, which must span the whole text.
pub fn parse(text: &str) -> Result<ExpectedValue, String> {
    let mut cursor = Cursor { text, at: 0 };
    let value = cursor.value()?;
    cursor.skip_spaces();
    if cursor.at != text.len() {
        return Err(cursor.fault("trailing text"));
    }
    Ok(value)
}

struct Cursor<'t> {
    text: &'t str,
    at: usize,
}

impl Cursor<'_> {
    fn rest(&self) -> &str {
        &self.text[self.at..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn skip_spaces(&mut self) {
        while self.peek() == Some(' ') {
            self.at += 1;
        }
    }

    fn fault(&self, what: &str) -> String {
        format!(
            "expected value `{}`: {what} at `{}`",
            self.text,
            self.rest()
        )
    }

    fn eat(&mut self, expected: char) -> Result<(), String> {
        self.skip_spaces();
        if self.peek() == Some(expected) {
            self.at += expected.len_utf8();
            Ok(())
        } else {
            Err(self.fault(&format!("expected `{expected}`")))
        }
    }

    fn take_while(&mut self, accept: impl Fn(char) -> bool) -> &str {
        let start = self.at;
        while self.peek().is_some_and(&accept) {
            self.at += 1;
        }
        &self.text[start..self.at]
    }

    fn value(&mut self) -> Result<ExpectedValue, String> {
        self.skip_spaces();
        match self.peek() {
            Some('[') => self.list(),
            Some(c) if c == '-' || c.is_ascii_digit() => self.number(),
            Some(c) if c.is_ascii_alphabetic() => self.word_led(),
            _ => Err(self.fault("expected a value")),
        }
    }

    fn list(&mut self) -> Result<ExpectedValue, String> {
        self.eat('[')?;
        let mut items = Vec::new();
        self.skip_spaces();
        if self.peek() == Some(']') {
            self.at += 1;
            return Ok(ExpectedValue::List(items));
        }
        loop {
            items.push(self.value()?);
            self.skip_spaces();
            match self.peek() {
                Some(',') => self.at += 1,
                Some(']') => {
                    self.at += 1;
                    return Ok(ExpectedValue::List(items));
                }
                _ => return Err(self.fault("expected `,` or `]`")),
            }
        }
    }

    fn number(&mut self) -> Result<ExpectedValue, String> {
        let start = self.at;
        if self.peek() == Some('-') {
            self.at += 1;
        }
        if self.take_while(|c| c.is_ascii_digit()).is_empty() {
            return Err(self.fault("expected digits"));
        }
        if self.peek() == Some('.') {
            self.at += 1;
            if self.take_while(|c| c.is_ascii_digit()).is_empty() {
                return Err(self.fault("expected digits after `.`"));
            }
        }
        let magnitude: f64 = self.text[start..self.at]
            .parse()
            .map_err(|_| self.fault("not a number"))?;
        let unit = self.unit()?;
        Ok(ExpectedValue::Number { magnitude, unit })
    }

    fn unit(&mut self) -> Result<Option<String>, String> {
        let before = self.at;
        self.skip_spaces();
        let name = self.take_while(|c| c.is_ascii_alphabetic()).to_string();
        if name.is_empty() {
            self.at = before;
            return Ok(None);
        }
        if self.peek() != Some('^') {
            return Ok(Some(name));
        }
        self.at += 1;
        let power_start = self.at;
        if self.peek() == Some('-') {
            self.at += 1;
        }
        if self.take_while(|c| c.is_ascii_digit()).is_empty() {
            return Err(self.fault("expected a power after `^`"));
        }
        Ok(Some(format!("{name}^{}", &self.text[power_start..self.at])))
    }

    fn word_led(&mut self) -> Result<ExpectedValue, String> {
        let word = self
            .take_while(|c| c.is_ascii_alphanumeric() || c == '_')
            .to_string();
        match word.as_str() {
            "undefined" | "undef" => Ok(ExpectedValue::Undefined),
            "true" => Ok(ExpectedValue::Bool(true)),
            "false" => Ok(ExpectedValue::Bool(false)),
            _ => self.structure(word),
        }
    }

    fn structure(&mut self, type_name: String) -> Result<ExpectedValue, String> {
        self.eat('{')?;
        let mut fields = Vec::new();
        loop {
            self.skip_spaces();
            let field = self
                .take_while(|c| c.is_ascii_alphanumeric() || c == '_')
                .to_string();
            if field.is_empty() {
                return Err(self.fault("expected a field name"));
            }
            self.eat(':')?;
            fields.push((field, self.value()?));
            self.skip_spaces();
            match self.peek() {
                Some(',') => self.at += 1,
                Some('}') => {
                    self.at += 1;
                    return Ok(ExpectedValue::Structure { type_name, fields });
                }
                _ => return Err(self.fault("expected `,` or `}`")),
            }
        }
    }
}

#[test]
fn a_malformed_value_is_refused_naming_its_text() {
    let refusals = [
        ("8 mm x", "trailing text"),
        ("Plate{t 3}", "expected `:`"),
        ("Plate{t: 3 mm", "expected `,` or `}`"),
        ("[1, 2", "expected `,` or `]`"),
        ("3 mm^", "expected a power after `^`"),
        ("-", "expected digits"),
        ("1.", "expected digits after `.`"),
        ("", "expected a value"),
    ];
    for (text, what) in refusals {
        match parse(text) {
            Ok(value) => panic!("`{text}` must be refused, but parsed as {value:?}"),
            Err(error) => assert!(
                error.contains(&format!("`{text}`")) && error.contains(what),
                "the refusal of `{text}` must name the text and say {what:?}, got: {error}"
            ),
        }
    }
}
