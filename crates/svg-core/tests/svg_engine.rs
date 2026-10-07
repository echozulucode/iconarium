//! Integration tests for the SVG engine (agent B1). Run with `cargo test -p svg-core svg`.

use std::borrow::Cow;

use resvg::tiny_skia::{Pixmap, Transform};
use svg_core::config::Limits;
use svg_core::model::{ProcessingState, Region, SvgMeta};
use svg_core::svg::{
    self, analyze, crop_svg, prepare_for_viewer, render_png, render_region_png, render_thumbnail,
    Background,
};

fn decode(png: &[u8]) -> Pixmap {
    Pixmap::decode_png(png).expect("valid PNG")
}

/// Premultiplied RGBA at (x, y).
fn px(p: &Pixmap, x: u32, y: u32) -> [u8; 4] {
    let c = p.pixel(x, y).unwrap();
    [c.red(), c.green(), c.blue(), c.alpha()]
}

mod svg_analyze {
    use super::*;

    #[test]
    fn zero_byte_file() {
        let a = analyze(b"", &Limits::default());
        assert_eq!(a.state, ProcessingState::ParseError);
        assert_eq!(a.error.as_deref(), Some("Empty file"));
        assert_eq!(a.content_hash, blake3::hash(b"").to_hex().to_string());
    }

    #[test]
    fn malformed_xml() {
        let a = analyze(
            b"<svg xmlns='http://www.w3.org/2000/svg'><g></svg>",
            &Limits::default(),
        );
        assert_eq!(a.state, ProcessingState::ParseError);
        assert!(a.error.unwrap().starts_with("Malformed XML"));
        let a = analyze(b"not xml at all", &Limits::default());
        assert_eq!(a.state, ProcessingState::ParseError);
        let a = analyze(b"<svg>\xFF\xFE</svg>", &Limits::default());
        assert_eq!(a.state, ProcessingState::ParseError);
    }

    #[test]
    fn non_svg_root() {
        let a = analyze(b"<html><body/></html>", &Limits::default());
        assert_eq!(a.state, ProcessingState::ParseError);
        assert!(a.error.unwrap().contains("<html>"));
        // An <svg> root in a foreign namespace is not SVG either.
        let a = analyze(b"<svg xmlns='urn:other'/>", &Limits::default());
        assert_eq!(a.state, ProcessingState::ParseError);
    }

    #[test]
    fn oversize_file() {
        let limits = Limits {
            max_file_bytes: 100,
            ..Default::default()
        };
        let mut doc = String::from("<svg xmlns='http://www.w3.org/2000/svg'>");
        doc.push_str(&"<!-- padding -->".repeat(20));
        doc.push_str("</svg>");
        let a = analyze(doc.as_bytes(), &limits);
        assert_eq!(a.state, ProcessingState::LimitExceeded);
        let e = a.error.unwrap();
        assert!(e.starts_with("File exceeds rendering limit ("), "{e}");
        assert!(e.ends_with("> 100 bytes)"), "{e}");
        assert_eq!(a.complexity.file_size, doc.len() as u64);
        assert!(!a.content_hash.is_empty());
        // Renderer refuses as well.
        assert!(matches!(
            render_thumbnail(doc.as_bytes(), 64, &limits, None),
            Err(svg_core::CoreError::Limit(_))
        ));
    }

    #[test]
    fn node_bomb() {
        let limits = Limits {
            max_nodes: 1000,
            ..Default::default()
        };
        let doc = format!(
            "<svg xmlns='http://www.w3.org/2000/svg'>{}</svg>",
            "<g/>".repeat(5000)
        );
        let a = analyze(doc.as_bytes(), &limits);
        assert_eq!(a.state, ProcessingState::LimitExceeded);
        assert!(a.error.unwrap().contains("Too many XML nodes"));
        assert!(matches!(
            render_png(doc.as_bytes(), 1.0, Background::Transparent, &limits, None),
            Err(svg_core::CoreError::Limit(_))
        ));
        // Under the limit is fine.
        let doc = format!(
            "<svg xmlns='http://www.w3.org/2000/svg'>{}</svg>",
            "<g/>".repeat(500)
        );
        assert_eq!(
            analyze(doc.as_bytes(), &limits).state,
            ProcessingState::Ready
        );
    }

