//! Shared low-level helpers: byte decoding, XML parsing with limits, SVG lengths,
//! viewBox parsing, root lookup and raw (byte-preserving) start-tag scanning.

use std::borrow::Cow;

use roxmltree::{Document, Node, ParsingOptions};

use crate::model::ViewBox;

pub const SVG_NS: &str = "http://www.w3.org/2000/svg";
pub const XLINK_NS: &str = "http://www.w3.org/1999/xlink";

/// Why a document could not be parsed.
#[derive(Debug, Clone, PartialEq)]
pub enum ParseFailure {
    /// A complexity limit was hit while parsing (e.g. node count).
    Limit(String),
    /// Malformed / unsupported input.
    Malformed(String),
}

impl ParseFailure {
    pub fn message(&self) -> &str {
        match self {
            Self::Limit(m) | Self::Malformed(m) => m,
        }
    }
}

/// Decoded document text plus whether it was transcoded from a non-UTF-8 encoding
/// (in which case any XML declaration's `encoding` no longer matches).
#[derive(Debug)]
pub struct DecodedText<'a> {
    pub text: Cow<'a, str>,
    pub transcoded: bool,
}

/// Decode SVG bytes to text. Handles a UTF-8 BOM (stripped), UTF-16 LE/BE with BOM,
/// and ISO-8859-1 / windows-1252 documents that declare such an encoding (decoded as
/// Latin-1). Everything else must be valid UTF-8.
pub fn decode_text(bytes: &[u8]) -> Result<DecodedText<'_>, String> {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return std::str::from_utf8(rest)
            .map(|s| DecodedText {
                text: Cow::Borrowed(s),
                transcoded: false,
            })
            .map_err(|_| "File is not valid UTF-8 text".to_string());
    }
    let utf16 = |le: bool| -> Result<DecodedText<'_>, String> {
        let body = &bytes[2..];
        if !body.len().is_multiple_of(2) {
            return Err("Truncated UTF-16 text".to_string());
        }
        let units: Vec<u16> = body
            .chunks_exact(2)
            .map(|c| {
                if le {
                    u16::from_le_bytes([c[0], c[1]])
                } else {
                    u16::from_be_bytes([c[0], c[1]])
                }
            })
            .collect();
        String::from_utf16(&units)
            .map(|s| DecodedText {
                text: Cow::Owned(s),
                transcoded: true,
            })
            .map_err(|_| "File is not valid UTF-16 text".to_string())
    };
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return utf16(true);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return utf16(false);
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => Ok(DecodedText {
            text: Cow::Borrowed(s),
            transcoded: false,
        }),
        Err(_) => {
            // Latin-1 fallback only when the XML declaration asks for it.
            let head = &bytes[..bytes.len().min(200)];
            let head = String::from_utf8_lossy(head).to_ascii_lowercase();
            let latin = head.starts_with("<?xml")
                && [
                    "iso-8859-1",
                    "latin1",
                    "latin-1",
                    "windows-1252",
                    "iso-8859-15",
                ]
                .iter()
                .any(|e| head.contains(e));
            if latin {
                Ok(DecodedText {
                    text: Cow::Owned(bytes.iter().map(|&b| b as char).collect()),
                    transcoded: true,
                })
            } else {
                Err("File is not valid UTF-8 text".to_string())
            }
        }
    }
}

/// Hard nesting cap for callers that don't carry [`crate::config::Limits`] (crop,
/// viewer normalization). Parsing alone is safe well beyond this on a 2 MiB stack.
pub const HARD_MAX_DEPTH: u32 = 1024;

