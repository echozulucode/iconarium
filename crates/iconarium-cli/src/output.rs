//! Human-readable and `--json` output for each command.
//!
//! Everything goes through [`outln!`]: when stdout is closed early (`| head`,
//! `| Select-Object -First 5`), the process exits quietly instead of panicking.

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use iconarium_catalog::dto::*;
use iconarium_catalog::ops::Preview;
use serde::Serialize;

/// Write one line to stdout; exit quietly if the reader has gone away.
pub fn write_line(args: std::fmt::Arguments<'_>) {
    let mut out = std::io::stdout().lock();
    if let Err(e) = out.write_fmt(args).and_then(|()| out.write_all(b"\n")) {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            std::process::exit(0);
        }
        let _ = writeln!(std::io::stderr(), "error: cannot write output: {e}");
        std::process::exit(1);
    }
}

macro_rules! outln {
    () => {
        write_line(format_args!(""))
    };
    ($($t:tt)*) => {
        write_line(format_args!($($t)*))
    };
}

fn print_json<T: Serialize>(v: &T) {
    outln!("{}", serde_json::to_string_pretty(v).expect("serializable"));
}

fn notes(warnings: &[String]) {
    for w in warnings {
        eprintln!("note: {w}");
    }
}

pub fn doctor(index: &Path, json: bool) -> ExitCode {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Doctor {
        cli_version: &'static str,
        index: String,
        index_exists: bool,
        readable: bool,
        error: Option<String>,
        libraries: usize,
        assets: usize,
        thumbnails_dir: Option<String>,
        log_dir: Option<String>,
    }
    let opened = iconarium_catalog::IndexReader::open(index);
    let (readable, error, libraries, assets) = match &opened {
        Ok(r) => {
            let s = r.snapshot();
            (true, None, s.libraries.len(), s.asset_count())
        }
        Err(e) => (false, Some(e.to_string()), 0, 0),
    };
    let d = Doctor {
        cli_version: env!("CARGO_PKG_VERSION"),
        index: index.display().to_string(),
        index_exists: index.is_file(),
        readable,
        error,
        libraries,
        assets,
        thumbnails_dir: svg_core::paths::thumbnails_dir().map(|p| p.display().to_string()),
        log_dir: svg_core::paths::app_log_dir().map(|p| p.display().to_string()),
    };
    if json {
        print_json(&d);
    } else {
        outln!("iconarium-cli {}", d.cli_version);
        outln!(
            "index:       {}{}",
            d.index,
            if d.index_exists { "" } else { "  (not found)" }
        );
        match &d.error {
            None => outln!(
                "status:      ok — {} libraries, {} icons",
                d.libraries,
                d.assets
            ),
            Some(e) => outln!("status:      cannot read: {e}"),
        }
        if let Some(t) = &d.thumbnails_dir {
            outln!("thumbnails:  {t}");
        }
        if let Some(l) = &d.log_dir {
            outln!("logs:        {l}");
        }
    }
    if readable {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

pub fn libraries(libs: &[LibraryDto], json: bool) {
    if json {
        return print_json(&libs);
    }
    if libs.is_empty() {
        outln!("No libraries yet: open a folder in the Iconarium app.");
        return;
    }
    outln!(
        "{:>4}  {:<28} {:>7} {:>9}  ROOT",
        "ID",
        "NAME",
        "ICONS",
        "ANALYZED"
    );
    for l in libs {
        let name: String = l.name.chars().take(28).collect();
        outln!(
            "{:>4}  {:<28} {:>7} {:>9}  {}{}",
            l.id,
            name,
            l.assets,
            l.analyzed,
            l.root,
            if l.available { "" } else { "  (missing)" }
        );
        notes(&l.warnings);
    }
}

pub fn search(res: &SearchResponse, json: bool) {
    if json {
        return print_json(res);
    }
    for h in &res.hits {
        let tags = if h.tags.is_empty() {
            String::new()
        } else {
            format!("  [{}]", h.tags.join(", "))
        };
        let state = if h.state == "ready" || h.state == "discovered" {
            String::new()
        } else {
            format!("  ({})", h.state)
        };
        outln!(
            "{:>6}  {}  —  {}: {}{}{}",
            h.id,
            h.name,
            h.library,
            h.path,
            tags,
            state
        );
    }
    if res.total == 0 {
        outln!("No icons found.");
    } else if let Some(c) = &res.next_cursor {
        outln!(
            "({} of {}; next page: --cursor {c})",
            res.returned,
            res.total
        );
    } else {
        outln!("({} of {})", res.returned, res.total);
    }
    notes(&res.warnings);
}

pub fn detail(d: &IconDetail, json: bool) {
    if json {
        return print_json(d);
    }
    let kv = |k: &str, v: &str| outln!("{k:<13}{v}");
    kv("id", &d.id.to_string());
    kv("name", &d.name);
    kv("library", &format!("{} [id {}]", d.library, d.library_id));
    kv("path", &d.path);
    kv(
        "file",
        &format!(
            "{}{}",
            d.abs_path,
            if d.exists { "" } else { "  (missing)" }
        ),
    );
    kv("state", &d.state);
    if let Some(p) = &d.problem {
        kv("problem", p);
    }
    if let (Some(w), Some(h)) = (d.width, d.height) {
        kv("size", &format!("{w} × {h} px"));
    }
    if let Some(v) = d.view_box {
        kv("viewBox", &format!("{} {} {} {}", v[0], v[1], v[2], v[3]));
    }
    kv("bytes", &d.file_size.to_string());
    if !d.tags.is_empty() {
        kv(
            "tags",
            &d.tags
                .iter()
                .map(|t| format!("{} ({})", t.tag, t.source.as_str()))
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    for (k, v) in [
        ("note", &d.metadata_description),
        ("title", &d.title),
        ("description", &d.description),
        ("text", &d.visible_text),
        ("ids/classes", &d.identifiers),
    ] {
        if let Some(v) = v {
            kv(k, v);
        }
    }
    if let Some(dups) = &d.duplicates {
        if dups.is_empty() {
            kv("duplicates", "none");
        }
        for x in dups {
            kv("duplicate", &format!("#{} {}: {}", x.id, x.library, x.path));
        }
    }
    if let Some(svg) = &d.svg {
        outln!();
        outln!("{svg}");
        if d.svg_truncated {
            eprintln!("note: SVG truncated; raise --max-svg-bytes");
        }
    }
}

pub fn tags(t: &TagList, json: bool) {
    if json {
        return print_json(t);
    }
    for c in &t.tags {
        let src: Vec<&str> = c.sources.iter().map(|s| s.as_str()).collect();
        outln!("{:>6}  {}  ({})", c.count, c.tag, src.join(", "));
    }
    if t.total > t.tags.len() {
        outln!(
            "({} of {} tags; raise --limit for more)",
            t.tags.len(),
            t.total
        );
    }
}

pub fn folders(f: &FolderListing, json: bool) {
    if json {
        return print_json(f);
    }
    outln!("{} [id {}]: /{}", f.library, f.library_id, f.folder);
    for d in &f.folders {
        outln!("  {:<40} {:>6} icons", format!("{}/", d.name), d.assets);
    }
    for x in &f.files {
        outln!("  {:<40} #{}", x.file, x.id);
    }
    if f.more_files > 0 {
        outln!("  … and {} more files (raise --max-files)", f.more_files);
    }
}

pub fn preview(p: &Preview, out: &Path, json: bool) {
    if json {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Cell<'a> {
            cell: usize,
            id: u32,
            name: &'a str,
            library: &'a str,
            path: &'a str,
            problem: Option<&'a str>,
        }
        #[derive(Serialize)]
        struct Out<'a> {
            file: String,
            width: u32,
            height: u32,
            cells: Vec<Cell<'a>>,
        }
        return print_json(&Out {
            file: out.display().to_string(),
            width: p.width,
            height: p.height,
            cells: p
                .legend
                .iter()
                .map(|l| Cell {
                    cell: l.cell,
                    id: l.id,
                    name: &l.name,
                    library: &l.library,
                    path: &l.path,
                    problem: l.problem.as_deref(),
                })
                .collect(),
        });
    }
    outln!("wrote {} ({}×{})", out.display(), p.width, p.height);
    for l in &p.legend {
        let prob = l
            .problem
            .as_deref()
            .map(|p| format!("  — not rendered: {p}"))
            .unwrap_or_default();
        outln!(
            "{:>3}  #{}  {}  —  {}: {}{}",
            l.cell,
            l.id,
            l.name,
            l.library,
            l.path,
            prob
        );
    }
}

pub fn export(r: &ExportResult, json: bool) {
    if json {
        return print_json(r);
    }
    for w in &r.written {
        outln!("wrote  {}", w.path);
    }
    for s in &r.skipped {
        eprintln!("skipped #{}: {}", s.id, s.reason);
    }
}

pub fn tagged(r: &TagUpdateResult, json: bool) {
    if json {
        return print_json(r);
    }
    for u in &r.updated {
        outln!("#{} {}: [{}]", u.id, u.path, u.metadata_tags.join(", "));
    }
    for f in &r.files {
        outln!("updated {f}");
    }
}