    #[test]
    fn entity_bomb_does_not_hang_or_panic() {
        let doc = r#"<?xml version="1.0"?>
<!DOCTYPE svg [
 <!ENTITY a "aaaaaaaaaa">
 <!ENTITY b "&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;">
 <!ENTITY c "&b;&b;&b;&b;&b;&b;&b;&b;&b;&b;">
 <!ENTITY d "&c;&c;&c;&c;&c;&c;&c;&c;&c;&c;">
 <!ENTITY e "&d;&d;&d;&d;&d;&d;&d;&d;&d;&d;">
 <!ENTITY f "&e;&e;&e;&e;&e;&e;&e;&e;&e;&e;">
 <!ENTITY g "&f;&f;&f;&f;&f;&f;&f;&f;&f;&f;">
 <!ENTITY h "&g;&g;&g;&g;&g;&g;&g;&g;&g;&g;">
 <!ENTITY i "&h;&h;&h;&h;&h;&h;&h;&h;&h;&h;">
]>
<svg xmlns="http://www.w3.org/2000/svg"><text>&i;</text></svg>"#;
        let a = analyze(doc.as_bytes(), &Limits::default());
        assert_ne!(a.state, ProcessingState::Discovered);
        if a.state == ProcessingState::Ready {
            assert!(
                a.text.visible_text.len() <= Limits::default().max_extracted_text_bytes as usize
            );
        }
    }

    #[test]
    fn huge_data_uri_image() {
        let limits = Limits {
            max_embedded_raster_bytes: 1000,
            ..Default::default()
        };
        let payload = "A".repeat(4000); // ≈ 3000 decoded bytes
        let doc = format!(
            "<svg xmlns='http://www.w3.org/2000/svg' xmlns:xlink='http://www.w3.org/1999/xlink'>\
             <image width='10' height='10' xlink:href='data:image/png;base64,{payload}'/></svg>"
        );
        let a = analyze(doc.as_bytes(), &limits);
        assert_eq!(a.state, ProcessingState::LimitExceeded);
        assert_eq!(a.complexity.embedded_raster_bytes, 3000);
        assert!(a.error.unwrap().starts_with("Embedded images exceed limit"));
        // SVG 2 plain href counts too.
        let doc2 = doc.replace("xlink:href", "href");
        assert_eq!(
            analyze(doc2.as_bytes(), &limits).state,
            ProcessingState::LimitExceeded
        );
        assert_eq!(
            analyze(doc.as_bytes(), &Limits::default()).state,
            ProcessingState::Ready
        );
    }

    #[test]
    fn script_and_external_ref_flags() {
        let l = Limits::default();
        let plain = analyze(
            b"<svg xmlns='http://www.w3.org/2000/svg'><use href='#a'/><rect id='a'/></svg>",
            &l,
        );
        assert_eq!(plain.state, ProcessingState::Ready);
        assert!(!plain.complexity.has_scripts && !plain.complexity.has_external_refs);

        let s = analyze(
            b"<svg xmlns='http://www.w3.org/2000/svg'><script>alert(1)</script></svg>",
            &l,
        );
        assert!(s.complexity.has_scripts);
        let s = analyze(
            b"<svg xmlns='http://www.w3.org/2000/svg' onload='x()'/>",
            &l,
        );
        assert!(s.complexity.has_scripts);
        let s = analyze(
            b"<svg xmlns='http://www.w3.org/2000/svg'><a href='javascript:x()'/></svg>",
            &l,
        );
        assert!(s.complexity.has_scripts);

        let e = analyze(b"<svg xmlns='http://www.w3.org/2000/svg'><image href='https://example.com/a.png'/></svg>", &l);
        assert!(e.complexity.has_external_refs);
        let e = analyze(
            b"<svg xmlns='http://www.w3.org/2000/svg' xmlns:xlink='http://www.w3.org/1999/xlink'><image xlink:href='pics/a.png'/></svg>",
            &l,
        );
        assert!(e.complexity.has_external_refs);
        assert!(!e.complexity.has_scripts);
    }

    #[test]
    fn complexity_counts() {
        let doc = "<svg xmlns='http://www.w3.org/2000/svg'><text>abcdef</text><g/></svg>";
        let a = analyze(doc.as_bytes(), &Limits::default());
        assert_eq!(a.state, ProcessingState::Ready);
        // root node + svg + text + text node + g
        assert_eq!(a.complexity.node_count, 5);
        assert_eq!(a.complexity.max_text_node_chars, 6);
        assert_eq!(a.complexity.file_size, doc.len() as u64);
        assert_eq!(a.meta.element_count, 3);
    }
}

mod svg_metadata {
    use super::*;