/// Maximum element nesting depth of an XML text, computed by a cheap linear scan of the
/// raw markup (comments, CDATA, processing instructions, DOCTYPE internal subsets and
/// quoted attribute values are skipped). Stops early once `stop_above` is exceeded.
pub fn element_depth(text: &str, stop_above: u32) -> u32 {
    let b = text.as_bytes();
    let n = b.len();
    let mut i = 0usize;
    let mut depth: u32 = 0;
    let mut max: u32 = 0;
    let find = |from: usize, pat: &[u8]| -> usize {
        memchr::memmem::find(&b[from.min(n)..], pat)
            .map(|p| from + p + pat.len())
            .unwrap_or(n)
    };
    while i < n {
        let Some(p) = memchr::memchr(b'<', &b[i..]) else {
            break;
        };
        i += p + 1;
        if i >= n {
            break;
        }
        match b[i] {
            b'!' => {
                if b[i..].starts_with(b"!--") {
                    i = find(i + 3, b"-->");
                } else if b[i..].starts_with(b"![CDATA[") {
                    i = find(i + 8, b"]]>");
                } else {
                    // DOCTYPE / declarations, possibly with an internal subset [...]
                    let mut bracket = 0i32;
                    while i < n {
                        match b[i] {
                            b'[' => bracket += 1,
                            b']' => bracket -= 1,
                            b'>' if bracket <= 0 => {
                                i += 1;
                                break;
                            }
                            _ => {}
                        }
                        i += 1;
                    }
                }
            }
            b'?' => i = find(i + 1, b"?>"),
            b'/' => {
                depth = depth.saturating_sub(1);
                i = find(i + 1, b">");
            }
            _ => {
                // Start tag: scan to its end, honoring quoted attribute values.
                let mut quote: u8 = 0;
                let mut self_closing = false;
                while i < n {
                    let c = b[i];
                    if quote != 0 {
                        if c == quote {
                            quote = 0;
                        }
                    } else if c == b'"' || c == b'\'' {
                        quote = c;
                    } else if c == b'>' {
                        self_closing = i > 0 && b[i - 1] == b'/';
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                if !self_closing {
                    depth += 1;
                    if depth > max {
                        max = depth;
                        if max > stop_above {
                            return max;
                        }
                    }
                }
            }
        }
    }
    max
}

/// Parse XML with DTD support and a node limit (`max_nodes`; one more node fails).
/// Documents nested deeper than `max_depth` are rejected *before* parsing, because the
/// parser and renderer recurse per level and would overflow the thread stack.
pub fn parse_xml(text: &str, max_nodes: u32, max_depth: u32) -> Result<Document<'_>, ParseFailure> {
    let depth = element_depth(text, max_depth);
    if depth > max_depth {
        return Err(ParseFailure::Limit(format!(
            "Elements nested too deeply (more than {} levels)",
            group_thousands(max_depth as u64)
        )));
    }
    let opts = ParsingOptions {
        allow_dtd: true,
        nodes_limit: max_nodes.saturating_add(1),
        ..Default::default()
    };
    match Document::parse_with_options(text, opts) {
        Ok(d) => Ok(d),
        Err(roxmltree::Error::NodesLimitReached) => Err(ParseFailure::Limit(format!(
            "Too many XML nodes (more than {})",
            group_thousands(max_nodes as u64)
        ))),
        Err(roxmltree::Error::NoRootNode) => {
            Err(ParseFailure::Malformed("Not an XML document".into()))
        }
        Err(e) => Err(ParseFailure::Malformed(format!("Malformed XML: {e}"))),
    }
}

/// True if the node is an element with the given local name in the SVG namespace
/// (or in no namespace, which lenient documents use).
pub fn is_svg_el(node: Node<'_, '_>, local: &str) -> bool {
    if !node.is_element() {
        return false;
    }
    let tag = node.tag_name();
    tag.name() == local && matches!(tag.namespace(), None | Some(SVG_NS))
}

/// The document's root element if it is `<svg>` (SVG namespace or none).
pub fn root_svg<'a, 'input>(doc: &'a Document<'input>) -> Result<Node<'a, 'input>, String> {
    let root = doc.root_element();
    if is_svg_el(root, "svg") {
        Ok(root)
    } else {
        Err(format!(
            "Root element is <{}>, not <svg>",
            root.tag_name().name()
        ))
    }
}

/// `href` (SVG 2) or `xlink:href` value of an element.
pub fn href<'a>(node: Node<'a, '_>) -> Option<&'a str> {
    node.attribute("href")
        .or_else(|| node.attribute((XLINK_NS, "href")))
}

/// Split the numeric prefix off a CSS/SVG length/number. Returns (number, rest).
fn split_number(s: &str) -> Option<(f64, &str)> {
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let digits_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i == digits_start || (i == digits_start + 1 && b[digits_start] == b'.') {
        return None;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        if j < b.len() && b[j].is_ascii_digit() {
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            i = j;
        }
    }
    let n: f64 = s[..i].parse().ok()?;
    n.is_finite().then_some((n, &s[i..]))
}

/// Parse an SVG length and convert to CSS px. Units: none/px, pt (4/3), pc (16),
/// mm (96/25.4), cm (96/2.54), in (96), em (16 px), ex (8 px). Percentages and
/// unknown units yield `None`.
pub fn parse_length(s: &str) -> Option<f64> {
    let (n, unit) = split_number(s.trim())?;
    let factor = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "px" => 1.0,
        "pt" => 4.0 / 3.0,
        "pc" => 16.0,
        "mm" => 96.0 / 25.4,
        "cm" => 96.0 / 2.54,
        "in" => 96.0,
        "em" => 16.0,
        "ex" => 8.0,
        _ => return None,
    };
    let v = n * factor;
    v.is_finite().then_some(v)
}

