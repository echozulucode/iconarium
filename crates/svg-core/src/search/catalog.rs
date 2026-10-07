//! In-memory catalog of the active library (plan §16/§17, implementation-plan §2).
//!
//! Each asset keeps its [`AssetRecord`], its original [`SearchText`] (for snippets)
//! and one pre-lower-cased string holding every searchable field back to back, so a
//! search is a tight loop of `memmem` scans with no per-asset allocation.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::error::Result;
use crate::model::{AssetId, AssetRecord, AssetSummary, MatchField, MatchInfo, SearchText};

use super::query::{normalize_text, parse};
use super::ranking::{separator_normalize, CompiledQuery, Fields, NormMap};

/// Field slots inside [`Entry::lc`].
const F_PATH: usize = 0;
const F_STEM_NORM: usize = 1;
const F_TITLE: usize = 2;
const F_TEXT: usize = 3;
const F_DESC: usize = 4;
const F_IDS: usize = 5;
const NFIELDS: usize = 6;

#[derive(Debug)]
struct Entry {
    rec: AssetRecord,
    text: Option<Box<SearchText>>,
    /// `path_lc \n stem_norm \n title \n text \n desc \n ids` (all lower-cased; fields never
    /// contain `\n`, so whole-string scans are exact prefilters and `^`/`$` in multi-line
    /// regexes still anchor at field boundaries).
    lc: Box<str>,
    ranges: [(u32, u32); NFIELDS],
    /// Byte offset of the filename inside the path slot.
    name_start: u32,
    penalty: u16,
}

impl Entry {
    fn build(rec: AssetRecord, text: Option<SearchText>) -> Self {
        let text = text.filter(|t| !t.is_empty()).map(Box::new);
        let mut path_lc = rec.relative_path.to_lowercase();
        if path_lc.contains('\n') {
            path_lc = path_lc.replace('\n', " "); // keep '\n' reserved as the field separator
        }
        let name_start = path_lc.rfind('/').map(|i| i + 1).unwrap_or(0);
        let name = &path_lc[name_start..];
        let stem = match name.rfind('.') {
            Some(i) if i > 0 => &name[..i],
            _ => name,
        };
        let stem_norm = separator_normalize(stem);

        let mut lc = String::with_capacity(
            path_lc.len()
                + stem_norm.len()
                + 8
                + text.as_ref().map_or(0, |t| {
                    t.title.len() + t.visible_text.len() + t.description.len() + t.identifiers.len()
                }),
        );
        let mut ranges = [(0u32, 0u32); NFIELDS];
        let mut push = |lc: &mut String, slot: usize, s: &str| {
            if slot > 0 {
                lc.push('\n');
            }
            let start = lc.len() as u32;
            lc.push_str(s);
            ranges[slot] = (start, lc.len() as u32);
        };
        push(&mut lc, F_PATH, &path_lc);
        push(&mut lc, F_STEM_NORM, &stem_norm);
        let (title, body, desc, ids) = match &text {
            Some(t) => (
                normalize_text(&t.title),
                normalize_text(&t.visible_text),
                normalize_text(&t.description),
                normalize_text(&t.identifiers),
            ),
            None => Default::default(),
        };
        push(&mut lc, F_TITLE, &title);
        push(&mut lc, F_TEXT, &body);
        push(&mut lc, F_DESC, &desc);
        push(&mut lc, F_IDS, &ids);

        let depth = rec.relative_path.matches('/').count() as i64;
        let len = rec.relative_path.chars().count() as i64;
        let penalty = (depth * 3 + len.min(200) / 8).min(99) as u16;

        Entry {
            rec,
            text,
            lc: lc.into_boxed_str(),
            ranges,
            name_start: name_start as u32,
            penalty,
        }
    }

    #[inline]
    fn slot(&self, i: usize) -> &str {
        let (s, e) = self.ranges[i];
        &self.lc[s as usize..e as usize]
    }

