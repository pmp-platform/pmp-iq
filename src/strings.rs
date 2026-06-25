//! Small shared string helpers reused across the app.

/// Trim a value and collapse blank/whitespace-only optionals to `None`.
///
/// Form fields arrive as `Some("")` when left empty; storing that as-is later
/// produces invalid values (e.g. an empty provider base URL becomes a relative
/// request URL). Normalising to `None` lets defaults apply instead.
pub fn blank_to_none(value: Option<String>) -> Option<String> {
    value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_passes_through() {
        assert_eq!(blank_to_none(None), None);
    }

    #[test]
    fn empty_and_whitespace_become_none() {
        assert_eq!(blank_to_none(Some(String::new())), None);
        assert_eq!(blank_to_none(Some("   ".into())), None);
    }

    #[test]
    fn value_is_trimmed() {
        assert_eq!(blank_to_none(Some("  x  ".into())), Some("x".into()));
    }
}
