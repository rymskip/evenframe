//! Checking a value against its fields' validators, reporting every field
//! that fails rather than the first.

use std::collections::{BTreeMap, HashMap};
use std::fmt;

/// One field that failed a validator, at its path in serde's names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    /// `email`, `address.city`, `items[2].name` or `tags["key"]`.
    pub path: String,
    pub message: String,
}

impl fmt::Display for FieldError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path, self.message)
    }
}

/// Every field of a value that failed its validators.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidationErrors {
    errors: Vec<FieldError>,
}

impl ValidationErrors {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn errors(&self) -> &[FieldError] {
        &self.errors
    }

    pub fn push(&mut self, path: impl Into<String>, message: impl fmt::Display) {
        self.errors.push(FieldError {
            path: path.into(),
            message: message.to_string(),
        });
    }

    /// Adds `nested`'s failures under `prefix`, the path of the value that
    /// holds them.
    pub fn nest(&mut self, prefix: &str, nested: ValidationErrors) {
        self.errors
            .extend(nested.errors.into_iter().map(|error| FieldError {
                path: if error.path.starts_with('[') {
                    format!("{prefix}{}", error.path)
                } else {
                    format!("{prefix}.{}", error.path)
                },
                message: error.message,
            }));
    }

    /// `Ok` when nothing failed.
    pub fn into_result(self) -> Result<(), Self> {
        if self.is_empty() { Ok(()) } else { Err(self) }
    }
}

impl fmt::Display for ValidationErrors {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (position, error) in self.errors.iter().enumerate() {
            if position > 0 {
                formatter.write_str("; ")?;
            }
            write!(formatter, "{error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ValidationErrors {}

/// A value whose fields carry validators. The `Evenframe` derive implements
/// it, checking every field and every value nested in one. A field whose
/// type is a generic parameter is not descended into.
pub trait Validate {
    /// Checks every field against its validators, reporting each that fails.
    fn validate(&self) -> Result<(), ValidationErrors>;
}

impl<T: Validate + ?Sized> Validate for Box<T> {
    fn validate(&self) -> Result<(), ValidationErrors> {
        (**self).validate()
    }
}

impl<T: Validate> Validate for Option<T> {
    fn validate(&self) -> Result<(), ValidationErrors> {
        self.as_ref().map_or(Ok(()), Validate::validate)
    }
}

impl<T: Validate> Validate for [T] {
    fn validate(&self) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();
        for (position, item) in self.iter().enumerate() {
            if let Err(nested) = item.validate() {
                errors.nest(&format!("[{position}]"), nested);
            }
        }
        errors.into_result()
    }
}

impl<T: Validate> Validate for Vec<T> {
    fn validate(&self) -> Result<(), ValidationErrors> {
        self.as_slice().validate()
    }
}

/// The entries of a map, each under its key.
fn validate_entries<'a, K: fmt::Debug + 'a, V: Validate + 'a>(
    entries: impl Iterator<Item = (&'a K, &'a V)>,
) -> Result<(), ValidationErrors> {
    let mut errors = ValidationErrors::new();
    for (key, value) in entries {
        if let Err(nested) = value.validate() {
            errors.nest(&format!("[{key:?}]"), nested);
        }
    }
    errors.into_result()
}

impl<K: fmt::Debug, V: Validate, S> Validate for HashMap<K, V, S> {
    fn validate(&self) -> Result<(), ValidationErrors> {
        validate_entries(self.iter())
    }
}

impl<K: fmt::Debug, V: Validate> Validate for BTreeMap<K, V> {
    fn validate(&self) -> Result<(), ValidationErrors> {
        validate_entries(self.iter())
    }
}

/// What the derive's generated code reaches for: validating a field's value
/// when its type implements [`Validate`], and nothing otherwise, which a
/// generic derive cannot tell apart on its own. `(&Probe(value)).nested()`
/// finds the impl on `Probe` before the one on `&Probe` that needs another
/// borrow, and that first impl applies only to a `Validate` type.
#[doc(hidden)]
pub mod __private {
    use super::{Validate, ValidationErrors};

    pub struct Probe<'a, T: ?Sized>(pub &'a T);

    pub trait Nested {
        fn nested(&self) -> Result<(), ValidationErrors>;
    }

    impl<T: Validate + ?Sized> Nested for Probe<'_, T> {
        fn nested(&self) -> Result<(), ValidationErrors> {
            self.0.validate()
        }
    }

    impl<T: ?Sized> Nested for &Probe<'_, T> {
        fn nested(&self) -> Result<(), ValidationErrors> {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Validate, ValidationErrors};
    use std::collections::BTreeMap;

    struct Named(&'static str);

    impl Validate for Named {
        fn validate(&self) -> Result<(), ValidationErrors> {
            let mut errors = ValidationErrors::new();
            if self.0.is_empty() {
                errors.push("name", "must be a non-empty string");
            }
            errors.into_result()
        }
    }

    #[test]
    fn nested_failures_carry_their_path() {
        let items = vec![Named("a"), Named(""), Named("")];
        let error = items.validate().expect_err("two items fail");
        assert_eq!(
            error.to_string(),
            "[1].name: must be a non-empty string; [2].name: must be a non-empty string"
        );
        let mut outer = ValidationErrors::new();
        outer.nest("items", error);
        assert_eq!(outer.errors()[0].path, "items[1].name");

        let tags = BTreeMap::from([("first", Named(""))]);
        let error = tags.validate().expect_err("the entry fails");
        assert_eq!(error.errors()[0].path, "[\"first\"].name");
    }
}
