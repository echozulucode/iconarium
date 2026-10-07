//! Matching and ranking (plan §17).
//!
//! Field priority: exact filename/stem (whole query) > filename word > filename
//! substring > path > title > visible text > description > IDs/classes.
//!
//! An asset's score is the sum over query terms of the best field weight that term
//! hits, plus bonuses (exact stem, phrase adjacency), minus a tiny penalty for deep or
//! long paths. Scores are integers (`weight * SCALE - penalty`) so ordering is exact
//! and deterministic; remaining ties are broken by relative path by the caller.

use memchr::memmem::Finder;
use regex::{Regex, RegexBuilder};

use crate::error::{CoreError, Result};
use crate::model::MatchField;

use super::query::{normalize_text, ParsedQuery, TermKind};

// ---- weights (score units) ----
/// Whole query equals the file stem / filename (separators `-_. ` ignored).
pub const W_EXACT: u32 = 1000;
/// Term equals a complete filename word.
pub const W_FILENAME_WORD: u32 = 60;
/// Term starts at a filename word boundary.
pub const W_FILENAME_PREFIX: u32 = 52;
/// Term occurs anywhere in the filename.
pub const W_FILENAME_SUB: u32 = 45;
pub const W_PATH: u32 = 30;
pub const W_TITLE: u32 = 20;
pub const W_TEXT: u32 = 15;
pub const W_DESC: u32 = 10;
pub const W_IDS: u32 = 5;
/// Extra when one term (of several) equals the whole stem.
pub const W_STEM_TERM: u32 = 10;
/// Multi-term queries whose terms appear adjacent (as a phrase) in one field get
/// `PHRASE_MULT * field_weight` extra.
pub const PHRASE_MULT: u32 = 4;
/// Score units are multiplied by this; the path penalty stays below it.
pub const SCALE: i64 = 100;
const MAX_PENALTY: i64 = SCALE - 1;

/// Max snippet length in characters (including ellipses).
pub const SNIPPET_MAX_CHARS: usize = 120;

/// Borrowed, pre-lowercased searchable fields of one asset.
/// Content fields are whitespace-collapsed (see [`normalize_text`]).
#[derive(Debug, Clone, Copy)]
pub struct Fields<'a> {
    /// All fields below joined by `'\n'` (no field contains `'\n'`). Used as a single-pass
    /// prefilter before per-field checks.
    pub all: &'a str,
    pub name: &'a str,
    /// Stem with separator runs (`-_. ` and whitespace) replaced by single spaces.
    pub stem_norm: &'a str,
    pub path: &'a str,
    pub title: &'a str,
    pub text: &'a str,
    pub desc: &'a str,
    pub ids: &'a str,
}

/// Lower-case, map separators (`-`, `_`, `.`, whitespace) to single spaces, trim.
pub fn separator_normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending = false;
    for c in s.chars() {
        if c == '-' || c == '_' || c == '.' || c.is_whitespace() {
            pending = !out.is_empty();
            continue;
        }
        if pending {
            out.push(' ');
            pending = false;
        }
        out.extend(c.to_lowercase());
    }
    out
}

#[inline]
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b >= 0x80
}

/// Best filename weight for a substring needle, if it occurs.
#[inline]
fn filename_weight(name: &str, finder: &Finder<'_>) -> Option<u32> {
    let hay = name.as_bytes();
    let n = finder.needle().len();
    let mut best: Option<u32> = None;
    for pos in finder.find_iter(hay) {
        let start_b = pos == 0 || !is_word_byte(hay[pos - 1]);
        let end = pos + n;
        let end_b = end >= hay.len() || !is_word_byte(hay[end]);
        let w = match (start_b, end_b) {
            (true, true) => return Some(W_FILENAME_WORD),
            (true, false) => W_FILENAME_PREFIX,
            _ => W_FILENAME_SUB,
        };
        best = Some(best.map_or(w, |b: u32| b.max(w)));
    }
    best
}

fn field_weight(f: MatchField) -> u32 {
    match f {
        MatchField::Filename => W_FILENAME_WORD,
        MatchField::Path => W_PATH,
        MatchField::Title => W_TITLE,
        MatchField::Text => W_TEXT,
        MatchField::Desc => W_DESC,
        MatchField::IdClass => W_IDS,
    }
}

/// Content fields in priority order.
const CONTENT: [MatchField; 4] = [
    MatchField::Title,
    MatchField::Text,
    MatchField::Desc,
    MatchField::IdClass,
];

impl<'a> Fields<'a> {
    #[inline]
    pub fn content(&self, f: MatchField) -> &'a str {
        match f {
            MatchField::Title => self.title,
            MatchField::Text => self.text,
            MatchField::Desc => self.desc,
            MatchField::IdClass => self.ids,
            MatchField::Path => self.path,
            MatchField::Filename => self.name,
        }
    }
}

