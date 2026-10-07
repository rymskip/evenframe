//! Morphs: steps that rewrite a value into canonical form on the way in and
//! keep its type, as `#[morphs(...)]` lists them. They run before the value's
//! validators, which check the result.

use super::bounds;
use super::keywords::{self, NormalForm};
use derive_more::From;
use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use serde::{Deserialize, Serialize};
use try_from_expr::TryFromExpr;

/// The largest number of decimal places `round` keeps: an f64 holds about 15
/// significant digits, so further places are noise.
pub const MAX_ROUND_PLACES: u32 = 15;

#[derive(Debug, Clone, PartialEq, From, Eq, Hash, TryFromExpr, Serialize, Deserialize)]
pub enum Morph {
    StringMorph(StringMorph),
    NumberMorph(NumberMorph),
    ArrayMorph(ArrayMorph),
}

/// A morph of a string into another string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TryFromExpr, Serialize, Deserialize)]
pub enum StringMorph {
    /// Removes leading and trailing whitespace, as JavaScript's `trim` does.
    Trim,
    /// Turns each run of whitespace into one space.
    CollapseWhitespace,
    Lower,
    Upper,
    /// Upper-cases the first character.
    Capitalize,
    /// NFC normalization, ArkType's `string.normalize`.
    Normalize,
    NormalizeNfc,
    NormalizeNfd,
    NormalizeNfkc,
    NormalizeNfkd,
}

/// A morph of a number, bigint or decimal into one of the same type.
#[derive(Debug, Clone, PartialEq, Eq, Hash, TryFromExpr, Serialize, Deserialize)]
pub enum NumberMorph {
    /// Rounds to this many decimal places, halves toward positive infinity as
    /// JavaScript's `Math.round` does.
    Round(u32),
    /// Pulls the value into the range, its bounds written as decimals so a
    /// bigint keeps every digit.
    Clamp(String, String),
}

/// A morph of an array into one of the same element type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TryFromExpr, Serialize, Deserialize)]
pub enum ArrayMorph {
    /// Orders the elements ascending.
    Sort,
    /// Drops every element equal to an earlier one, in O(n log n).
    Unique,
}

impl StringMorph {
    pub fn apply(self, value: &str) -> String {
        match self {
            StringMorph::Trim => keywords::trim(value).to_owned(),
            StringMorph::CollapseWhitespace => keywords::collapse_whitespace(value),
            StringMorph::Lower => value.to_lowercase(),
            StringMorph::Upper => value.to_uppercase(),
            StringMorph::Capitalize => keywords::capitalize(value),
            StringMorph::Normalize | StringMorph::NormalizeNfc => {
                keywords::normalize(value, NormalForm::Nfc)
            }
            StringMorph::NormalizeNfd => keywords::normalize(value, NormalForm::Nfd),
            StringMorph::NormalizeNfkc => keywords::normalize(value, NormalForm::Nfkc),
            StringMorph::NormalizeNfkd => keywords::normalize(value, NormalForm::Nfkd),
        }
    }

    /// The Unicode form a normalize morph produces.
    pub fn normal_form(self) -> Option<NormalForm> {
        match self {
            StringMorph::Normalize | StringMorph::NormalizeNfc => Some(NormalForm::Nfc),
            StringMorph::NormalizeNfd => Some(NormalForm::Nfd),
            StringMorph::NormalizeNfkc => Some(NormalForm::Nfkc),
            StringMorph::NormalizeNfkd => Some(NormalForm::Nfkd),
            StringMorph::Trim
            | StringMorph::CollapseWhitespace
            | StringMorph::Lower
            | StringMorph::Upper
            | StringMorph::Capitalize => None,
        }
    }

    /// The ArkType keyword this morph is named after, when it is one.
    pub fn arktype_keyword(self) -> Option<&'static str> {
        Some(match self {
            StringMorph::Trim => "string.trim",
            StringMorph::Lower => "string.lower",
            StringMorph::Upper => "string.upper",
            StringMorph::Capitalize => "string.capitalize",
            StringMorph::Normalize => "string.normalize",
            StringMorph::NormalizeNfc => "string.normalize.NFC",
            StringMorph::NormalizeNfd => "string.normalize.NFD",
            StringMorph::NormalizeNfkc => "string.normalize.NFKC",
            StringMorph::NormalizeNfkd => "string.normalize.NFKD",
            StringMorph::CollapseWhitespace => return None,
        })
    }

    /// The JavaScript expression this morph rewrites `value` to.
    pub fn javascript(self) -> String {
        match self {
            StringMorph::Trim => "value.trim()".to_owned(),
            StringMorph::CollapseWhitespace => r#"value.replace(/\s+/g, " ")"#.to_owned(),
            StringMorph::Lower => "value.toLowerCase()".to_owned(),
            StringMorph::Upper => "value.toUpperCase()".to_owned(),
            StringMorph::Capitalize => {
                "value.length === 0 ? value : value[0].toUpperCase() + value.slice(1)".to_owned()
            }
            StringMorph::Normalize
            | StringMorph::NormalizeNfc
            | StringMorph::NormalizeNfd
            | StringMorph::NormalizeNfkc
            | StringMorph::NormalizeNfkd => {
                let form = self.normal_form().map_or("NFC", NormalForm::name);
                format!("value.normalize(\"{form}\")")
            }
        }
    }
}

