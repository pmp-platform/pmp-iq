//! Small shared string helpers reused across the app.

use crate::config::EnvSource;

/// Replace `${VAR}` and `${VAR:-default}` references in `raw` using `env`.
///
/// An unset variable with no default resolves to an empty string; a literal
/// `$$` escapes to a single `$`. Used to let `config.yaml` pull values from the
/// environment (M18) so secrets stay outside the file.
pub fn interpolate_env(raw: &str, env: &dyn EnvSource) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('$') => {
                chars.next();
                out.push('$');
            }
            Some('{') => {
                chars.next();
                out.push_str(&resolve_ref(&mut chars, env));
            }
            _ => out.push('$'),
        }
    }
    out
}

/// Read a `VAR}` or `VAR:-default}` body (the opening `${` already consumed) and
/// resolve it against `env`.
fn resolve_ref(chars: &mut std::iter::Peekable<std::str::Chars>, env: &dyn EnvSource) -> String {
    let mut body = String::new();
    for c in chars.by_ref() {
        if c == '}' {
            break;
        }
        body.push(c);
    }
    let (name, default) = match body.split_once(":-") {
        Some((n, d)) => (n, Some(d)),
        None => (body.as_str(), None),
    };
    env.get(name.trim())
        .or_else(|| default.map(str::to_string))
        .unwrap_or_default()
}

/// Percent-encode a string for use as a URL query value. Keeps the RFC 3986
/// unreserved set; everything else is `%`-escaped. Reused wherever the app
/// builds outbound URLs (e.g. the GitHub OAuth authorize URL).
pub fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

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

    use crate::config::MapEnv;

    #[test]
    fn interpolates_set_variable() {
        let env = MapEnv::new().with("HOST", "db.internal");
        assert_eq!(interpolate_env("h=${HOST}", &env), "h=db.internal");
    }

    #[test]
    fn uses_default_when_unset() {
        let env = MapEnv::new();
        assert_eq!(interpolate_env("p=${PORT:-5432}", &env), "p=5432");
    }

    #[test]
    fn set_variable_wins_over_default() {
        let env = MapEnv::new().with("PORT", "9999");
        assert_eq!(interpolate_env("p=${PORT:-5432}", &env), "p=9999");
    }

    #[test]
    fn unset_without_default_is_empty() {
        let env = MapEnv::new();
        assert_eq!(interpolate_env("x=${MISSING}y", &env), "x=y");
    }

    #[test]
    fn double_dollar_escapes() {
        let env = MapEnv::new();
        assert_eq!(interpolate_env("cost=$$5", &env), "cost=$5");
    }

    #[test]
    fn lone_dollar_is_literal() {
        let env = MapEnv::new();
        assert_eq!(interpolate_env("a $ b", &env), "a $ b");
    }

    #[test]
    fn percent_encode_keeps_unreserved_escapes_rest() {
        assert_eq!(percent_encode("read:user org"), "read%3Auser%20org");
        assert_eq!(percent_encode("http://x/cb"), "http%3A%2F%2Fx%2Fcb");
        assert_eq!(percent_encode("Aa0-_.~"), "Aa0-_.~");
    }
}
