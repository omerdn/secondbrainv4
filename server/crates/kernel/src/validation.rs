//! Input validation helpers.
//!
//! Checks return `Result<(), FieldError>`; [`ValidationErrors`] collects them so a request reports
//! every invalid field at once (HTTP 422 via [`AppError::Validation`]).
//!
//! ```
//! use kernel::validation::{max_chars, non_blank, ValidationErrors};
//!
//! let title = "  ";
//! let mut errors = ValidationErrors::new();
//! errors.check(non_blank("title", title));
//! errors.check(max_chars("title", title, 200));
//! assert!(errors.into_result().is_err());
//! ```
//!
//! [`AppError::Validation`]: crate::AppError::Validation

use std::fmt;

use serde::Serialize;

use crate::AppError;

/// Machine-readable reason a field is invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldErrorCode {
    /// Missing or blank.
    Required,
    /// Shorter than allowed.
    TooShort,
    /// Longer than allowed.
    TooLong,
    /// Outside the allowed numeric range.
    OutOfRange,
    /// Malformed (e.g. not a valid id, date or cursor).
    Invalid,
}

/// One invalid field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldError {
    /// Field name as sent by the client (e.g. `title`, `subtasks[2].title`).
    pub field: String,
    /// Machine-readable code for clients to localise.
    pub code: FieldErrorCode,
    /// Developer-facing English description.
    pub message: String,
}

impl FieldError {
    /// Creates a field error.
    pub fn new(field: impl Into<String>, code: FieldErrorCode, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            code,
            message: message.into(),
        }
    }
}

/// A collection of field errors.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ValidationErrors {
    errors: Vec<FieldError>,
}

impl ValidationErrors {
    /// An empty collection.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records `result` if it is an error; returns `self` for chaining.
    pub fn check(&mut self, result: Result<(), FieldError>) -> &mut Self {
        if let Err(err) = result {
            self.errors.push(err);
        }
        self
    }

    /// Records an error directly.
    pub fn push(&mut self, error: FieldError) -> &mut Self {
        self.errors.push(error);
        self
    }

    /// `true` if no errors were recorded.
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    /// The recorded errors, in the order they were added.
    pub fn errors(&self) -> &[FieldError] {
        &self.errors
    }

    /// `Ok(())` if empty, otherwise [`AppError::Validation`].
    pub fn into_result(self) -> Result<(), AppError> {
        if self.is_empty() {
            Ok(())
        } else {
            Err(AppError::Validation(self))
        }
    }
}

impl From<FieldError> for ValidationErrors {
    fn from(error: FieldError) -> Self {
        Self {
            errors: vec![error],
        }
    }
}

/// The value must contain at least one non-whitespace character.
pub fn non_blank(field: &str, value: &str) -> Result<(), FieldError> {
    if value.trim().is_empty() {
        Err(FieldError::new(
            field,
            FieldErrorCode::Required,
            "must not be blank",
        ))
    } else {
        Ok(())
    }
}

/// At most `max` characters (Unicode scalar values, so "ş" counts as one).
pub fn max_chars(field: &str, value: &str, max: usize) -> Result<(), FieldError> {
    if value.chars().count() > max {
        Err(FieldError::new(
            field,
            FieldErrorCode::TooLong,
            format!("must be at most {max} characters"),
        ))
    } else {
        Ok(())
    }
}

/// At least `min` characters (Unicode scalar values).
pub fn min_chars(field: &str, value: &str, min: usize) -> Result<(), FieldError> {
    if value.chars().count() < min {
        Err(FieldError::new(
            field,
            FieldErrorCode::TooShort,
            format!("must be at least {min} characters"),
        ))
    } else {
        Ok(())
    }
}

/// `min <= value <= max`.
pub fn in_range<T>(field: &str, value: T, min: T, max: T) -> Result<(), FieldError>
where
    T: PartialOrd + fmt::Display,
{
    if value < min || value > max {
        Err(FieldError::new(
            field,
            FieldErrorCode::OutOfRange,
            format!("must be between {min} and {max}"),
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn non_blank_rejects_whitespace_only() {
        assert!(non_blank("title", "Gitar").is_ok());
        let err = non_blank("title", " \t\n").unwrap_err();
        assert_eq!(err.field, "title");
        assert_eq!(err.code, FieldErrorCode::Required);
    }

    #[test]
    fn char_limits_count_turkish_letters_once() {
        // 6 characters, 12 bytes in UTF-8.
        let word = "çğışöü";
        assert_eq!(word.len(), 12);
        assert!(max_chars("name", word, 6).is_ok());
        assert_eq!(
            max_chars("name", word, 5).unwrap_err().code,
            FieldErrorCode::TooLong
        );
        assert!(min_chars("name", word, 6).is_ok());
        assert_eq!(
            min_chars("name", word, 7).unwrap_err().code,
            FieldErrorCode::TooShort
        );
    }

    #[test]
    fn in_range_is_inclusive() {
        assert!(in_range("daily_target", 1, 1, 100).is_ok());
        assert!(in_range("daily_target", 100, 1, 100).is_ok());
        assert_eq!(
            in_range("daily_target", 0, 1, 100).unwrap_err().code,
            FieldErrorCode::OutOfRange
        );
        assert!(in_range("weight", 1.5, 0.0, 10.0).is_ok());
    }

    #[test]
    fn collects_every_error_in_order() {
        let mut errors = ValidationErrors::new();
        errors
            .check(non_blank("title", ""))
            .check(max_chars("description", "ok", 10))
            .check(in_range("daily_target", 0, 1, 100));

        let fields: Vec<_> = errors.errors().iter().map(|e| e.field.as_str()).collect();
        assert_eq!(fields, ["title", "daily_target"]);
        assert!(matches!(errors.into_result(), Err(AppError::Validation(_))));
    }

    #[test]
    fn empty_collection_is_ok() {
        assert!(ValidationErrors::new().into_result().is_ok());
    }

    #[test]
    fn serializes_as_a_list_with_snake_case_codes() {
        let errors = ValidationErrors::from(FieldError::new(
            "title",
            FieldErrorCode::TooLong,
            "too long",
        ));
        assert_eq!(
            serde_json::to_value(&errors).unwrap(),
            serde_json::json!([{ "field": "title", "code": "too_long", "message": "too long" }])
        );
    }

    proptest! {
        #[test]
        fn max_chars_matches_char_count(s in "\\PC{0,40}", max in 0usize..40) {
            prop_assert_eq!(max_chars("f", &s, max).is_ok(), s.chars().count() <= max);
        }

        #[test]
        fn in_range_matches_definition(v in any::<i64>(), a in any::<i64>(), b in any::<i64>()) {
            let (min, max) = (a.min(b), a.max(b));
            prop_assert_eq!(in_range("f", v, min, max).is_ok(), min <= v && v <= max);
        }
    }
}
