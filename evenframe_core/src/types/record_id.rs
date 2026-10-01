//! Parses a record id written as SurrealQL text, `table:key`, into the SDK's
//! [`RecordId`], keeping the key's type: `user:42` has a number key, while
//! `` user:`42` `` and `user:⟨42⟩` have string keys.

use std::iter::Peekable;
use std::str::Chars;
use surrealdb_types::{RecordId, RecordIdKey, Uuid};

/// Why a string is not a record id.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{input:?} is not a `table:key` record id: {reason}")]
pub struct RecordIdParseError {
    input: String,
    reason: String,
}

/// The record id `input` writes. A table or key holding characters other than
/// ASCII letters, digits and `_` is escaped in backticks or `⟨⟩`. Array,
/// object and range keys have no string form here; they are read from the
/// record id's object form instead.
pub fn parse_record_id(input: &str) -> Result<RecordId, RecordIdParseError> {
    let fail = |reason: &str| RecordIdParseError {
        input: input.to_string(),
        reason: reason.to_string(),
    };
    let mut chars = input.chars().peekable();
    let table = match chars.peek() {
        Some('`') | Some('⟨') => escaped(&mut chars).map_err(|reason| fail(&reason))?,
        _ => {
            let table = identifier(&mut chars);
            if table.is_empty() {
                return Err(fail("it has no table"));
            }
            if table.starts_with(|first: char| first.is_ascii_digit()) {
                return Err(fail("a table starting with a digit must be escaped"));
            }
            table
        }
    };
    if chars.next() != Some(':') {
        return Err(fail("the table is not followed by `:`"));
    }
    let key = match chars.peek().copied() {
        None => return Err(fail("it has no key")),
        Some('`') | Some('⟨') => {
            RecordIdKey::String(escaped(&mut chars).map_err(|reason| fail(&reason))?)
        }
        Some('[') | Some('{') => {
            return Err(fail(
                "array and object keys are only read from the record id's object form",
            ));
        }
        Some('u') if opens_uuid(&chars) => {
            RecordIdKey::Uuid(uuid_literal(&mut chars).map_err(|reason| fail(&reason))?)
        }
        Some(_) => number_or_identifier(&mut chars).map_err(|reason| fail(&reason))?,
    };
    if let Some(rest) = Some(chars.collect::<String>()).filter(|rest| !rest.is_empty()) {
        let reason = if rest.starts_with("..") || rest.starts_with('>') {
            "range keys are only read from the record id's object form".to_string()
        } else {
            format!("unexpected {rest:?} after the key")
        };
        return Err(fail(&reason));
    }
    Ok(RecordId::new(table, key))
}

/// Whether `chars` open a `u'…'` or `u"…"` uuid.
fn opens_uuid(chars: &Peekable<Chars<'_>>) -> bool {
    let opening: String = chars.clone().take(2).collect();
    opening == "u'" || opening == "u\""
}

/// The uuid of the `u'…'` or `u"…"` literal at the front of `chars`.
fn uuid_literal(chars: &mut Peekable<Chars<'_>>) -> Result<Uuid, String> {
    chars.next();
    let quote = chars
        .next()
        .ok_or_else(|| "the uuid is not quoted".to_string())?;
    let mut text = String::new();
    loop {
        match chars.next() {
            Some(next) if next == quote => break,
            Some(next) => text.push(next),
            None => return Err(format!("the uuid is not closed with {quote}")),
        }
    }
    uuid::Uuid::parse_str(&text)
        .map(Uuid::from)
        .map_err(|error| format!("the uuid key is invalid: {error}"))
}

/// The ASCII letters, digits and `_` at the front of `chars`.
fn identifier(chars: &mut Peekable<Chars<'_>>) -> String {
    let mut text = String::new();
    while let Some(next) = chars.next_if(|next| next.is_ascii_alphanumeric() || *next == '_') {
        text.push(next);
    }
    text
}

/// A number key, or a string key written without escaping. As in SurrealQL,
/// digits are a number unless they are followed by identifier characters or
/// overflow an `i64`, which leaves them a string.
fn number_or_identifier(chars: &mut Peekable<Chars<'_>>) -> Result<RecordIdKey, String> {
    let sign = chars.next_if(|next| *next == '-' || *next == '+');
    let text = identifier(chars);
    if text.is_empty() {
        return Err("the key is empty".to_string());
    }
    let digits_only = text.chars().all(|next| next.is_ascii_digit());
    match sign {
        Some(sign) if !digits_only => Err(format!("`{sign}` must be followed by digits")),
        Some('-') => Ok(format!("-{text}").parse::<i64>().map_or_else(
            |_| RecordIdKey::String(format!("-{text}")),
            RecordIdKey::Number,
        )),
        _ if digits_only => Ok(text
            .parse::<i64>()
            .map_or_else(|_| RecordIdKey::String(text.clone()), RecordIdKey::Number)),
        _ => Ok(RecordIdKey::String(text)),
    }
}

