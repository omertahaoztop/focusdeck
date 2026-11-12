use crate::i18n::{msg, t_args};
use std::time::Duration;

pub fn parse(s: &str) -> Result<Duration, String> {
    let quoted = format!("{s:?}");
    let bad = || {
        t_args(
            msg::ERROR_DURATION_INVALID,
            &[("value", quoted.as_str().into())],
        )
    };
    let unit = s.chars().next_back().ok_or_else(bad)?;
    let per_unit: u64 = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        _ => return Err(bad()),
    };
    let digits = &s[..s.len() - 1];
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad());
    }
    let n: u64 = digits.parse().map_err(|_| bad())?;
    let secs = n.checked_mul(per_unit).ok_or_else(bad)?;
    if secs == 0 {
        return Err(t_args(
            msg::ERROR_DURATION_ZERO,
            &[("value", quoted.as_str().into())],
        ));
    }
    Ok(Duration::from_secs(secs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn parses_units() {
        assert_eq!(parse("90s"), Ok(Duration::from_secs(90)));
        assert_eq!(parse("2m"), Ok(Duration::from_secs(120)));
        assert_eq!(parse("1h"), Ok(Duration::from_secs(3600)));
    }

    #[test]
    fn error_texts_are_english_without_a_language() {
        assert_eq!(
            parse("2").unwrap_err(),
            "invalid duration: \"2\" (example: 90s, 2m, 1h)"
        );
        assert_eq!(parse("0m").unwrap_err(), "duration cannot be zero: \"0m\"");
    }

    #[test]
    fn rejects_bad_input() {
        for s in [
            "2",
            "0m",
            "",
            "m",
            "-5s",
            "+5s",
            "5 s",
            "5d",
            "1.5h",
            "99999999999999999999h",
            "18446744073709551615h",
        ] {
            assert!(parse(s).is_err(), "{s:?} kabul edilmemeli");
        }
    }
}
