//! The argument of a string validator that looks for text: literal text, or
//! a [`Format`] whose pattern the validator anchors where it belongs.

use crate::schemasync::mockmake::format::Format;
use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use serde::{Deserialize, Serialize};

/// What `starts_with`, `ends_with` or `includes` looks for.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "TextPatternWire", into = "TextPatternWire")]
pub enum TextPattern {
    Text(String),
    Format(Format),
}

/// Where a validator anchors a format's pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Anchoring {
    Start,
    End,
    Anywhere,
}

impl Anchoring {
    /// `body` anchored here, for a regex engine.
    pub fn anchor(self, body: &str) -> String {
        match self {
            Anchoring::Start => format!("^(?:{body})"),
            Anchoring::End => format!("(?:{body})$"),
            Anchoring::Anywhere => format!("(?:{body})"),
        }
    }
}

impl TextPattern {
    /// The pattern looked for, anchored as `anchoring` says, and the
    /// JavaScript flags of a typesync-only custom pattern.
    pub fn regex(&self, anchoring: Anchoring) -> (String, Option<&str>) {
        match self {
            TextPattern::Text(text) => (anchoring.anchor(&regex::escape(text)), None),
            TextPattern::Format(Format::Custom(custom)) => {
                (anchoring.anchor(custom.as_str()), custom.flags())
            }
            TextPattern::Format(format) => (anchoring.anchor(&format.body()), None),
        }
    }

    /// What is looked for, in words.
    pub fn describe(&self) -> String {
        match self {
            TextPattern::Text(text) => format!("\"{text}\""),
            TextPattern::Format(format) => format.description(),
        }
    }
}

impl From<&str> for TextPattern {
    fn from(text: &str) -> Self {
        TextPattern::Text(text.to_owned())
    }
}

/// Text stays a plain string, as it was before formats were allowed; a format
/// is written under `Format`.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum TextPatternWire {
    Text(String),
    Format {
        #[serde(rename = "Format")]
        format: Format,
    },
}

impl From<TextPatternWire> for TextPattern {
    fn from(wire: TextPatternWire) -> Self {
        match wire {
            TextPatternWire::Text(text) => TextPattern::Text(text),
            TextPatternWire::Format { format } => TextPattern::Format(format),
        }
    }
}

impl From<TextPattern> for TextPatternWire {
    fn from(pattern: TextPattern) -> Self {
        match pattern {
            TextPattern::Text(text) => TextPatternWire::Text(text),
            TextPattern::Format(format) => TextPatternWire::Format { format },
        }
    }
}

/// A string literal is text; anything else names a format, as
/// `format(uppercase)` or `format(custom = "...")`.
impl TryFrom<&syn::Expr> for TextPattern {
    type Error = syn::Error;

    fn try_from(expr: &syn::Expr) -> Result<Self, Self::Error> {
        if let syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(literal),
            ..
        }) = expr
        {
            return Ok(TextPattern::Text(literal.value()));
        }
        Format::try_from(expr)
            .map(TextPattern::Format)
            .map_err(|error| {
                syn::Error::new_spanned(
                    expr,
                    format!(
                        "expected text in quotes or a format, such as `format(uppercase)`: {error}"
                    ),
                )
            })
    }
}

impl ToTokens for TextPattern {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.extend(match self {
            TextPattern::Text(text) => {
                quote! { ::evenframe::validator::text_pattern::TextPattern::Text(#text.to_owned()) }
            }
            TextPattern::Format(format) => {
                quote! { ::evenframe::validator::text_pattern::TextPattern::Format(#format) }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{Anchoring, TextPattern};
    use crate::schemasync::mockmake::format::Format;

    fn parse(text: &str) -> TextPattern {
        TextPattern::try_from(&syn::parse_str::<syn::Expr>(text).expect("an expression"))
            .expect("a text pattern")
    }

    #[test]
    fn text_and_formats_read_from_their_attribute_forms() {
        assert_eq!(parse(r#""ID-""#), TextPattern::Text("ID-".to_owned()));
        assert_eq!(
            parse("format(uppercase)"),
            TextPattern::Format(Format::Uppercase)
        );
        assert_eq!(parse("Format::Slug"), TextPattern::Format(Format::Slug));
    }

    #[test]
    fn a_format_is_anchored_where_the_validator_looks() {
        let uppercase = TextPattern::Format(Format::Uppercase);
        assert_eq!(uppercase.regex(Anchoring::Anywhere).0, "(?:[A-Z])");
        assert_eq!(uppercase.regex(Anchoring::Start).0, "^(?:[A-Z])");
        let text = TextPattern::Text("a.b".to_owned());
        assert_eq!(text.regex(Anchoring::End).0, r"(?:a\.b)$");
    }

    #[test]
    fn text_serializes_as_a_plain_string() {
        let text = TextPattern::Text("x".to_owned());
        assert_eq!(serde_json::to_value(&text).unwrap(), serde_json::json!("x"));
        let format = TextPattern::Format(Format::Digit);
        let written = serde_json::to_value(&format).unwrap();
        assert_eq!(written, serde_json::json!({ "Format": "Digit" }));
        assert_eq!(
            serde_json::from_value::<TextPattern>(written).unwrap(),
            format
        );
    }
}
