//! Searchable text extraction (plan §12): title, description, visible text, ids/classes.
//!
//! Normalization: XML entities are decoded by the parser; whitespace is collapsed and
//! trimmed; empty runs dropped; repeated identical runs (within a `<text>`) and lines
//! (across the document) de-duplicated; each text node is capped at
//! `limits.max_text_node_chars` characters; the whole result at
//! `limits.max_extracted_text_bytes` bytes (fields filled in order title, description,
//! visible text, identifiers; extraction stops once the budget is spent). Case is
//! preserved — the search layer lowercases.
//!
//! Text inside `<script>`, `<style>` and `<metadata>` is never part of visible text.
//! As a fallback when there is no SVG `<title>` / `<desc>`, Dublin Core
//! `dc:title` / `dc:description` (Inkscape RDF metadata) are used.

use std::collections::HashSet;

use roxmltree::{Document, Node};

use super::parser::{collapse_ws, is_svg_el, truncate_bytes, truncate_chars};
use crate::config::Limits;
use crate::model::SearchText;

const DC_NS: &str = "http://purl.org/dc/elements/1.1/";

struct Budget {
    left: usize,
}

impl Budget {
    fn exhausted(&self) -> bool {
        self.left == 0
    }
    /// Append `sep` (if `out` is non-empty) + `piece`, within budget. Returns false
    /// once the budget is exhausted.
    fn push(&mut self, out: &mut String, sep: &str, piece: &str) -> bool {
        if self.left == 0 {
            return false;
        }
        if !out.is_empty() {
            if sep.len() >= self.left {
                self.left = 0;
                return false;
            }
            out.push_str(sep);
            self.left -= sep.len();
        }
        let p = truncate_bytes(piece, self.left);
        out.push_str(p);
        self.left -= p.len();
        p.len() == piece.len() && self.left > 0
    }
}

fn normalize_node_text(raw: &str, max_chars: usize) -> String {
    let collapsed = collapse_ws(raw);
    truncate_chars(&collapsed, max_chars).trim_end().to_string()
}

/// Text of an element: its descendant text nodes, normalized, joined with spaces.
fn element_text(node: Node<'_, '_>, max_chars: usize) -> String {
    let mut runs: Vec<String> = Vec::new();
    for t in node.descendants().filter(|n| n.is_text()) {
        let s = normalize_node_text(t.text().unwrap_or(""), max_chars);
        if !s.is_empty() && runs.last() != Some(&s) {
            runs.push(s);
        }
    }
    runs.join(" ")
}

fn is_dc(node: Node<'_, '_>, local: &str) -> bool {
    node.is_element()
        && node.tag_name().name() == local
        && node.tag_name().namespace() == Some(DC_NS)
}

/// Root's direct child `<{name}>`, else the first `<{name}>` anywhere, else the first
/// Dublin Core `dc:{dc_name}`; first non-empty wins.
fn find_labelled(
    doc: &Document<'_>,
    root: Node<'_, '_>,
    name: &str,
    dc_name: &str,
    max_chars: usize,
) -> String {
    let direct = root.children().filter(|n| is_svg_el(*n, name));
    let anywhere = doc.descendants().filter(|n| is_svg_el(*n, name));
    let dc = doc.descendants().filter(|n| is_dc(*n, dc_name));
    for n in direct.chain(anywhere).chain(dc) {
        let t = element_text(n, max_chars);
        if !t.is_empty() {
            return t;
        }
    }
    String::new()
}

/// One line per `<text>` element: nested `<tspan>`/`<textPath>`/`<a>` runs joined by a space.
fn text_line(text_el: Node<'_, '_>, max_chars: usize) -> String {
    let mut runs: Vec<String> = Vec::new();
    for t in text_el.descendants().filter(|n| n.is_text()) {
        // Skip <title>/<desc> nested inside <text> (tooltips, not visible text).
        let in_label = t
            .ancestors()
            .take_while(|a| *a != text_el)
            .any(|a| is_svg_el(a, "title") || is_svg_el(a, "desc"));
        if in_label {
            continue;
        }
        let s = normalize_node_text(t.text().unwrap_or(""), max_chars);
        if !s.is_empty() && runs.last() != Some(&s) {
            runs.push(s);
        }
    }
    runs.join(" ")
}

/// Extract searchable text from a parsed document whose root is `<svg>`.
pub fn extract_text(doc: &Document<'_>, root: Node<'_, '_>, limits: &Limits) -> SearchText {
    let max_chars = limits.max_text_node_chars as usize;
    let mut budget = Budget {
        left: limits.max_extracted_text_bytes as usize,
    };
    let mut out = SearchText::default();

    let title = find_labelled(doc, root, "title", "title", max_chars);
    budget.push(&mut out.title, "", &title);
    let desc = find_labelled(doc, root, "desc", "description", max_chars);
    budget.push(&mut out.description, "", &desc);

    if !budget.exhausted() {
        let mut seen: HashSet<String> = HashSet::new();
        for el in doc.descendants().filter(|n| is_svg_el(*n, "text")) {
            let line = text_line(el, max_chars);
            if line.is_empty() || seen.contains(&line) {
                continue;
            }
            let more = budget.push(&mut out.visible_text, "\n", &line);
            seen.insert(line);
            if !more {
                break;
            }
        }
    }

    if !budget.exhausted() {
        let mut seen: HashSet<&str> = HashSet::new();
        'outer: for el in doc.descendants().filter(|n| n.is_element()) {
            let id = el
                .attribute("id")
                .or_else(|| el.attribute(("http://www.w3.org/XML/1998/namespace", "id")));
            let classes = el.attribute("class").unwrap_or("");
            for tok in id
                .map(str::trim)
                .into_iter()
                .chain(classes.split_whitespace())
            {
                if tok.is_empty() || !seen.insert(tok) {
                    continue;
                }
                let tok = truncate_chars(tok, max_chars);
                if !budget.push(&mut out.identifiers, " ", tok) {
                    break 'outer;
                }
            }
        }
    }
    out
}