    fn meta(doc: &str) -> SvgMeta {
        let a = analyze(doc.as_bytes(), &Limits::default());
        assert_eq!(a.state, ProcessingState::Ready, "{:?}", a.error);
        a.meta
    }

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn px_dimensions_with_viewbox() {
        let m = meta("<svg xmlns='http://www.w3.org/2000/svg' width='600' height='400px' viewBox='10 20 300 200'/>");
        assert_eq!(m.width, Some(600.0));
        assert_eq!(m.height, Some(400.0));
        let vb = m.view_box.unwrap();
        assert_eq!(
            (vb.min_x, vb.min_y, vb.width, vb.height),
            (10.0, 20.0, 300.0, 200.0)
        );
        assert_eq!(m.doc_box, m.view_box);
        assert_eq!(m.element_count, 1);
    }

    #[test]
    fn absolute_units_without_viewbox() {
        let m = meta("<svg xmlns='http://www.w3.org/2000/svg' width='100mm' height='72pt'/>");
        assert!(approx(m.width.unwrap(), 100.0 * 96.0 / 25.4));
        assert!(approx(m.height.unwrap(), 96.0));
        let db = m.doc_box.unwrap();
        assert_eq!((db.min_x, db.min_y), (0.0, 0.0));
        assert!(approx(db.width, 100.0 * 96.0 / 25.4) && approx(db.height, 96.0));
        let m = meta("<svg xmlns='http://www.w3.org/2000/svg' width='1in' height='2cm'/>");
        assert!(approx(m.width.unwrap(), 96.0) && approx(m.height.unwrap(), 2.0 * 96.0 / 2.54));
        let m = meta("<svg xmlns='http://www.w3.org/2000/svg' width='2pc' height='1em'/>");
        assert_eq!((m.width, m.height), (Some(32.0), Some(16.0)));
    }

    #[test]
    fn percent_and_comma_viewbox() {
        let m = meta("<svg xmlns='http://www.w3.org/2000/svg' width='100%' height='50%' viewBox='0,0,40,30'/>");
        assert_eq!((m.width, m.height), (None, None));
        let db = m.doc_box.unwrap();
        assert_eq!((db.width, db.height), (40.0, 30.0));
    }

    #[test]
    fn invalid_viewbox_falls_back_to_size() {
        let m = meta(
            "<svg xmlns='http://www.w3.org/2000/svg' width='50' height='20' viewBox='0 0 0 10'/>",
        );
        assert_eq!(m.view_box, None);
        let db = m.doc_box.unwrap();
        assert_eq!((db.width, db.height), (50.0, 20.0));
    }

    #[test]
    fn renderer_fallback_uses_content_extent() {
        // No viewBox, no size: usvg sizes the canvas to the content's right/bottom edge.
        let m = meta("<svg xmlns='http://www.w3.org/2000/svg'><rect x='10' y='10' width='50' height='30'/></svg>");
        assert_eq!((m.width, m.height, m.view_box), (None, None, None));
        let db = m.doc_box.unwrap();
        assert!(approx(db.width, 60.0) && approx(db.height, 40.0), "{db:?}");
        // Empty document: final fallback 0 0 100 100.
        let m = meta("<svg xmlns='http://www.w3.org/2000/svg'/>");
        let db = m.doc_box.unwrap();
        assert_eq!((db.width, db.height), (100.0, 100.0));
    }

    #[test]
    fn no_namespace_root_is_accepted() {
        let m = meta("<svg width='10' height='20'><rect/></svg>");
        assert_eq!((m.width, m.height), (Some(10.0), Some(20.0)));
        assert_eq!(m.element_count, 2);
    }
}

mod svg_text {
    use super::*;

    fn text(doc: &str, limits: &Limits) -> svg_core::model::SearchText {
        let a = analyze(doc.as_bytes(), limits);
        assert_eq!(a.state, ProcessingState::Ready, "{:?}", a.error);
        a.text
    }

    #[test]
    fn title_desc_text_ids() {
        let doc = r##"<svg xmlns="http://www.w3.org/2000/svg" id="root" class="icon  big">
  <g id="layer1" class="icon">
     <title>Nested title</title>
     <text id="t1">Hello <tspan class="em">World</tspan></text>
  </g>
  <title>  Main
     Title  </title>
  <desc>A &amp; B &#169; &lt;x&gt;</desc>
  <text>Second <textPath href="#p">on   path</textPath> <a><tspan>link</tspan></a></text>
  <text>Hello <tspan class="em">World</tspan></text>
  <script>var hidden = 1;</script>
  <style>.hidden{}</style>
</svg>"##;
        let t = text(doc, &Limits::default());
        assert_eq!(t.title, "Main Title");
        assert_eq!(t.description, "A & B \u{a9} <x>");
        assert_eq!(t.visible_text, "Hello World\nSecond on path link");
        assert_eq!(t.identifiers, "root icon big layer1 t1 em");
        assert!(!t.visible_text.contains("hidden"));
    }