/// Parse a positive absolute length (> 0) in px.
pub fn parse_positive_length(s: &str) -> Option<f64> {
    parse_length(s).filter(|v| *v > 0.0)
}

/// Parse a `viewBox` attribute: four numbers separated by whitespace and/or commas,
/// width and height > 0.
pub fn parse_view_box(s: &str) -> Option<ViewBox> {
    let mut nums = [0.0f64; 4];
    let mut count = 0;
    for part in s
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|p| !p.is_empty())
    {
        if count == 4 {
            return None;
        }
        let (n, rest) = split_number(part)?;
        if !rest.is_empty() {
            return None;
        }
        nums[count] = n;
        count += 1;
    }
    if count != 4 || nums[2] <= 0.0 || nums[3] <= 0.0 {
        return None;
    }
    Some(ViewBox {
        min_x: nums[0],
        min_y: nums[1],
        width: nums[2],
        height: nums[3],
    })
}

/// Collapse runs of whitespace into single spaces and trim.
pub fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for w in s.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(w);
    }
    out
}

/// Truncate to at most `max_chars` characters (char-boundary safe).
pub fn truncate_chars(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// Truncate to at most `max_bytes` bytes on a char boundary.
pub fn truncate_bytes(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut i = max_bytes;
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    &s[..i]
}

/// Compact number formatting for generated markup: at most 6 decimals, no trailing
/// zeros, never "-0".
pub fn fmt_num(v: f64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let mut s = format!("{v:.6}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" {
        s = "0".into();
    }
    s
}

/// Human-readable byte size: "512 bytes", "12.5 KB", "31.2 MB" (whole values have no decimal).
pub fn fmt_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let b = bytes as f64;
    let (v, unit) = if b >= MB {
        (b / MB, "MB")
    } else if b >= KB {
        (b / KB, "KB")
    } else {
        return format!("{bytes} bytes");
    };
    if (v - v.round()).abs() < 1e-9 {
        format!("{} {unit}", v.round() as u64)
    } else {
        format!("{v:.1} {unit}")
    }
}

pub fn group_thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

// ---------------------------------------------------------------------------
// Raw start-tag scanning (byte preserving).
// ---------------------------------------------------------------------------

/// One attribute exactly as written in the source.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawAttr<'a> {
    /// Qualified name, e.g. `viewBox`, `xmlns:xlink`.
    pub qname: &'a str,
    /// Raw value between the quotes (entity references not expanded).
    pub value: &'a str,
    /// The full raw `name="value"` text.
    pub raw: &'a str,
}

impl RawAttr<'_> {
    /// Unprefixed attribute (no namespace) with this exact name.
    pub fn is_plain(&self, name: &str) -> bool {
        self.qname == name
    }
}

/// A start tag scanned from source text.
#[derive(Debug, Clone, PartialEq)]
pub struct StartTag<'a> {
    pub qname: &'a str,
    pub attrs: Vec<RawAttr<'a>>,
    pub self_closing: bool,
    /// Byte offset where the tag starts (`<`).
    pub start: usize,
    /// Byte offset just past the closing `>`.
    pub end: usize,
}

impl StartTag<'_> {
    /// Prefix of the element's qualified name (`"svg"` for `<svg:svg>`), if any.
    pub fn prefix(&self) -> Option<&str> {
        self.qname.split_once(':').map(|(p, _)| p)
    }
    pub fn attr(&self, qname: &str) -> Option<&RawAttr<'_>> {
        self.attrs.iter().find(|a| a.qname == qname)
    }
}

fn is_name_char(c: u8) -> bool {
    !(c.is_ascii_whitespace()
        || c == b'='
        || c == b'>'
        || c == b'/'
        || c == b'<'
        || c == b'"'
        || c == b'\'')
}