/// One compiled include/exclude term.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)] // a handful per query; keep the hot Finder inline
enum Matcher {
    Plain(Finder<'static>),
    /// Glob compiled to a regex, matched against the lower-cased filename.
    Name(regex::bytes::Regex),
    /// Glob compiled to a regex, matched against the lower-cased relative path.
    Path(regex::bytes::Regex),
}

impl Matcher {
    /// Best (weight, field) this matcher hits, in priority order.
    #[inline]
    fn best(&self, f: &Fields<'_>) -> Option<(u32, MatchField)> {
        match self {
            Matcher::Plain(fd) => {
                // Terms never contain '\n', so a miss on the joined string is a miss everywhere.
                fd.find(f.all.as_bytes())?;
                if let Some(w) = filename_weight(f.name, fd) {
                    return Some((w, MatchField::Filename));
                }
                let b = |s: &str| fd.find(s.as_bytes()).is_some();
                if b(f.path) {
                    Some((W_PATH, MatchField::Path))
                } else if b(f.title) {
                    Some((W_TITLE, MatchField::Title))
                } else if b(f.text) {
                    Some((W_TEXT, MatchField::Text))
                } else if b(f.desc) {
                    Some((W_DESC, MatchField::Desc))
                } else if b(f.ids) {
                    Some((W_IDS, MatchField::IdClass))
                } else {
                    None
                }
            }
            Matcher::Name(re) => re
                .is_match(f.name.as_bytes())
                .then_some((W_FILENAME_WORD, MatchField::Filename)),
            Matcher::Path(re) => re
                .is_match(f.path.as_bytes())
                .then_some((W_PATH, MatchField::Path)),
        }
    }
}

fn compile_glob(pattern: &str, path: bool) -> Result<regex::bytes::Regex> {
    // Path globs are anchored at any directory unless they start with '/'.
    let pat = if path {
        if let Some(rest) = pattern.strip_prefix('/') {
            rest.to_string()
        } else if pattern.starts_with("**") {
            pattern.to_string()
        } else {
            format!("**/{pattern}")
        }
    } else {
        pattern.to_string()
    };
    let glob = globset::GlobBuilder::new(&pat)
        .case_insensitive(true)
        .literal_separator(true)
        .backslash_escape(true)
        .build()
        .map_err(|e| CoreError::Query(format!("invalid glob '{pattern}': {e}")))?;
    regex::bytes::Regex::new(glob.regex())
        .map_err(|e| CoreError::Query(format!("invalid glob '{pattern}': {e}")))
}

/// A query ready to be evaluated against many assets.
#[derive(Debug)]
pub struct CompiledQuery {
    include: Vec<Matcher>,
    /// Indices into `include` that are plain terms (for explanations), with their text.
    plain_texts: Vec<(usize, String)>,
    exclude: Vec<Matcher>,
    regex: Option<Regex>,
    /// Multi-term phrase (plain include terms joined by spaces).
    phrase: Option<Finder<'static>>,
    /// Separator-normalized phrase for comparing with the stem.
    phrase_sep: Option<String>,
    /// Separator-normalized whole query for the exact-stem bonus.
    exact: Option<String>,
    /// Plain terms, separator-normalized, for the stem-term bonus.
    stem_terms: Vec<String>,
}

impl CompiledQuery {
    /// Compile a parsed query. `Ok(None)` means "empty query" (list everything).
    pub fn compile(parsed: &ParsedQuery) -> Result<Option<Self>> {
        let mut q = CompiledQuery {
            include: Vec::new(),
            plain_texts: Vec::new(),
            exclude: Vec::new(),
            regex: None,
            phrase: None,
            phrase_sep: None,
            exact: None,
            stem_terms: Vec::new(),
        };
        match parsed {
            ParsedQuery::Empty => return Ok(None),
            ParsedQuery::Regex(pat) => {
                let re = RegexBuilder::new(pat)
                    .case_insensitive(true)
                    .multi_line(true)
                    .size_limit(10 * (1 << 20))
                    .build()
                    .map_err(|e| CoreError::Query(format!("invalid regular expression: {e}")))?;
                q.regex = Some(re);
            }
            ParsedQuery::Terms(terms) => {
                let mut plain_inc: Vec<&str> = Vec::new();
                let mut all_plain = true;
                for t in terms {
                    let m = match t.kind {
                        TermKind::Plain => {
                            Matcher::Plain(Finder::new(t.text.as_bytes()).into_owned())
                        }
                        TermKind::GlobName => Matcher::Name(compile_glob(&t.text, false)?),
                        TermKind::GlobPath => Matcher::Path(compile_glob(&t.text, true)?),
                    };
                    if t.negated {
                        q.exclude.push(m);
                    } else {
                        if t.kind == TermKind::Plain {
                            q.plain_texts.push((q.include.len(), t.text.clone()));
                            plain_inc.push(&t.text);
                            q.stem_terms.push(separator_normalize(&t.text));
                        } else {
                            all_plain = false;
                        }
                        q.include.push(m);
                    }
                }
                if !plain_inc.is_empty() {
                    let joined = plain_inc.join(" ");
                    if all_plain {
                        let ex = separator_normalize(&joined);
                        if !ex.is_empty() {
                            q.exact = Some(ex);
                        }
                    }
                    if plain_inc.len() >= 2 {
                        q.phrase_sep = Some(separator_normalize(&joined));
                        q.phrase = Some(Finder::new(joined.as_bytes()).into_owned());
                    }
                }
                if q.stem_terms.len() < 2 {
                    q.stem_terms.clear(); // single-term stem equality is covered by the exact bonus
                }
            }
        }
        Ok(Some(q))
    }

