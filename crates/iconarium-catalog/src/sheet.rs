//! Contact sheets: many icons in one labelled PNG, so a model (or a person) can compare
//! candidates in a single image.
//!
//! Each icon renders independently through svg-core with the user's limits on a worker
//! thread with a 16 MiB stack (like the app's preview workers). A failure becomes a
//! placeholder cell, never a failed sheet. Cells are composited with tiny-skia, then one
//! SVG overlay draws all labels (text needs the font database).

use std::path::PathBuf;

use svg_core::config::Limits;
use svg_core::svg::tiny_skia::{Color, Paint, Pixmap, PixmapPaint, Rect, Transform};
use svg_core::svg::{render_pixmap, render_thumbnail_pixmap, Background};

use crate::error::{CatalogError, Result};

/// Stack size for render threads; parsing and rendering recurse per nesting level.
pub const RENDER_STACK_BYTES: usize = 16 * 1024 * 1024;
/// Maximum cells in one sheet.
pub const MAX_CELLS: usize = 48;
/// Sheets wrap to keep the image at most about this wide.
const MAX_SHEET_WIDTH: u32 = 1024;
const LABEL_HEIGHT: u32 = 18;
const LABEL_FONTS: &str = "Segoe UI, Arial, Helvetica, Liberation Sans, DejaVu Sans, sans-serif";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SheetBackground {
    #[default]
    White,
    Checker,
    Transparent,
}

/// One cell: its label and the SVG to draw (`Err` = show a placeholder with the reason).
#[derive(Debug)]
pub struct SheetCell {
    pub label: String,
    pub svg: std::result::Result<Vec<u8>, String>,
    pub resources_dir: Option<PathBuf>,
}

#[derive(Debug)]
pub struct SheetOutput {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Per cell: `None` if rendered, else why not.
    pub problems: Vec<Option<String>>,
}

/// Run `f` on a thread with a [`RENDER_STACK_BYTES`] stack and wait for it.
pub fn with_render_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .name("render".into())
            .stack_size(RENDER_STACK_BYTES)
            .spawn_scoped(s, f)
            .expect("spawn render thread")
            .join()
            .unwrap_or_else(|p| std::panic::resume_unwind(p))
    })
}

fn xml_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => o.push(' '),
            c => o.push(c),
        }
    }
    o
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

fn fill(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, c: Color) {
    if let Some(r) = Rect::from_xywh(x, y, w, h) {
        let mut p = Paint::default();
        p.set_color(c);
        pm.fill_rect(r, &p, Transform::identity(), None);
    }
}