/// Scan the start tag beginning at byte `start` (which must point to `<`).
pub fn scan_start_tag(text: &str, start: usize) -> Option<StartTag<'_>> {
    let b = text.as_bytes();
    if b.get(start) != Some(&b'<') {
        return None;
    }
    let mut i = start + 1;
    let name_start = i;
    while i < b.len() && is_name_char(b[i]) {
        i += 1;
    }
    if i == name_start {
        return None;
    }
    let qname = &text[name_start..i];
    let mut attrs = Vec::new();
    loop {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        match b.get(i)? {
            b'>' => {
                return Some(StartTag {
                    qname,
                    attrs,
                    self_closing: false,
                    start,
                    end: i + 1,
                })
            }
            b'/' => {
                return (b.get(i + 1) == Some(&b'>')).then(|| StartTag {
                    qname,
                    attrs,
                    self_closing: true,
                    start,
                    end: i + 2,
                });
            }
            _ => {}
        }
        let a_start = i;
        while i < b.len() && is_name_char(b[i]) {
            i += 1;
        }
        if i == a_start {
            return None;
        }
        let a_name = &text[a_start..i];
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if b.get(i) != Some(&b'=') {
            return None;
        }
        i += 1;
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let quote = *b.get(i)?;
        if quote != b'"' && quote != b'\'' {
            return None;
        }
        let v_start = i + 1;
        let v_end = v_start + memchr::memchr(quote, &b[v_start..])?;
        i = v_end + 1;
        attrs.push(RawAttr {
            qname: a_name,
            value: &text[v_start..v_end],
            raw: &text[a_start..i],
        });
    }
}

/// Byte layout of the root element in the source text.
#[derive(Debug, Clone, PartialEq)]
pub struct RootLayout<'a> {
    pub tag: StartTag<'a>,
    /// Range of the root's raw content (between start and end tag); empty when self-closing.
    pub content: std::ops::Range<usize>,
    /// Byte offset just past the root's end tag (or self-closing start tag).
    pub end: usize,
}

/// Locate the root element's start tag, content and end tag in the raw text.
pub fn root_layout<'t>(text: &'t str, root: Node<'_, '_>) -> Option<RootLayout<'t>> {
    let range = root.range();
    if range.end > text.len() || range.start >= range.end {
        return None;
    }
    let tag = scan_start_tag(text, range.start)?;
    if tag.self_closing {
        return Some(RootLayout {
            content: tag.end..tag.end,
            end: tag.end,
            tag,
        });
    }
    let slice = &text.as_bytes()[tag.end..range.end];
    let close = tag.end + memchr::memmem::rfind(slice, b"</")?;
    Some(RootLayout {
        content: tag.end..close,
        end: range.end,
        tag,
    })
}

/// Return `text` with the root start tag rebuilt via [`build_start_tag`]; everything
/// else (prolog, content, end tag) is kept byte-for-byte.
pub fn rewrite_root(
    text: &str,
    layout: &RootLayout<'_>,
    remove: &[&str],
    add: &[(String, String)],
) -> String {
    let tag = &layout.tag;
    let mut out = String::with_capacity(text.len() + 128);
    out.push_str(&text[..tag.start]);
    out.push_str(&build_start_tag(tag, remove, add));
    out.push_str(if tag.self_closing { "/>" } else { ">" });
    out.push_str(&text[tag.end..]);
    out
}

/// Rebuild the root start tag *without* its closing `>` / `/>`: keep all original
/// attributes (raw) except those whose qualified name is in `remove`, then append
/// `add` (name, already-escaped value).
pub fn build_start_tag(tag: &StartTag<'_>, remove: &[&str], add: &[(String, String)]) -> String {
    let mut s = String::with_capacity(tag.end - tag.start + 64);
    s.push('<');
    s.push_str(tag.qname);
    for a in &tag.attrs {
        if remove.contains(&a.qname) {
            continue;
        }
        s.push(' ');
        s.push_str(a.raw);
    }
    for (k, v) in add {
        s.push(' ');
        s.push_str(k);
        s.push_str("=\"");
        s.push_str(v);
        s.push('"');
    }
    s
}