    #[inline]
    fn fields(&self) -> Fields<'_> {
        let path = self.slot(F_PATH);
        Fields {
            all: &self.lc,
            name: &path[self.name_start as usize..],
            stem_norm: self.slot(F_STEM_NORM),
            path,
            title: self.slot(F_TITLE),
            text: self.slot(F_TEXT),
            desc: self.slot(F_DESC),
            ids: self.slot(F_IDS),
        }
    }
}

/// Path order key: case-insensitive, '/' sorts before every other character so a
/// directory's files stay together; original path and id break remaining ties.
fn path_key(e: &Entry) -> (Vec<u8>, &str, AssetId) {
    let key: Vec<u8> = e
        .slot(F_PATH)
        .bytes()
        .map(|c| if c == b'/' { 0 } else { c })
        .collect();
    (key, e.rec.relative_path.as_str(), e.rec.id)
}

#[derive(Debug)]
struct PathOrder {
    /// Entry slots sorted by path.
    sorted: Vec<u32>,
    /// `rank[slot]` = position in `sorted`.
    rank: Vec<u32>,
}

/// In-memory search index of one library.
#[derive(Debug, Default)]
pub struct Catalog {
    entries: Vec<Entry>,
    by_id: HashMap<AssetId, usize>,
    order: OnceLock<PathOrder>,
}

