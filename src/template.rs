/// Template rendering shared across pr.rs, config.rs, etc.
///
/// Supports two forms:
///   {variable}                  → substitute value directly
///   {variable: 'fmt with $1'}   → if value non-empty, replace $1 and emit; else emit nothing
use std::collections::HashMap;

pub(crate) fn render_template(template: &str, vars: &HashMap<String, String>) -> String {
    let mut result = String::new();
    let mut rest = template;

    while let Some(start) = rest.find('{') {
        result.push_str(&rest[..start]);
        rest = &rest[start + 1..];

        let Some(end) = rest.find('}') else {
            result.push('{');
            continue;
        };

        let inner = &rest[..end];
        rest = &rest[end + 1..];

        if let Some(colon) = inner.find(':') {
            let var_name = inner[..colon].trim();
            let fmt = inner[colon + 1..]
                .trim()
                .trim_matches(|c| c == '\'' || c == '"');
            let value = vars.get(var_name).map(|s| s.as_str()).unwrap_or("");
            if !value.is_empty() {
                result.push_str(&fmt.replace("$1", value));
            }
        } else {
            let value = vars.get(inner.trim()).map(|s| s.as_str()).unwrap_or("");
            result.push_str(value);
        }
    }

    result.push_str(rest);
    result
}

/// Process escape sequences in strings coming from YAML block scalars.
pub(crate) fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some('n') => {
                    chars.next();
                    out.push('\n');
                }
                Some('t') => {
                    chars.next();
                    out.push('\t');
                }
                Some('r') => {
                    chars.next();
                    out.push('\r');
                }
                Some('\\') => {
                    chars.next();
                    out.push('\\');
                }
                _ => out.push(c),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    // ── render_template ───────────────────────────────────────────────────────

    #[test]
    fn simple_substitution() {
        let v = vars(&[("name", "world")]);
        assert_eq!(render_template("Hello {name}!", &v), "Hello world!");
    }

    #[test]
    fn multiple_variables() {
        let v = vars(&[("a", "foo"), ("b", "bar")]);
        assert_eq!(render_template("{a}-{b}", &v), "foo-bar");
    }

    #[test]
    fn missing_variable_renders_empty() {
        let v = vars(&[]);
        assert_eq!(
            render_template("before {missing} after", &v),
            "before  after"
        );
    }

    #[test]
    fn conditional_set() {
        let v = vars(&[("x", "DEVELOP")]);
        assert_eq!(
            render_template("prefix{x: '-$1'}suffix", &v),
            "prefix-DEVELOPsuffix"
        );
    }

    #[test]
    fn conditional_empty_omits_block() {
        let v = vars(&[("x", "")]);
        assert_eq!(
            render_template("prefix{x: '-$1'}suffix", &v),
            "prefixsuffix"
        );
    }

    #[test]
    fn conditional_missing_omits_block() {
        let v = vars(&[]);
        assert_eq!(
            render_template("prefix{x: '-$1'}suffix", &v),
            "prefixsuffix"
        );
    }

    #[test]
    fn conditional_double_quoted_format() {
        let v = vars(&[("x", "val")]);
        assert_eq!(render_template(r#"a{x: "-$1"}b"#, &v), "a-valb");
    }

    #[test]
    fn no_vars_passthrough() {
        let v = vars(&[]);
        assert_eq!(render_template("plain text", &v), "plain text");
    }

    #[test]
    fn unclosed_brace_emitted_literally() {
        let v = vars(&[]);
        // A lone '{' with no closing '}' should be emitted as-is.
        assert!(render_template("open { brace", &v).contains('{'));
    }

    #[test]
    fn branch_format_no_conflict_base() {
        let v = vars(&[
            ("branchPrefix", "fix"),
            ("ticket.key", "CAPY-1234"),
            ("slug", "some-feature"),
            ("conflictBase", ""),
        ]);
        let fmt = "{branchPrefix}/{ticket.key}{conflictBase: '-$1'}-{slug}";
        assert_eq!(render_template(fmt, &v), "fix/CAPY-1234-some-feature");
    }

    #[test]
    fn branch_format_with_conflict_base() {
        let v = vars(&[
            ("branchPrefix", "fix"),
            ("ticket.key", "CAPY-1234"),
            ("slug", "some-feature"),
            ("conflictBase", "DEVELOP"),
        ]);
        let fmt = "{branchPrefix}/{ticket.key}{conflictBase: '-$1'}-{slug}";
        assert_eq!(
            render_template(fmt, &v),
            "fix/CAPY-1234-DEVELOP-some-feature"
        );
    }

    // ── unescape ──────────────────────────────────────────────────────────────

    #[test]
    fn unescape_newline() {
        assert_eq!(unescape(r"\n"), "\n");
    }

    #[test]
    fn unescape_tab() {
        assert_eq!(unescape(r"\t"), "\t");
    }

    #[test]
    fn unescape_cr() {
        assert_eq!(unescape(r"\r"), "\r");
    }

    #[test]
    fn unescape_backslash() {
        assert_eq!(unescape(r"\\"), "\\");
    }

    #[test]
    fn unescape_unknown_sequence_kept() {
        // Unknown escape sequences are kept as-is.
        assert_eq!(unescape(r"\q"), r"\q");
    }

    #[test]
    fn unescape_no_escapes() {
        assert_eq!(unescape("hello world"), "hello world");
    }

    #[test]
    fn unescape_mixed() {
        assert_eq!(unescape(r"line1\nline2\ttabbed"), "line1\nline2\ttabbed");
    }
}
