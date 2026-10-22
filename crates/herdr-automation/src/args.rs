//! Reading a script's arguments into typed values. Every refusal names the
//! function and field, so `check` can say exactly what to fix.

use crate::{Problem, Refusal, schedule};
use mlua::{Table, Value};
use std::time::Duration;

/// Where an argument came from, for the refusal's message.
#[derive(Clone, Copy)]
pub(crate) struct Site {
    pub(crate) function: &'static str,
}

impl Site {
    pub(crate) fn refuse(self, problem: Problem) -> mlua::Error {
        mlua::Error::external(Refusal::Invalid {
            function: self.function,
            problem,
        })
    }

    /// Refuses any key of `table` that is not in `allowed`.
    pub(crate) fn only(self, table: &Table, allowed: &[&str]) -> mlua::Result<()> {
        for pair in table.pairs::<Value, Value>() {
            let (key, _) = pair?;
            let known = match &key {
                Value::String(name) => allowed
                    .iter()
                    .any(|field| name.as_bytes() == field.as_bytes()),
                _ => false,
            };
            if !known {
                let name = match key {
                    Value::String(name) => name.to_string_lossy(),
                    other => other.type_name().to_owned(),
                };
                return Err(self.refuse(Problem::Unknown(name)));
            }
        }
        Ok(())
    }

    pub(crate) fn text(self, value: Value, rule: Text) -> mlua::Result<String> {
        let Value::String(text) = value else {
            return Err(self.refuse(Problem::Type {
                field: rule.field,
                expected: "a string",
            }));
        };
        let text = text.to_str()?.to_owned();
        rule.check(self, &text)?;
        Ok(text)
    }

    pub(crate) fn required_text(self, table: &Table, rule: Text) -> mlua::Result<String> {
        match table.raw_get::<Value>(rule.field)? {
            Value::Nil => Err(self.refuse(Problem::Missing(rule.field))),
            value => self.text(value, rule),
        }
    }

    pub(crate) fn optional_text(self, table: &Table, rule: Text) -> mlua::Result<Option<String>> {
        match table.raw_get::<Value>(rule.field)? {
            Value::Nil => Ok(None),
            value => self.text(value, rule).map(Some),
        }
    }

    pub(crate) fn optional_bool(
        self,
        table: &Table,
        field: &'static str,
    ) -> mlua::Result<Option<bool>> {
        match table.raw_get::<Value>(field)? {
            Value::Nil => Ok(None),
            Value::Boolean(value) => Ok(Some(value)),
            _ => Err(self.refuse(Problem::Type {
                field,
                expected: "true or false",
            })),
        }
    }

    /// A whole number in `min..=max`, from a Luau number.
    pub(crate) fn whole(
        self,
        table: &Table,
        field: &'static str,
        min: u64,
        max: u64,
        range: &'static str,
    ) -> mlua::Result<u64> {
        let number = match table.raw_get::<Value>(field)? {
            Value::Nil => return Err(self.refuse(Problem::Missing(field))),
            Value::Integer(value) => value as f64,
            Value::Number(value) => value,
            _ => {
                return Err(self.refuse(Problem::Type {
                    field,
                    expected: "a number",
                }));
            }
        };
        if number.fract() != 0. || number < min as f64 || number > max as f64 {
            return Err(self.refuse(Problem::Range { field, range }));
        }
        Ok(number as u64)
    }

    /// A duration such as `30m`, between `min` and `max`.
    pub(crate) fn duration(
        self,
        value: Value,
        field: &'static str,
        min: Duration,
        max: Duration,
        range: &'static str,
    ) -> mlua::Result<Duration> {
        let text = self.text(value, Text::name(field))?;
        let duration =
            schedule::parse_duration(&text).ok_or_else(|| self.refuse(Problem::Duration(text)))?;
        if duration < min || duration > max {
            return Err(self.refuse(Problem::Range { field, range }));
        }
        Ok(duration)
    }

    pub(crate) fn optional_duration(
        self,
        table: &Table,
        field: &'static str,
        min: Duration,
        max: Duration,
        range: &'static str,
    ) -> mlua::Result<Option<Duration>> {
        match table.raw_get::<Value>(field)? {
            Value::Nil => Ok(None),
            value => self.duration(value, field, min, max, range).map(Some),
        }
    }
}

/// How a text argument is bounded.
#[derive(Clone, Copy)]
pub(crate) struct Text {
    pub(crate) field: &'static str,
    limit: usize,
    multiline: bool,
}

impl Text {
    /// A one-line name: a branch, a script name, a harness.
    pub(crate) const fn name(field: &'static str) -> Self {
        Self {
            field,
            limit: 200,
            multiline: false,
        }
    }

    /// Free text that may span lines: a prompt, a command.
    pub(crate) const fn body(field: &'static str, limit: usize) -> Self {
        Self {
            field,
            limit,
            multiline: true,
        }
    }

    fn check(self, site: Site, text: &str) -> mlua::Result<()> {
        if text.trim().is_empty() {
            return Err(site.refuse(Problem::Empty(self.field)));
        }
        if text.len() > self.limit {
            return Err(site.refuse(Problem::TooLong {
                field: self.field,
                limit: self.limit,
            }));
        }
        let allowed = |c: char| self.multiline && matches!(c, '\n' | '\t');
        if text.chars().any(|c| c.is_control() && !allowed(c)) {
            return Err(site.refuse(Problem::Control(self.field)));
        }
        if text.chars().any(invisible) {
            return Err(site.refuse(Problem::Format(self.field)));
        }
        Ok(())
    }
}

/// Characters that change how text reads without showing: bidi controls,
/// zero-width characters and the like. A plan must read as what runs.
fn invisible(c: char) -> bool {
    matches!(
        c,
        '\u{00ad}'
            | '\u{061c}'
            | '\u{180e}'
            | '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206f}'
            | '\u{feff}'
            | '\u{fff9}'..='\u{fffb}'
    )
}