impl NumberMorph {
    /// Rejects places beyond [`MAX_ROUND_PLACES`] and bounds that are not
    /// decimals or are out of order.
    pub fn check_bounds(&self) -> Result<(), String> {
        match self {
            NumberMorph::Round(places) if *places > MAX_ROUND_PLACES => Err(format!(
                "round keeps at most {MAX_ROUND_PLACES} decimal places, not {places}"
            )),
            NumberMorph::Round(_) => Ok(()),
            NumberMorph::Clamp(min, max) => {
                if bounds::decimal(min)? > bounds::decimal(max)? {
                    return Err(format!("clamp's minimum {min} is above its maximum {max}"));
                }
                Ok(())
            }
        }
    }

    /// `10^places`, the factor `round` scales by.
    pub fn round_factor(places: u32) -> f64 {
        10f64.powi(i32::try_from(places).unwrap_or(i32::MAX))
    }
}

/// JavaScript's `Math.round`: the nearest integer, halves toward positive
/// infinity.
pub fn javascript_round(value: f64) -> f64 {
    let floor = value.floor();
    if value - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
    }
}

/// `value` rounded to `places`, as `Math.round(value * 10 ** places) / 10 ** places`.
pub fn round_to(value: f64, places: u32) -> f64 {
    let factor = NumberMorph::round_factor(places);
    javascript_round(value * factor) / factor
}

impl Morph {
    /// This morph as an endec `normalize` step.
    pub fn endec_step(&self) -> Result<String, String> {
        Ok(match self {
            Morph::StringMorph(morph) => match morph {
                StringMorph::Trim => "trim".to_owned(),
                StringMorph::CollapseWhitespace => "collapseWhitespace".to_owned(),
                StringMorph::Lower => "lowercase".to_owned(),
                StringMorph::Upper => "uppercase".to_owned(),
                StringMorph::Capitalize => "capitalize".to_owned(),
                StringMorph::Normalize
                | StringMorph::NormalizeNfc
                | StringMorph::NormalizeNfd
                | StringMorph::NormalizeNfkc
                | StringMorph::NormalizeNfkd => format!(
                    "normalizeUnicode(\"{}\")",
                    morph.normal_form().map_or("NFC", NormalForm::name)
                ),
            },
            Morph::NumberMorph(NumberMorph::Round(places)) => format!("round({places})"),
            Morph::NumberMorph(NumberMorph::Clamp(min, max)) => {
                format!(
                    "clamp({}, {})",
                    bounds::decimal(min)?,
                    bounds::decimal(max)?
                )
            }
            Morph::ArrayMorph(ArrayMorph::Sort) => "sort".to_owned(),
            Morph::ArrayMorph(ArrayMorph::Unique) => "unique".to_owned(),
        })
    }

    pub fn check_bounds(&self) -> Result<(), String> {
        match self {
            Morph::NumberMorph(morph) => morph.check_bounds(),
            Morph::StringMorph(_) | Morph::ArrayMorph(_) => Ok(()),
        }
    }