impl Catalog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.by_id.clear();
        self.order = OnceLock::new();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn invalidate(&mut self) {
        if self.order.get().is_some() {
            self.order = OnceLock::new();
        }
    }

    /// Bulk load (e.g. from [`crate::index::Database::load_catalog`]); upserts each item.
    pub fn extend(&mut self, items: impl IntoIterator<Item = (AssetRecord, Option<SearchText>)>) {
        let items = items.into_iter();
        self.entries.reserve(items.size_hint().0);
        for (rec, text) in items {
            self.upsert(rec, text);
        }
    }

    /// Insert or replace an asset together with its searchable text.
    pub fn upsert(&mut self, rec: AssetRecord, text: Option<SearchText>) {
        let id = rec.id;
        let entry = Entry::build(rec, text);
        match self.by_id.get(&id) {
            Some(&slot) => {
                let path_changed = self.entries[slot].rec.relative_path != entry.rec.relative_path;
                self.entries[slot] = entry;
                if path_changed {
                    self.invalidate();
                }
            }
            None => {
                self.by_id.insert(id, self.entries.len());
                self.entries.push(entry);
                self.invalidate();
            }
        }
    }

    /// Replace the record of an asset but keep its current searchable text.
    /// Inserts (without text) if the id is unknown.
    pub fn update_record(&mut self, rec: AssetRecord) {
        let text = self
            .by_id
            .get(&rec.id)
            .and_then(|&s| self.entries[s].text.as_deref().cloned());
        self.upsert(rec, text);
    }

    /// Set the searchable text of a known asset (no-op for unknown ids).
    pub fn set_text(&mut self, id: AssetId, text: SearchText) {
        if let Some(&slot) = self.by_id.get(&id) {
            let rec = self.entries[slot].rec.clone();
            self.entries[slot] = Entry::build(rec, Some(text));
        }
    }

    /// Remove an asset; returns whether it was present.
    pub fn remove(&mut self, id: AssetId) -> bool {
        let Some(slot) = self.by_id.remove(&id) else {
            return false;
        };
        self.entries.swap_remove(slot);
        if slot < self.entries.len() {
            let moved = self.entries[slot].rec.id;
            self.by_id.insert(moved, slot);
        }
        self.invalidate();
        true
    }

    pub fn get(&self, id: AssetId) -> Option<&AssetRecord> {
        self.by_id.get(&id).map(|&s| &self.entries[s].rec)
    }

    pub fn get_text(&self, id: AssetId) -> Option<&SearchText> {
        self.by_id
            .get(&id)
            .and_then(|&s| self.entries[s].text.as_deref())
    }

    pub fn summary(&self, id: AssetId) -> Option<AssetSummary> {
        self.get(id).map(AssetSummary::from)
    }

    /// All asset ids (unordered).
    pub fn ids(&self) -> impl Iterator<Item = AssetId> + '_ {
        self.entries.iter().map(|e| e.rec.id)
    }

    fn order(&self) -> &PathOrder {
        self.order.get_or_init(|| {
            let mut sorted: Vec<u32> = (0..self.entries.len() as u32).collect();
            sorted.sort_by_cached_key(|&s| path_key(&self.entries[s as usize]));
            let mut rank = vec![0u32; sorted.len()];
            for (pos, &slot) in sorted.iter().enumerate() {
                rank[slot as usize] = pos as u32;
            }
            PathOrder { sorted, rank }
        })
    }

    /// Ranked asset ids for `query` (see [`super::query`] for syntax). An empty query
    /// lists every asset in path order. `limit` truncates the result.
    pub fn search(&self, query: &str, limit: Option<usize>) -> Result<Vec<AssetId>> {
        let compiled = CompiledQuery::compile(&parse(query))?;
        let order = self.order();
        let limit = limit.unwrap_or(usize::MAX);
        let Some(q) = compiled else {
            return Ok(order
                .sorted
                .iter()
                .take(limit)
                .map(|&s| self.entries[s as usize].rec.id)
                .collect());
        };
        let mut hits: Vec<(i64, u32)> = Vec::new();
        for (slot, e) in self.entries.iter().enumerate() {
            if let Some(score) = q.score(&e.fields(), e.penalty as i64) {
                hits.push((score, order.rank[slot]));
            }
        }
        let cmp = |a: &(i64, u32), b: &(i64, u32)| b.0.cmp(&a.0).then(a.1.cmp(&b.1));
        if limit < hits.len() {
            hits.select_nth_unstable_by(limit, cmp);
            hits.truncate(limit);
        }
        hits.sort_unstable_by(cmp);
        Ok(hits
            .into_iter()
            .map(|(_, rank)| self.entries[order.sorted[rank as usize] as usize].rec.id)
            .collect())
    }

    /// Why `id` matched `query`, when the match comes from SVG content (title, visible
    /// text, description, ids/classes). `None` for filename/path matches, empty or
    /// invalid queries, non-matching or unknown assets.
    pub fn explain(&self, id: AssetId, query: &str) -> Option<MatchInfo> {
        let e = &self.entries[*self.by_id.get(&id)?];
        let q = CompiledQuery::compile(&parse(query)).ok()??;
        Self::explain_entry(e, &q)
    }

    fn explain_entry(e: &Entry, q: &CompiledQuery) -> Option<MatchInfo> {
        let t = e.text.as_deref()?;
        let (field, start, end) = q.explain_location(&e.fields())?;
        let original = match field {
            MatchField::Title => &t.title,
            MatchField::Text => &t.visible_text,
            MatchField::Desc => &t.description,
            MatchField::IdClass => &t.identifiers,
            MatchField::Filename | MatchField::Path => return None,
        };
        Some(MatchInfo {
            field,
            snippet: NormMap::new(original).snippet(start, end),
        })
    }

    /// Summaries (with match explanations when `query` is given) for a page of ids,
    /// in the given order; unknown ids are skipped. The query is compiled once.
    pub fn summaries(&self, ids: &[AssetId], query: Option<&str>) -> Vec<AssetSummary> {
        let compiled = query.and_then(|q| CompiledQuery::compile(&parse(q)).ok().flatten());
        ids.iter()
            .filter_map(|&id| {
                let e = &self.entries[*self.by_id.get(&id)?];
                let mut s = AssetSummary::from(&e.rec);
                s.match_info = compiled.as_ref().and_then(|q| Self::explain_entry(e, q));
                Some(s)
            })
            .collect()
    }
}