/// Render cells into one PNG. `cell` is the square cell size in px (32–512).
pub fn render_sheet(
    cells: Vec<SheetCell>,
    cell: u32,
    background: SheetBackground,
    labels: bool,
    limits: &Limits,
) -> Result<SheetOutput> {
    if cells.is_empty() {
        return Err(CatalogError::Invalid("nothing to preview".into()));
    }
    if cells.len() > MAX_CELLS {
        return Err(CatalogError::Invalid(format!(
            "at most {MAX_CELLS} icons per preview (got {})",
            cells.len()
        )));
    }
    let cell = cell.clamp(32, 512);
    let n = cells.len() as u32;
    let cols = n.min((MAX_SHEET_WIDTH / cell).max(1));
    let rows = n.div_ceil(cols);
    let label_h = if labels { LABEL_HEIGHT } else { 0 };
    let pad = (cell / 16).max(4);
    let inner = cell - 2 * pad;
    let (w, h) = (cols * cell, rows * (cell + label_h));

    // Render cells in parallel on big-stack threads.
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .clamp(1, 4)
        .min(cells.len());
    let rendered: Vec<std::result::Result<Pixmap, String>> = std::thread::scope(|s| {
        let chunks: Vec<&[SheetCell]> = cells.chunks(cells.len().div_ceil(threads)).collect();
        let handles: Vec<_> = chunks
            .into_iter()
            .map(|chunk| {
                std::thread::Builder::new()
                    .name("sheet-render".into())
                    .stack_size(RENDER_STACK_BYTES)
                    .spawn_scoped(s, move || {
                        chunk
                            .iter()
                            .map(|c| match &c.svg {
                                Ok(bytes) => render_thumbnail_pixmap(
                                    bytes,
                                    inner,
                                    limits,
                                    c.resources_dir.as_deref(),
                                )
                                .map_err(|e| e.to_string()),
                                Err(why) => Err(why.clone()),
                            })
                            .collect::<Vec<_>>()
                    })
                    .expect("spawn render thread")
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_else(|p| std::panic::resume_unwind(p)))
            .collect()
    });

    let mut sheet = Pixmap::new(w, h)
        .ok_or_else(|| CatalogError::Invalid(format!("cannot allocate a {w}×{h} sheet")))?;
    match background {
        SheetBackground::White => sheet.fill(Color::WHITE),
        SheetBackground::Transparent => {}
        SheetBackground::Checker => {
            sheet.fill(Color::WHITE);
            let g = Color::from_rgba8(0xe6, 0xe6, 0xe6, 0xff);
            let sq = 8u32;
            for y in (0..h).step_by(sq as usize) {
                for x in (0..w).step_by(sq as usize) {
                    if ((x / sq) + (y / sq)).is_multiple_of(2) {
                        fill(&mut sheet, x as f32, y as f32, sq as f32, sq as f32, g);
                    }
                }
            }
        }
    }

    let grid = Color::from_rgba8(0xd8, 0xd8, 0xd8, 0xff);
    let mut overlay = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" font-family="{LABEL_FONTS}">"#
    );
    let max_label_chars = ((cell.saturating_sub(8)) as f32 / 6.4).floor().max(4.0) as usize;
    let mut problems = Vec::with_capacity(cells.len());
    for (i, (c, r)) in cells.iter().zip(rendered).enumerate() {
        let (col, row) = (i as u32 % cols, i as u32 / cols);
        let (x0, y0) = (col * cell, row * (cell + label_h));
        // Cell frame (1px).
        fill(&mut sheet, x0 as f32, y0 as f32, cell as f32, 1.0, grid);
        fill(
            &mut sheet,
            x0 as f32,
            y0 as f32,
            1.0,
            (cell + label_h) as f32,
            grid,
        );
        match r {
            Ok(pm) => {
                let x = x0 + (cell - pm.width()) / 2;
                let y = y0 + (cell - pm.height()) / 2;
                sheet.draw_pixmap(
                    x as i32,
                    y as i32,
                    pm.as_ref(),
                    &PixmapPaint::default(),
                    Transform::identity(),
                    None,
                );
                problems.push(None);
            }
            Err(why) => {
                let shade = Color::from_rgba8(0xf0, 0xf0, 0xf0, 0xff);
                fill(
                    &mut sheet,
                    (x0 + pad) as f32,
                    (y0 + pad) as f32,
                    inner as f32,
                    inner as f32,
                    shade,
                );
                overlay.push_str(&format!(
                    r##"<text x="{}" y="{}" font-size="11" fill="#888" text-anchor="middle">not rendered</text>"##,
                    x0 + cell / 2,
                    y0 + cell / 2 + 4
                ));
                problems.push(Some(why));
            }
        }
        if labels {
            let label = format!("{} {}", i + 1, c.label);
            overlay.push_str(&format!(
                r##"<text x="{}" y="{}" font-size="11" fill="#333">{}</text>"##,
                x0 + 4,
                y0 + cell + label_h - 5,
                xml_escape(&truncate(&label, max_label_chars))
            ));
        }
    }
    // Right and bottom edges.
    fill(&mut sheet, (w - 1) as f32, 0.0, 1.0, h as f32, grid);
    fill(&mut sheet, 0.0, (h - 1) as f32, w as f32, 1.0, grid);
    overlay.push_str("</svg>");

    let overlay_limits = Limits {
        max_nodes: limits.max_nodes.max(10_000),
        max_render_text_chars: limits.max_render_text_chars.max(100_000),
        ..limits.clone()
    };
    if let Ok(text) = with_render_stack(|| {
        render_pixmap(
            overlay.as_bytes(),
            1.0,
            Background::Transparent,
            &overlay_limits,
            None,
        )
    }) {
        sheet.draw_pixmap(
            0,
            0,
            text.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
    }
    let png = sheet
        .encode_png()
        .map_err(|e| CatalogError::Invalid(format!("PNG encoding failed: {e}")))?;
    Ok(SheetOutput {
        png,
        width: w,
        height: h,
        problems,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svg(color: &str) -> Vec<u8> {
        format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><circle cx="5" cy="5" r="5" fill="{color}"/></svg>"#).into_bytes()
    }

    #[test]
    fn layout_and_placeholders() {
        let cells: Vec<SheetCell> = (0..10)
            .map(|i| SheetCell {
                label: format!("icon <{i}> & co"),
                svg: if i == 3 {
                    Err("over a limit".into())
                } else if i == 4 {
                    Ok(b"<svg".to_vec())
                } else {
                    Ok(svg("#e63946"))
                },
                resources_dir: None,
            })
            .collect();
        let out =
            render_sheet(cells, 128, SheetBackground::White, true, &Limits::default()).unwrap();
        assert_eq!((out.width, out.height), (8 * 128, 2 * (128 + LABEL_HEIGHT)));
        assert_eq!(out.problems.iter().filter(|p| p.is_some()).count(), 2);
        assert_eq!(out.problems[3].as_deref(), Some("over a limit"));
        let pm = Pixmap::decode_png(&out.png).unwrap();
        // Centre of cell 0 is the red circle.
        let px = pm.pixel(64, 64).unwrap();
        assert!(px.red() > 200 && px.green() < 100, "{px:?}");
    }

    #[test]
    fn limits_and_single_cell() {
        assert!(
            render_sheet(vec![], 64, SheetBackground::White, true, &Limits::default()).is_err()
        );
        let many = (0..49)
            .map(|_| SheetCell {
                label: String::new(),
                svg: Ok(svg("red")),
                resources_dir: None,
            })
            .collect();
        assert!(render_sheet(many, 64, SheetBackground::White, true, &Limits::default()).is_err());
        let one = vec![SheetCell {
            label: "x".into(),
            svg: Ok(svg("blue")),
            resources_dir: None,
        }];
        let out = render_sheet(
            one,
            400,
            SheetBackground::Transparent,
            false,
            &Limits::default(),
        )
        .unwrap();
        assert_eq!((out.width, out.height), (400, 400));
    }
}
