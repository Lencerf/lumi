//! Useful functions for parsing and accounting.

use crate::{Decimal, Error, ErrorLevel, ErrorType, Source};

/// Parses a [`Decimal`](crate::Decimal) from a [`&str`].
#[inline]
pub fn parse_decimal(num_str: &str, src: &Source) -> Result<Decimal, Error> {
    match num_str.parse::<Decimal>() {
        Ok(num) => Ok(num),
        Err(_) => {
            let error = Error {
                msg: "Invalid number.".to_string(),
                src: src.clone(),
                r#type: ErrorType::Syntax,
                level: ErrorLevel::Error,
            };
            Err(error)
        }
    }
}