/// The `xmlns` declaration needed so a no-namespace `<svg>` root becomes real SVG.
pub fn needs_svg_xmlns(root: Node<'_, '_>) -> bool {
    root.tag_name().namespace().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths() {
        assert_eq!(parse_length("10"), Some(10.0));
        assert_eq!(parse_length(" 10px "), Some(10.0));
        assert_eq!(parse_length("72pt"), Some(96.0));
        assert_eq!(parse_length("1pc"), Some(16.0));
        assert!((parse_length("25.4mm").unwrap() - 96.0).abs() < 1e-9);
        assert!((parse_length("2.54cm").unwrap() - 96.0).abs() < 1e-9);
        assert_eq!(parse_length("1in"), Some(96.0));
        assert_eq!(parse_length("2em"), Some(32.0));
        assert_eq!(parse_length("1ex"), Some(8.0));
        assert_eq!(parse_length("1e2"), Some(100.0));
        assert_eq!(parse_length("1.5E1px"), Some(15.0));
        assert_eq!(parse_length(".5"), Some(0.5));
        assert_eq!(parse_length("50%"), None);
        assert_eq!(parse_length("auto"), None);
        assert_eq!(parse_length(""), None);
        assert_eq!(parse_length("10furlongs"), None);
        assert_eq!(parse_positive_length("0"), None);
        assert_eq!(parse_positive_length("-5"), None);
    }

    #[test]
    fn view_boxes() {
        let v = parse_view_box("0 0 100 50").unwrap();
        assert_eq!(
            (v.min_x, v.min_y, v.width, v.height),
            (0.0, 0.0, 100.0, 50.0)
        );
        let v = parse_view_box("-10,-20,30,40").unwrap();
        assert_eq!(
            (v.min_x, v.min_y, v.width, v.height),
            (-10.0, -20.0, 30.0, 40.0)
        );
        let v = parse_view_box(" 1 , 2  3,4 ").unwrap();
        assert_eq!((v.min_x, v.width), (1.0, 3.0));
        assert!(parse_view_box("0 0 0 10").is_none());
        assert!(parse_view_box("0 0 -1 10").is_none());
        assert!(parse_view_box("0 0 10").is_none());
        assert!(parse_view_box("0 0 10 10 10").is_none());
        assert!(parse_view_box("a b c d").is_none());
    }

    #[test]
    fn numbers_and_sizes() {
        assert_eq!(fmt_num(1.0), "1");
        assert_eq!(fmt_num(-0.0), "0");
        assert_eq!(fmt_num(-0.0000001), "0");
        assert_eq!(fmt_num(1.25), "1.25");
        assert_eq!(fmt_num(1.0 / 3.0), "0.333333");
        assert_eq!(fmt_num(-12.5), "-12.5");
        assert_eq!(fmt_size(25 * 1024 * 1024), "25 MB");
        assert_eq!(fmt_size((31.2 * 1024.0 * 1024.0) as u64), "31.2 MB");
        assert_eq!(fmt_size(100), "100 bytes");
        assert_eq!(group_thousands(100000), "100,000");
    }

    #[test]
    fn start_tag_scan() {
        let t = r#"<?xml version="1.0"?><svg:svg xmlns:svg='http://www.w3.org/2000/svg' width = "10" a="x>y"/>"#;
        let start = t.find("<svg:svg").unwrap();
        let tag = scan_start_tag(t, start).unwrap();
        assert_eq!(tag.qname, "svg:svg");
        assert_eq!(tag.prefix(), Some("svg"));
        assert!(tag.self_closing);
        assert_eq!(tag.end, t.len());
        assert_eq!(tag.attrs.len(), 3);
        assert_eq!(tag.attr("width").unwrap().value, "10");
        assert_eq!(tag.attr("a").unwrap().value, "x>y");
        assert_eq!(tag.attrs[1].raw, r#"width = "10""#);
    }

    #[test]
    fn decoding() {
        assert_eq!(decode_text(b"\xEF\xBB\xBF<svg/>").unwrap().text, "<svg/>");
        let mut u16le = vec![0xFF, 0xFE];
        for u in "<svg/>".encode_utf16() {
            u16le.extend_from_slice(&u.to_le_bytes());
        }
        let d = decode_text(&u16le).unwrap();
        assert_eq!(d.text, "<svg/>");
        assert!(d.transcoded);
        let latin =
            b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><svg><title>caf\xE9</title></svg>";
        assert!(decode_text(latin).unwrap().text.contains("caf\u{e9}"));
        assert!(decode_text(b"<svg>\xFF</svg>").is_err());
    }
}

#[cfg(test)]
mod depth_tests {
    use super::*;

    #[test]
    fn depth_scanner() {
        assert_eq!(element_depth("<svg/>", 10), 0);
        assert_eq!(element_depth("<svg><g><g/></g></svg>", 10), 2);
        assert_eq!(
            element_depth("<svg><!-- <g><g><g> --><g a='>'>x</g></svg>", 10),
            2
        );
        assert_eq!(element_depth("<?xml version='1.0'?><!DOCTYPE svg [<!ENTITY a '<g>'>]><svg><![CDATA[<g><g>]]></svg>", 10), 1);
        let deep = format!("<svg>{}{}</svg>", "<g>".repeat(5000), "</g>".repeat(5000));
        assert!(element_depth(&deep, 256) > 256);
        assert!(matches!(
            parse_xml(&deep, u32::MAX - 1, 256),
            Err(ParseFailure::Limit(_))
        ));
    }
}
