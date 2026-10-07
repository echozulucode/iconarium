//! Query syntax (plan §16/§17).
//!
//! ```text
//! ethernet switch          → ethernet AND switch (case-insensitive substrings, any field)
//! "motor controller"       → one phrase token
//! -legacy  -"old style"    → exclusion
//! network/*switch*.svg     → glob ('/' present → relative path, anchored at any directory)
//! *.svg  icon-??           → glob against the filename
//! re:ethernet.*switch      → case-insensitive regex over the rest of the query
//! ```
//!
//! This module is purely syntactic; matching lives in [`super::ranking`].

/// Kind of a single query term.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermKind {
    /// Case-insensitive substring in any field.
    Plain,
    /// Glob against the filename (no '/' in the pattern).
    GlobName,
    /// Glob against the relative path (pattern contains '/').
    GlobPath,
}

/// One whitespace-delimited token or quoted phrase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Term {
    /// Lower-cased text; whitespace inside phrases collapsed to single spaces.
    pub text: String,
    pub kind: TermKind,
    pub negated: bool,
    pub quoted: bool,
}

/// Parsed query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedQuery {
    /// No terms: list everything in path order.
    Empty,
    /// `re:` query; holds the pattern (trimmed, original case).
    Regex(String),
    Terms(Vec<Term>),
}

impl ParsedQuery {
    pub fn is_empty(&self) -> bool {
        matches!(self, ParsedQuery::Empty)
    }
}

/// Lower-case and collapse runs of whitespace into single spaces (trimmed).
pub fn normalize_text(s: &str) -> String {
    if s.is_ascii() {
        // Fast path (the common case): byte loop, same semantics as below.
        let mut out = Vec::with_capacity(s.len());
        let mut pending_space = false;
        for &b in s.as_bytes() {
            if matches!(b, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r') {
                pending_space = !out.is_empty();
                continue;
            }
            if pending_space {
                out.push(b' ');
                pending_space = false;
            }
            out.push(b.to_ascii_lowercase());
        }
        return String::from_utf8(out).expect("ASCII is UTF-8");
    }
    let mut out = String::with_capacity(s.len());
    let mut pending_space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        for l in c.to_lowercase() {
            out.push(l);
        }
    }
    out
}

fn is_glob(s: &str) -> bool {
    s.contains(['*', '?'])
}

/// Parse a raw query string. Never fails; pattern validity is checked at compile time
/// in [`super::ranking::CompiledQuery::compile`].
pub fn parse(query: &str) -> ParsedQuery {
    let trimmed = query.trim();
    if trimmed.len() >= 3 && trimmed.as_bytes()[..3].eq_ignore_ascii_case(b"re:") {
        let pat = trimmed[3..].trim();
        return if pat.is_empty() {
            ParsedQuery::Empty
        } else {
            ParsedQuery::Regex(pat.to_string())
        };
    }

    let mut terms = Vec::new();
    let mut chars = trimmed.char_indices().peekable();
    while let Some(&(start, c)) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        // Optional leading '-' (exclusion) — only when followed by something.
        let mut negated = false;
        let mut pos = start;
        if c == '-' {
            let rest = &trimmed[start + 1..];
            if rest
                .chars()
                .next()
                .map(|n| !n.is_whitespace())
                .unwrap_or(false)
            {
                negated = true;
                chars.next();
                pos = start + 1;
            }
        }
        let quoted = trimmed[pos..].starts_with('"');
        let raw: &str;
        if quoted {
            chars.next(); // opening quote
            let body_start = pos + 1;
            // An unterminated quote runs to the end of the query.
            let mut end = trimmed.len();
            for (i, ch) in chars.by_ref() {
                if ch == '"' {
                    end = i;
                    break;
                }
            }
            raw = &trimmed[body_start..end];
        } else {
            let mut end = trimmed.len();
            while let Some(&(i, ch)) = chars.peek() {
                if ch.is_whitespace() {
                    end = i;
                    break;
                }
                chars.next();
            }
            raw = &trimmed[pos..end];
        }
        let text = normalize_text(raw);
        if text.is_empty() {
            continue;
        }
        let kind = if !quoted && is_glob(&text) {
            if text.contains('/') {
                TermKind::GlobPath
            } else {
                TermKind::GlobName
            }
        } else {
            TermKind::Plain
        };
        terms.push(Term {
            text,
            kind,
            negated,
            quoted,
        });
    }
    if terms.is_empty() {
        ParsedQuery::Empty
    } else {
        ParsedQuery::Terms(terms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(text: &str, kind: TermKind, negated: bool, quoted: bool) -> Term {
        Term {
            text: text.into(),
            kind,
            negated,
            quoted,
        }
    }

    #[test]
    fn tokens_phrases_negation_globs() {
        let q =
            parse(r#"  Ethernet  "Motor   Controller" -legacy -"old style" net/*sw*.svg *.SVG - "#);
        assert_eq!(
            q,
            ParsedQuery::Terms(vec![
                t("ethernet", TermKind::Plain, false, false),
                t("motor controller", TermKind::Plain, false, true),
                t("legacy", TermKind::Plain, true, false),
                t("old style", TermKind::Plain, true, true),
                t("net/*sw*.svg", TermKind::GlobPath, false, false),
                t("*.svg", TermKind::GlobName, false, false),
                t("-", TermKind::Plain, false, false),
            ])
        );
    }

    #[test]
    fn empty_and_regex() {
        assert_eq!(parse("   "), ParsedQuery::Empty);
        assert_eq!(parse(r#""""#), ParsedQuery::Empty);
        assert_eq!(parse(" RE: eth.*sw "), ParsedQuery::Regex("eth.*sw".into()));
        assert_eq!(parse("re:"), ParsedQuery::Empty);
    }

    #[test]
    fn quoted_globs_are_literal_and_unterminated_quote_runs_to_end() {
        assert_eq!(
            parse(r#""a*b""#),
            ParsedQuery::Terms(vec![t("a*b", TermKind::Plain, false, true)])
        );
        assert_eq!(
            parse(r#""open end"#),
            ParsedQuery::Terms(vec![t("open end", TermKind::Plain, false, true)])
        );
    }
}