    /// Score for one asset, or `None` if it does not match.
    #[inline]
    pub fn score(&self, f: &Fields<'_>, penalty: i64) -> Option<i64> {
        let mut s: u32 = 0;
        if let Some(re) = &self.regex {
            s = self.regex_best(re, f)?.0;
        } else {
            for m in &self.include {
                s += m.best(f)?.0;
            }
            for m in &self.exclude {
                if m.best(f).is_some() {
                    return None;
                }
            }
            if self.include.is_empty() {
                return Some(0); // exclusion-only query: keep pure path order
            }
            s += self.bonus(f);
        }
        Some(s as i64 * SCALE - penalty.clamp(0, MAX_PENALTY))
    }

    fn bonus(&self, f: &Fields<'_>) -> u32 {
        let mut b = 0;
        if let Some(ex) = &self.exact {
            let stem = f.stem_norm;
            // name = stem + ".svg", so "<stem> svg" is the normalized filename.
            let full = ex.len() == stem.len() + 4 && ex.starts_with(stem) && ex.ends_with(" svg");
            if ex == stem || full {
                b += W_EXACT;
            }
        }
        for t in &self.stem_terms {
            if t == f.stem_norm {
                b += W_STEM_TERM;
            }
        }
        if let Some(p) = &self.phrase {
            let in_name = self
                .phrase_sep
                .as_deref()
                .map(|ps| !ps.is_empty() && f.stem_norm.contains(ps))
                .unwrap_or(false);
            let w = if in_name {
                W_FILENAME_WORD
            } else if p.find(f.path.as_bytes()).is_some() {
                W_PATH
            } else {
                CONTENT
                    .iter()
                    .find(|&&fl| p.find(f.content(fl).as_bytes()).is_some())
                    .map(|&fl| field_weight(fl))
                    .unwrap_or(0)
            };
            b += w * PHRASE_MULT;
        }
        b
    }

    fn regex_best(&self, re: &Regex, f: &Fields<'_>) -> Option<(u32, MatchField)> {
        // Multi-line mode: '^'/'$' anchor at field boundaries and '.' never crosses one,
        // so a miss on the joined string is a miss in every field — except that the
        // filename is a suffix of the path line, so a '^'-anchored pattern can match the
        // filename without matching any line start. Skip the prefilter for those.
        if !re.as_str().contains('^') && !re.is_match(f.all) {
            return None;
        }
        if re.is_match(f.name) {
            Some((W_FILENAME_SUB, MatchField::Filename))
        } else if re.is_match(f.path) {
            Some((W_PATH, MatchField::Path))
        } else {
            CONTENT
                .iter()
                .find(|&&fl| re.is_match(f.content(fl)))
                .map(|&fl| (field_weight(fl), fl))
        }
    }