    #[test]
    fn fallback_to_first_title_anywhere_and_dc() {
        let t = text(
            "<svg xmlns='http://www.w3.org/2000/svg'><g><title>Inner</title><desc>D</desc></g></svg>",
            &Limits::default(),
        );
        assert_eq!((t.title.as_str(), t.description.as_str()), ("Inner", "D"));
        let doc = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
   xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:cc="http://creativecommons.org/ns#">
  <metadata><rdf:RDF><cc:Work><dc:title>RDF Title</dc:title>
  <dc:description>RDF desc</dc:description></cc:Work></rdf:RDF></metadata></svg>"#;
        let t = text(doc, &Limits::default());
        assert_eq!(
            (t.title.as_str(), t.description.as_str()),
            ("RDF Title", "RDF desc")
        );
        assert!(t.visible_text.is_empty());
    }

    #[test]
    fn whitespace_and_case_and_dedupe() {
        let doc = "<svg xmlns='http://www.w3.org/2000/svg'>\
            <text>  MiXeD\n\t Case  </text><text>MiXeD Case</text><text>   </text>\
            <text><tspan>dup</tspan><tspan>dup</tspan><tspan>x</tspan></text></svg>";
        let t = text(doc, &Limits::default());
        assert_eq!(t.visible_text, "MiXeD Case\ndup x");
    }

    #[test]
    fn per_node_cap_char_safe() {
        let limits = Limits {
            max_text_node_chars: 5,
            ..Default::default()
        };
        let doc = "<svg xmlns='http://www.w3.org/2000/svg'><title>ééééééééé</title><text>abcdefghij</text></svg>";
        let t = text(doc, &limits);
        assert_eq!(t.title, "ééééé");
        assert_eq!(t.visible_text, "abcde");
    }

    #[test]
    fn total_cap() {
        let limits = Limits {
            max_extracted_text_bytes: 30,
            ..Default::default()
        };
        let mut doc =
            String::from("<svg xmlns='http://www.w3.org/2000/svg'><title>0123456789</title>");
        for i in 0..100 {
            doc.push_str(&format!("<text id='id{i}'>line number {i}</text>"));
        }
        doc.push_str("</svg>");
        let t = text(&doc, &limits);
        let total =
            t.title.len() + t.description.len() + t.visible_text.len() + t.identifiers.len();
        assert!(total <= 30, "{total}");
        assert_eq!(t.title, "0123456789");
        assert!(t.visible_text.starts_with("line number 0"));
        assert!(t.identifiers.is_empty());
    }

    #[test]
    fn total_cap_is_char_boundary_safe() {
        let limits = Limits {
            max_extracted_text_bytes: 5,
            ..Default::default()
        };
        let t = text(
            "<svg xmlns='http://www.w3.org/2000/svg'><title>ééé</title></svg>",
            &limits,
        );
        assert_eq!(t.title, "éé");
    }
}

const SCENE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="200" height="100" viewBox="0 0 200 100">
  <defs>
    <linearGradient id="grad" x1="0" x2="1"><stop offset="0" stop-color="#f00"/><stop offset="1" stop-color="#00f"/></linearGradient>
  </defs>
  <rect x="5" y="5" width="190" height="90" fill="url(#grad)"/>
  <circle cx="70" cy="45" r="23.3" fill="#0c0" fill-opacity="0.7" stroke="#000" stroke-width="1.5"/>
  <path d="M 20 80 L 180 10 L 150 90 Z" fill="none" stroke="#ff0" stroke-width="2.7"/>
</svg>"##;

mod svg_render {
    use super::*;

    #[test]
    fn thumbnail_fitted_and_transparent() {
        let doc = "<svg xmlns='http://www.w3.org/2000/svg' width='200' height='100'>\
                   <circle cx='100' cy='50' r='40' fill='red'/></svg>";
        let png = render_thumbnail(doc.as_bytes(), 256, &Limits::default(), None).unwrap();
        let p = decode(&png);
        assert_eq!((p.width(), p.height()), (256, 128));
        assert_eq!(px(&p, 0, 0)[3], 0);
        assert_eq!(px(&p, 255, 127)[3], 0);
        assert_eq!(px(&p, 128, 64), [255, 0, 0, 255]);

        let tall = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 10 40'><rect width='10' height='40'/></svg>";
        let p = decode(&render_thumbnail(tall.as_bytes(), 256, &Limits::default(), None).unwrap());
        assert_eq!((p.width(), p.height()), (64, 256));
    }