    /// This morph's runtime step on `place`, a mutable place holding the
    /// value, as an expression of type `Result<(), runtime::Rejection>`.
    pub fn runtime_step(&self, place: &TokenStream) -> Result<TokenStream, String> {
        self.check_bounds()?;
        let call = match self {
            Morph::StringMorph(morph) => quote! { morph_string(&mut #place, #morph) },
            Morph::NumberMorph(NumberMorph::Round(places)) => {
                quote! { round_number(&mut #place, #places) }
            }
            Morph::NumberMorph(NumberMorph::Clamp(min, max)) => {
                quote! { clamp_number(&mut #place, #min, #max) }
            }
            Morph::ArrayMorph(ArrayMorph::Sort) => quote! { sort_items(&mut #place) },
            Morph::ArrayMorph(ArrayMorph::Unique) => quote! { unique_items(&mut #place) },
        };
        Ok(quote! { ::evenframe::validator::runtime::#call })
    }

    /// This morph in words, for messages.
    pub fn describe(&self) -> String {
        match self {
            Morph::StringMorph(morph) => format!("{morph:?}"),
            Morph::NumberMorph(NumberMorph::Round(places)) => {
                format!("rounded to {places} decimal places")
            }
            Morph::NumberMorph(NumberMorph::Clamp(min, max)) => {
                format!("clamped to {min} through {max}")
            }
            Morph::ArrayMorph(ArrayMorph::Sort) => "sorted".to_owned(),
            Morph::ArrayMorph(ArrayMorph::Unique) => "without duplicates".to_owned(),
        }
    }
}

impl ToTokens for Morph {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.extend(match self {
            Morph::StringMorph(morph) => {
                quote! { ::evenframe::validator::morph::Morph::StringMorph(#morph) }
            }
            Morph::NumberMorph(morph) => {
                quote! { ::evenframe::validator::morph::Morph::NumberMorph(#morph) }
            }
            Morph::ArrayMorph(morph) => {
                quote! { ::evenframe::validator::morph::Morph::ArrayMorph(#morph) }
            }
        });
    }
}

impl ToTokens for StringMorph {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let variant = match self {
            StringMorph::Trim => quote! { Trim },
            StringMorph::CollapseWhitespace => quote! { CollapseWhitespace },
            StringMorph::Lower => quote! { Lower },
            StringMorph::Upper => quote! { Upper },
            StringMorph::Capitalize => quote! { Capitalize },
            StringMorph::Normalize => quote! { Normalize },
            StringMorph::NormalizeNfc => quote! { NormalizeNfc },
            StringMorph::NormalizeNfd => quote! { NormalizeNfd },
            StringMorph::NormalizeNfkc => quote! { NormalizeNfkc },
            StringMorph::NormalizeNfkd => quote! { NormalizeNfkd },
        };
        tokens.extend(quote! { ::evenframe::validator::morph::StringMorph::#variant });
    }
}

impl ToTokens for NumberMorph {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.extend(match self {
            NumberMorph::Round(places) => {
                quote! { ::evenframe::validator::morph::NumberMorph::Round(#places) }
            }
            NumberMorph::Clamp(min, max) => quote! {
                ::evenframe::validator::morph::NumberMorph::Clamp(#min.to_owned(), #max.to_owned())
            },
        });
    }
}

impl ToTokens for ArrayMorph {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.extend(match self {
            ArrayMorph::Sort => quote! { ::evenframe::validator::morph::ArrayMorph::Sort },
            ArrayMorph::Unique => quote! { ::evenframe::validator::morph::ArrayMorph::Unique },
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{Morph, NumberMorph, StringMorph, javascript_round, round_to};

    #[test]
    fn morphs_parse_from_their_attribute_names() {
        let parse = |text: &str| {
            Morph::try_from(&syn::parse_str::<syn::Expr>(text).expect("an expression"))
                .expect("a morph")
        };
        assert_eq!(parse("trim"), Morph::StringMorph(StringMorph::Trim));
        assert_eq!(
            parse("collapse_whitespace"),
            Morph::StringMorph(StringMorph::CollapseWhitespace)
        );
        assert_eq!(
            parse("round = 2"),
            Morph::NumberMorph(NumberMorph::Round(2))
        );
        assert_eq!(
            parse(r#"clamp = ("0", "100")"#),
            Morph::NumberMorph(NumberMorph::Clamp("0".to_owned(), "100".to_owned()))
        );
    }

    #[test]
    fn rounding_follows_javascript() {
        assert_eq!(javascript_round(2.5), 3.0);
        assert_eq!(javascript_round(-2.5), -2.0);
        assert_eq!(javascript_round(-2.6), -3.0);
        assert_eq!(round_to(1.005, 2), 1.0);
        assert_eq!(round_to(1.236, 2), 1.24);
    }

    #[test]
    fn string_morphs_rewrite_as_javascript_does() {
        assert_eq!(
            StringMorph::CollapseWhitespace.apply("a \t\n b\u{3000}c"),
            "a b c"
        );
        assert_eq!(StringMorph::Trim.apply("\u{feff} a "), "a");
        assert_eq!(StringMorph::Capitalize.apply("élan"), "Élan");
    }

    #[test]
    fn bounds_are_checked() {
        assert!(
            NumberMorph::Clamp("10".to_owned(), "1".to_owned())
                .check_bounds()
                .is_err()
        );
        assert!(
            NumberMorph::Clamp("x".to_owned(), "1".to_owned())
                .check_bounds()
                .is_err()
        );
        assert!(NumberMorph::Round(16).check_bounds().is_err());
        assert!(NumberMorph::Round(2).check_bounds().is_ok());
    }
}