/// The text inside a `` `…` `` or `⟨…⟩` escape at the front of `chars`, with
/// its escape sequences decoded.
fn escaped(chars: &mut Peekable<Chars<'_>>) -> Result<String, String> {
    let close = match chars.next() {
        Some('`') => '`',
        Some('⟨') => '⟩',
        _ => return Err("an escaped part must open with ` or ⟨".to_string()),
    };
    let mut text = String::new();
    loop {
        match chars.next() {
            None => return Err(format!("an escaped part is not closed with {close}")),
            Some(next) if next == close => return Ok(text),
            Some('\\') => text.push(escape_sequence(chars)?),
            Some(next) => text.push(next),
        }
    }
}

/// The character an escape sequence after its `\` stands for.
fn escape_sequence(chars: &mut Peekable<Chars<'_>>) -> Result<char, String> {
    Ok(match chars.next() {
        Some('n') => '\n',
        Some('r') => '\r',
        Some('t') => '\t',
        Some('0') => '\0',
        Some('f') => '\x0C',
        Some('b') => '\x08',
        Some('u') => {
            if chars.next() != Some('{') {
                return Err("a `\\u` escape must be written `\\u{…}`".to_string());
            }
            let hex: String = chars.by_ref().take_while(|next| *next != '}').collect();
            u32::from_str_radix(&hex, 16)
                .ok()
                .and_then(char::from_u32)
                .ok_or_else(|| format!("`\\u{{{hex}}}` is not a character"))?
        }
        Some(other) => other,
        None => return Err("the input ends inside an escape sequence".to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::{RecordId, RecordIdKey, Uuid, parse_record_id};
    use surrealdb_types::ToSql;

    fn parsed(input: &str) -> RecordId {
        parse_record_id(input).unwrap_or_else(|error| panic!("{error}"))
    }

    #[test]
    fn keys_keep_their_type() {
        assert_eq!(parsed("user:42"), RecordId::new("user", 42_i64));
        assert_eq!(parsed("user:-7"), RecordId::new("user", -7_i64));
        assert_eq!(parsed("user:+7"), RecordId::new("user", 7_i64));
        assert_eq!(parsed("user:ada"), RecordId::new("user", "ada"));
        assert_eq!(parsed("user:42abc"), RecordId::new("user", "42abc"));
        assert_eq!(parsed("user:`42`"), RecordId::new("user", "42"));
        assert_eq!(parsed("user:⟨42⟩"), RecordId::new("user", "42"));
        assert_eq!(
            parsed("user:99999999999999999999"),
            RecordId::new("user", "99999999999999999999")
        );
        let uuid = uuid::Uuid::parse_str("0190d9df-a1b2-7c3d-8e4f-5a6b7c8d9e0f").expect("uuid");
        assert_eq!(
            parsed("user:u'0190d9df-a1b2-7c3d-8e4f-5a6b7c8d9e0f'"),
            RecordId::new("user", RecordIdKey::Uuid(Uuid::from(uuid)))
        );
    }

    #[test]
    fn escapes_are_decoded() {
        assert_eq!(
            parsed("`order line`:`a\\`b\\\\c`"),
            RecordId::new("order line", "a`b\\c")
        );
        assert_eq!(parsed("user:⟨a\\⟩b⟩"), RecordId::new("user", "a⟩b"));
        assert_eq!(
            parsed("user:`tab\\there`"),
            RecordId::new("user", "tab\there")
        );
        assert_eq!(parsed("user:`\\u{e9}`"), RecordId::new("user", "é"));
    }

    #[test]
    fn the_sdk_text_form_round_trips() {
        for id in [
            RecordId::new("user", 1_i64),
            RecordId::new("user", "1"),
            RecordId::new("user", "with space"),
            RecordId::new("user", "tick`and\\slash"),
            RecordId::new("2fa", "code"),
        ] {
            assert_eq!(parsed(&id.to_sql()), id, "{}", id.to_sql());
        }
    }

    #[test]
    fn malformed_ids_are_rejected() {
        for input in [
            "",
            "user",
            "user:",
            ":42",
            "1user:42",
            "user:`open",
            "user:42 trailing",
            "user:[1, 2]",
            "user:{ a: 1 }",
            "user:1..5",
            "user:-abc",
        ] {
            assert!(parse_record_id(input).is_err(), "{input:?} parsed");
        }
    }
}