    #[test]
    fn thumbnail_errors() {
        assert!(matches!(
            render_thumbnail(b"", 256, &Limits::default(), None),
            Err(svg_core::CoreError::Parse(_))
        ));
        assert!(matches!(
            render_thumbnail(b"<g/>", 256, &Limits::default(), None),
            Err(svg_core::CoreError::Parse(_))
        ));
    }

    #[test]
    fn render_png_scale_and_background() {
        let l = Limits::default();
        let r = render_png(SCENE.as_bytes(), 2.0, Background::Transparent, &l, None).unwrap();
        assert_eq!((r.width, r.height), (400, 200));
        let p = decode(&r.png);
        assert_eq!((p.width(), p.height()), (400, 200));
        assert_eq!(px(&p, 0, 0)[3], 0);

        let r = render_png(SCENE.as_bytes(), 1.0, Background::White, &l, None).unwrap();
        assert_eq!((r.width, r.height), (200, 100));
        assert_eq!(px(&decode(&r.png), 0, 0), [255, 255, 255, 255]);

        assert!(matches!(
            render_png(SCENE.as_bytes(), 0.0, Background::White, &l, None),
            Err(svg_core::CoreError::Invalid(_))
        ));
    }

    #[test]
    fn render_png_pixel_clamp() {
        let l = Limits {
            max_render_pixels: 5000,
            ..Default::default()
        };
        let r = render_png(SCENE.as_bytes(), 4.0, Background::Transparent, &l, None).unwrap();
        assert!(
            r.width as u64 * r.height as u64 <= 5000,
            "{}x{}",
            r.width,
            r.height
        );
        assert!(r.width >= 95 && r.width <= 100, "{}", r.width);
        let p = decode(&r.png);
        assert_eq!((p.width(), p.height()), (r.width, r.height));
    }

    #[test]
    fn no_namespace_svg_renders() {
        let doc = "<svg width='10' height='10'><rect width='10' height='10' fill='blue'/></svg>";
        let r = render_png(
            doc.as_bytes(),
            1.0,
            Background::Transparent,
            &Limits::default(),
            None,
        )
        .unwrap();
        assert_eq!(px(&decode(&r.png), 5, 5), [0, 0, 255, 255]);
    }

    #[test]
    fn warm_up_is_idempotent() {
        svg::warm_up_fonts();
        svg::warm_up_fonts();
    }
}

mod svg_crop {
    use super::*;

    fn region(x: f64, y: f64, w: f64, h: f64) -> Region {
        Region {
            x,
            y,
            width: w,
            height: h,
        }
    }

    fn root_attr(svg: &str, name: &str) -> Option<String> {
        let doc = roxmltree::Document::parse_with_options(
            svg,
            roxmltree::ParsingOptions {
                allow_dtd: true,
                ..Default::default()
            },
        )
        .expect("crop output parses");
        doc.root_element().attribute(name).map(str::to_string)
    }

