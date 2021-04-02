//! Useful functions for parsing and accounting.

use crate::{Decimal, Error, ErrorLevel, ErrorType, Source};

/// Parses a [`Decimal`](crate::Decimal) from a [`&str`] and pushes the error into
/// `errors`.
pub fn parse_decimal(num_str: &str, src: &Source, errors: &mut Vec<Error>) -> Option<Decimal> {
    match num_str.parse::<Decimal>() {
        Ok(num) => Some(num),
        Err(_) => {
            errors.push(Error {
                msg: "Invalid number.".to_string(),
                src: src.clone(),
                r#type: ErrorType::Syntax,
                level: ErrorLevel::Error,
            });
            None
        }
    }
}
