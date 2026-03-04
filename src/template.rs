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
                Some('n')  => { chars.next(); out.push('\n'); }
                Some('t')  => { chars.next(); out.push('\t'); }
                Some('r')  => { chars.next(); out.push('\r'); }
                Some('\\') => { chars.next(); out.push('\\'); }
                _ => out.push(c),
            }
        } else {
            out.push(c);
        }
    }
    out
}