    #[test]
    fn basic_structure_and_preservation() {
        let src = r##"<?xml version="1.0" encoding="UTF-8"?>
<!-- leading comment -->
<!DOCTYPE svg [ <!ENTITY brand "#123456"> ]>
<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape" style="font-family:Arial" class="drawing" width="1000" height="500" viewBox="0 0 1000 500" preserveAspectRatio="xMidYMid meet" x="0" y="0">
  <defs><linearGradient id="g1"><stop offset="0" stop-color="&brand;"/></linearGradient>
  <pattern id="pat" width="4" height="4"/><symbol id="sym"><rect width="1" height="1"/></symbol></defs>
  <style><![CDATA[ .a > b { fill: red } ]]></style>
  <!-- inner comment -->
  <text id="label" inkscape:label="L" x="300" y="200">Hello &amp; welcome</text>
  <use xlink:href="#sym" x="10" y="10"/>
</svg>
<!-- trailing -->"##;
        let out = crop_svg(src.as_bytes(), region(250.0, 150.0, 400.5, 200.25)).unwrap();
        assert!(out.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- leading comment -->\n<!DOCTYPE svg [ <!ENTITY brand \"#123456\"> ]>\n<svg "));
        assert_eq!(
            root_attr(&out, "viewBox").as_deref(),
            Some("0 0 400.5 200.25")
        );
        assert_eq!(root_attr(&out, "width").as_deref(), Some("400.5"));
        assert_eq!(root_attr(&out, "height").as_deref(), Some("200.25"));
        assert_eq!(root_attr(&out, "preserveAspectRatio"), None);
        assert_eq!(root_attr(&out, "x"), None);
        assert_eq!(
            root_attr(&out, "style").as_deref(),
            Some("font-family:Arial")
        );
        assert_eq!(root_attr(&out, "class").as_deref(), Some("drawing"));
        assert!(out.contains(r#"xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape""#));
        assert!(out.contains(r#"transform="translate(-250 -150)""#));
        assert!(out.contains(r#"<rect x="0" y="0" width="400.5" height="200.25"/>"#));
        // Original content byte-for-byte (entities, CDATA, comments, text).
        for frag in [
            r#"<stop offset="0" stop-color="&brand;"/>"#,
            r#"<pattern id="pat" width="4" height="4"/>"#,
            "<![CDATA[ .a > b { fill: red } ]]>",
            "<!-- inner comment -->",
            r#"<text id="label" inkscape:label="L" x="300" y="200">Hello &amp; welcome</text>"#,
            r##"<use xlink:href="#sym" x="10" y="10"/>"##,
        ] {
            assert!(out.contains(frag), "missing {frag}");
        }
        assert!(out
            .trim_end()
            .ends_with("</g></g></svg>\n<!-- trailing -->"));
        // Text stays text (not outlined) and is still extractable.
        let a = analyze(out.as_bytes(), &Limits::default());
        assert_eq!(a.state, ProcessingState::Ready);
        assert_eq!(a.text.visible_text, "Hello & welcome");
        // Renders.
        render_png(
            out.as_bytes(),
            1.0,
            Background::Transparent,
            &Limits::default(),
            None,
        )
        .unwrap();
    }

    #[test]
    fn unique_clip_id() {
        let src = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 10 10'>\
                   <g id='svgb-crop-clip'/><g id='svgb-crop-clip-1'/></svg>";
        let out = crop_svg(src.as_bytes(), region(1.0, 1.0, 5.0, 5.0)).unwrap();
        assert!(out.contains(r#"<clipPath id="svgb-crop-clip-2">"#));
        assert!(out.contains(r#"clip-path="url(#svgb-crop-clip-2)""#));
        let doc = roxmltree::Document::parse(&out).unwrap();
        let mut ids: Vec<_> = doc
            .descendants()
            .filter_map(|n| n.attribute("id"))
            .collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }

    #[test]
    fn non_zero_viewbox_origin() {
        let src = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='-50 100 200 100'><rect x='0' y='120' width='10' height='10'/></svg>";
        let out = crop_svg(src.as_bytes(), region(-20.0, 110.5, 30.0, 40.0)).unwrap();
        assert!(out.contains(r#"transform="translate(20 -110.5)""#));
        assert_eq!(root_attr(&out, "viewBox").as_deref(), Some("0 0 30 40"));
        // No absolute width: W/H = region size.
        assert_eq!(root_attr(&out, "width").as_deref(), Some("30"));
    }

    #[test]
    fn absolute_unit_scaling() {
        let src = "<svg xmlns='http://www.w3.org/2000/svg' width='200mm' height='100mm' viewBox='0 0 200 100'/>";
        let out = crop_svg(src.as_bytes(), region(10.0, 10.0, 50.0, 25.0)).unwrap();
        // 1 user unit = 1 mm = 3.779528 px.
        assert_eq!(root_attr(&out, "width").as_deref(), Some("188.976378"));
        assert_eq!(root_attr(&out, "height").as_deref(), Some("94.488189"));
        assert_eq!(root_attr(&out, "viewBox").as_deref(), Some("0 0 50 25"));
        // Rendered size follows the physical size.
        let r = render_region_png(
            src.as_bytes(),
            region(10.0, 10.0, 50.0, 25.0),
            1.0,
            Background::Transparent,
            &Limits::default(),
            None,
        )
        .unwrap();
        assert_eq!((r.width, r.height), (189, 95));

        // Non-uniform scale with preserveAspectRatio="none".
        let src = "<svg xmlns='http://www.w3.org/2000/svg' width='400' height='100' viewBox='0 0 200 100' preserveAspectRatio='none'/>";
        let out = crop_svg(src.as_bytes(), region(0.0, 0.0, 10.0, 10.0)).unwrap();
        assert_eq!(root_attr(&out, "width").as_deref(), Some("20"));
        assert_eq!(root_attr(&out, "height").as_deref(), Some("10"));
        assert_eq!(
            root_attr(&out, "preserveAspectRatio").as_deref(),
            Some("none")
        );
    }

    #[test]
    fn self_closing_prefixed_and_no_namespace_roots() {
        let out = crop_svg(
            b"<svg xmlns='http://www.w3.org/2000/svg' width='10' height='10'/>",
            region(0.0, 0.0, 5.0, 5.0),
        )
        .unwrap();
        assert!(out.ends_with("</g></g></svg>"));
        root_attr(&out, "viewBox").unwrap();

        let src = "<svg:svg xmlns:svg='http://www.w3.org/2000/svg' viewBox='0 0 10 10'><svg:rect width='10' height='10' fill='red'/></svg:svg>";
        let out = crop_svg(src.as_bytes(), region(2.0, 2.0, 4.0, 4.0)).unwrap();
        assert!(out.contains("<svg:defs><svg:clipPath"));
        assert!(out.ends_with("</svg:g></svg:g></svg:svg>"));
        let r = render_region_png(
            src.as_bytes(),
            region(2.0, 2.0, 4.0, 4.0),
            1.0,
            Background::Transparent,
            &Limits::default(),
            None,
        )
        .unwrap();
        assert_eq!((r.width, r.height), (4, 4));
        assert_eq!(px(&decode(&r.png), 1, 1), [255, 0, 0, 255]);

        let out = crop_svg(
            b"<svg viewBox='0 0 10 10'><rect/></svg>",
            region(0.0, 0.0, 5.0, 5.0),
        )
        .unwrap();
        assert_eq!(root_attr(&out, "xmlns").as_deref(), None); // namespace decls are not attributes
        assert!(out.contains(r#"xmlns="http://www.w3.org/2000/svg""#));
    }

    #[test]
    fn invalid_regions_rejected() {
        for r in [
            region(0.0, 0.0, 0.0, 5.0),
            region(0.0, 0.0, 5.0, -1.0),
            region(f64::NAN, 0.0, 1.0, 1.0),
            region(0.0, 0.0, f64::INFINITY, 1.0),
        ] {
            assert!(matches!(
                crop_svg(SCENE.as_bytes(), r),
                Err(svg_core::CoreError::Invalid(_))
            ));
        }
        assert!(matches!(
            crop_svg(b"<html/>", region(0.0, 0.0, 1.0, 1.0)),
            Err(svg_core::CoreError::Parse(_))
        ));
    }

    /// Render the source at 1:1 and compare the region's pixels with render_region_png.
    fn assert_pixel_equivalent(src: &str, doc_origin: (f64, f64), r: Region) {
        let l = Limits::default();
        let full = decode(
            &render_png(src.as_bytes(), 1.0, Background::Transparent, &l, None)
                .unwrap()
                .png,
        );
        let crop =
            render_region_png(src.as_bytes(), r, 1.0, Background::Transparent, &l, None).unwrap();
        assert_eq!((crop.width, crop.height), (r.width as u32, r.height as u32));
        let cp = decode(&crop.png);
        let (ox, oy) = ((r.x - doc_origin.0) as u32, (r.y - doc_origin.1) as u32);
        let mut max_diff = 0i32;
        let mut opaque = 0;
        for y in 0..crop.height {
            for x in 0..crop.width {
                let a = px(&full, ox + x, oy + y);
                let b = px(&cp, x, y);
                if b[3] > 0 {
                    opaque += 1;
                }
                for c in 0..4 {
                    max_diff = max_diff.max((a[c] as i32 - b[c] as i32).abs());
                }
            }
        }
        assert!(opaque > 0, "crop rendered nothing");
        assert!(max_diff <= 2, "max channel difference {max_diff}");
    }

    #[test]
    fn pixel_equivalence() {
        assert_pixel_equivalent(SCENE, (0.0, 0.0), region(40.0, 20.0, 60.0, 50.0));
        assert_pixel_equivalent(SCENE, (0.0, 0.0), region(0.0, 0.0, 200.0, 100.0));
        assert_pixel_equivalent(SCENE, (0.0, 0.0), region(150.0, 3.0, 50.0, 97.0));
    }

    #[test]
    fn pixel_equivalence_with_text() {
        let src = SCENE.replace(
            "</svg>",
            r##"<text x="30" y="60" font-family="sans-serif" font-size="22" fill="#222">Crop me <tspan font-weight="bold">bold</tspan></text></svg>"##,
        );
        assert_pixel_equivalent(&src, (0.0, 0.0), region(20.0, 30.0, 150.0, 45.0));
        // The cropped SVG still contains real text, not outlines.
        let out = crop_svg(src.as_bytes(), region(20.0, 30.0, 150.0, 45.0)).unwrap();
        assert!(out.contains(">Crop me <tspan"));
    }

    #[test]
    fn pixel_equivalence_non_zero_origin() {
        let src = SCENE
            .replace(r#"viewBox="0 0 200 100""#, r#"viewBox="100 -50 200 100""#)
            .replace(
                "<rect x=\"5\"",
                "<g transform=\"translate(100 -50)\"><rect x=\"5\"",
            )
            .replace("</svg>", "</g></svg>");
        assert_pixel_equivalent(&src, (100.0, -50.0), region(140.0, -30.0, 60.0, 50.0));
    }

    #[test]
    fn outside_content_is_clipped() {
        // Content fills the whole document; the crop must show nothing outside the region
        // even when rendered onto a larger canvas (renderers ignore root overflow).
        let src = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'><rect x='-50' y='-50' width='200' height='200' fill='#08f'/></svg>";
        let out = crop_svg(src.as_bytes(), region(30.0, 30.0, 20.0, 20.0)).unwrap();
        let tree = svg_core::svg::renderer::parse_tree(&out, &Limits::default(), None).unwrap();
        let mut pm = Pixmap::new(60, 60).unwrap();
        resvg::render(
            &tree,
            Transform::from_translate(20.0, 20.0),
            &mut pm.as_mut(),
        );
        for y in 0..60 {
            for x in 0..60 {
                let inside = (20..40).contains(&x) && (20..40).contains(&y);
                let a = px(&pm, x, y)[3];
                if inside {
                    assert_eq!(a, 255, "({x},{y}) should be painted");
                } else {
                    assert_eq!(a, 0, "({x},{y}) outside region is visible");
                }
            }
        }
    }

    #[test]
    fn region_png_scale_and_white() {
        let l = Limits::default();
        let r = render_region_png(
            SCENE.as_bytes(),
            region(0.0, 0.0, 60.0, 50.0),
            2.0,
            Background::White,
            &l,
            None,
        )
        .unwrap();
        assert_eq!((r.width, r.height), (120, 100));
        let p = decode(&r.png);
        assert_eq!(px(&p, 0, 0), [255, 255, 255, 255]);
        let r = render_region_png(
            SCENE.as_bytes(),
            region(0.0, 0.0, 60.0, 50.0),
            1.0,
            Background::Transparent,
            &l,
            None,
        )
        .unwrap();
        assert_eq!(px(&decode(&r.png), 0, 0)[3], 0);
    }

    #[test]
    fn utf16_source_crops_to_utf8() {
        let src = "<?xml version=\"1.0\" encoding=\"UTF-16\"?><svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 10 10'><text>ü</text></svg>";
        let mut bytes = vec![0xFF, 0xFE];
        for u in src.encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        let out = crop_svg(&bytes, region(0.0, 0.0, 5.0, 5.0)).unwrap();
        assert!(out.starts_with(r#"<?xml version="1.0" encoding="UTF-8"?><svg"#));
        assert!(out.contains("<text>ü</text>"));
    }
}

mod svg_normalize {
    use super::*;

    fn meta_of(doc: &str) -> SvgMeta {
        analyze(doc.as_bytes(), &Limits::default()).meta
    }

    #[test]
    fn injects_view_box() {
        let doc = "<svg xmlns='http://www.w3.org/2000/svg' width='100' height='50'><rect/></svg>";
        let out = prepare_for_viewer(doc.as_bytes(), &meta_of(doc));
        let s = std::str::from_utf8(&out).unwrap();
        assert_eq!(s, "<svg xmlns='http://www.w3.org/2000/svg' width='100' height='50' viewBox=\"0 0 100 50\"><rect/></svg>");
    }

    #[test]
    fn unchanged_when_view_box_present() {
        let doc = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 10 10'/>";
        assert!(matches!(
            prepare_for_viewer(doc.as_bytes(), &meta_of(doc)),
            Cow::Borrowed(_)
        ));
        // Unparseable input is passed through.
        assert!(matches!(
            prepare_for_viewer(b"<oops", &SvgMeta::default()),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn injects_size_and_namespace() {
        let doc = "<svg><rect x='0' y='0' width='30' height='20'/></svg>";
        let meta = SvgMeta {
            doc_box: Some(svg_core::model::ViewBox {
                min_x: 0.0,
                min_y: 0.0,
                width: 30.0,
                height: 20.0,
            }),
            ..Default::default()
        };
        let out = prepare_for_viewer(doc.as_bytes(), &meta);
        let s = std::str::from_utf8(&out).unwrap();
        assert_eq!(
            s,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 30 20\" width=\"30\" height=\"20\"><rect x='0' y='0' width='30' height='20'/></svg>"
        );
    }

    #[test]
    fn keeps_preserve_aspect_ratio_none() {
        let doc = "<svg xmlns='http://www.w3.org/2000/svg' width='10mm' height='5mm' preserveAspectRatio='none'/>";
        let out = prepare_for_viewer(doc.as_bytes(), &meta_of(doc));
        let s = std::str::from_utf8(&out).unwrap();
        assert!(s.contains("preserveAspectRatio='none'"));
        assert!(s.contains("viewBox=\"0 0 37.795276 18.897638\""));
    }
}