    /// Explain a match when it comes from SVG content. Returns the field and the
    /// `(start, end)` byte range of the match within that (lower-cased, collapsed) field.
    pub fn explain_location(&self, f: &Fields<'_>) -> Option<(MatchField, usize, usize)> {
        let is_content = |fl: MatchField| CONTENT.contains(&fl);
        if let Some(re) = &self.regex {
            let (_, fl) = self.regex_best(re, f)?;
            if !is_content(fl) {
                return None;
            }
            let m = re.find(f.content(fl))?;
            return Some((fl, m.start(), m.end()));
        }
        // Must actually match.
        let mut first_content: Option<(usize, MatchField)> = None;
        for (i, m) in self.include.iter().enumerate() {
            let (_, fl) = m.best(f)?;
            if first_content.is_none() && is_content(fl) {
                first_content = Some((i, fl));
            }
        }
        if self.exclude.iter().any(|m| m.best(f).is_some()) {
            return None;
        }
        let (idx, fl) = first_content?;
        // Prefer the whole phrase when it appears in a content field.
        if let Some(p) = &self.phrase {
            for &c in &CONTENT {
                let hay = f.content(c);
                if let Some(pos) = p.find(hay.as_bytes()) {
                    return Some((c, pos, pos + p.needle().len()));
                }
            }
        }
        let text = &self.plain_texts.iter().find(|(i, _)| *i == idx)?.1;
        let hay = f.content(fl);
        let pos = memchr::memmem::find(hay.as_bytes(), text.as_bytes())?;
        Some((fl, pos, pos + text.len()))
    }
}

/// Lower-cased, whitespace-collapsed view of an original string that remembers which
/// original character each byte came from (for snippets in original case).
pub struct NormMap {
    pub lc: String,
    /// Original characters after whitespace collapsing.
    chars: Vec<char>,
    /// For each byte of `lc`, the index into `chars`.
    map: Vec<u32>,
}

impl NormMap {
    pub fn new(original: &str) -> Self {
        let mut lc = String::with_capacity(original.len());
        let mut chars = Vec::with_capacity(original.len());
        let mut map = Vec::with_capacity(original.len());
        let mut pending = false;
        for c in original.chars() {
            if c.is_whitespace() {
                pending = !chars.is_empty();
                continue;
            }
            if pending {
                lc.push(' ');
                map.push(chars.len() as u32);
                chars.push(' ');
                pending = false;
            }
            let idx = chars.len() as u32;
            chars.push(c);
            for l in c.to_lowercase() {
                let before = lc.len();
                lc.push(l);
                map.extend(std::iter::repeat_n(idx, lc.len() - before));
            }
        }
        debug_assert_eq!(lc, normalize_text(original));
        Self { lc, chars, map }
    }

    /// Snippet (≤ [`SNIPPET_MAX_CHARS`] chars) around the `lc` byte range `start..end`.
    pub fn snippet(&self, start: usize, end: usize) -> String {
        if self.chars.is_empty() {
            return String::new();
        }
        let n = self.chars.len();
        let cs = self.map.get(start).copied().unwrap_or(0) as usize;
        let ce = if end == 0 {
            cs
        } else {
            self.map.get(end - 1).map(|&i| i as usize + 1).unwrap_or(n)
        };
        if n <= SNIPPET_MAX_CHARS {
            return self.chars.iter().collect();
        }
        let budget = SNIPPET_MAX_CHARS - 2; // room for two ellipses
        let mlen = ce.saturating_sub(cs);
        let (mut s, mut e);
        if mlen >= budget {
            s = cs;
            e = cs + budget;
        } else {
            let ctx = budget - mlen;
            s = cs.saturating_sub(ctx / 2);
            e = (s + budget).min(n);
            s = e.saturating_sub(budget);
            // Snap to word boundaries when it does not cut into the match.
            if s > 0 {
                if let Some(off) = self.chars[s..cs].iter().take(16).position(|c| *c == ' ') {
                    s += off + 1;
                }
            }
            if e < n {
                if let Some(off) = self.chars[ce..e]
                    .iter()
                    .rev()
                    .take(16)
                    .position(|c| *c == ' ')
                {
                    e -= off + 1;
                }
            }
        }
        let mut out = String::new();
        if s > 0 {
            out.push('…');
        }
        out.extend(self.chars[s..e].iter());
        if e < n {
            out.push('…');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_word_classes() {
        let f = Finder::new("motor");
        assert_eq!(
            filename_weight("motor-controller.svg", &f),
            Some(W_FILENAME_WORD)
        );
        assert_eq!(
            filename_weight("motormount.svg", &f),
            Some(W_FILENAME_PREFIX)
        );
        assert_eq!(filename_weight("servomotor.svg", &f), Some(W_FILENAME_SUB));
        assert_eq!(filename_weight("pump.svg", &f), None);
    }

    #[test]
    fn separator_normalization() {
        assert_eq!(
            separator_normalize("Motor-Controller__v2.SVG"),
            "motor controller v2 svg"
        );
        assert_eq!(separator_normalize("  a  b "), "a b");
    }

    #[test]
    fn snippet_centering_and_case() {
        let long = format!(
            "{} Ethernet   Control Interface {}",
            "lorem ipsum ".repeat(20),
            "dolor sit ".repeat(20)
        );
        let m = NormMap::new(&long);
        let pos = m.lc.find("ethernet control").unwrap();
        let s = m.snippet(pos, pos + "ethernet control".len());
        assert!(s.contains("Ethernet Control Interface"), "{s}");
        assert!(s.starts_with('…') && s.ends_with('…'), "{s}");
        assert!(s.chars().count() <= SNIPPET_MAX_CHARS);
        let short = NormMap::new("Hello\n  World");
        assert_eq!(short.snippet(0, 5), "Hello World");
    }
}
