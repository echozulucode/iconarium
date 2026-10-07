use svg_core::model::{AssetId, AssetRecord, MatchField, ProcessingState, SearchText};
use svg_core::search::Catalog;
use svg_core::CoreError;

fn rec(id: AssetId, path: &str) -> AssetRecord {
    AssetRecord {
        id,
        library_id: 1,
        relative_path: path.into(),
        filename: path.rsplit('/').next().unwrap().into(),
        file_size: 100,
        mtime_ns: 1,
        fast_fingerprint: format!("fp{id}"),
        content_hash: None,
        width: None,
        height: None,
        view_box: None,
        element_count: None,
        state: ProcessingState::Ready,
        parse_error: None,
    }
}

fn text(title: &str, visible: &str, desc: &str, ids: &str) -> Option<SearchText> {
    Some(SearchText {
        title: title.into(),
        visible_text: visible.into(),
        description: desc.into(),
        identifiers: ids.into(),
    })
}

/// A small library resembling plan §2.7 / §17 examples.
fn sample() -> Catalog {
    let mut c = Catalog::new();
    c.upsert(rec(1, "controls/motor-controller.svg"), None);
    c.upsert(
        rec(2, "systems/power-system.svg"),
        text("Power", "Main Bus\nMotor   Controller\nBreaker", "", "g1"),
    );
    c.upsert(
        rec(3, "systems/supervisory-controller.svg"),
        text("", "Motor A\nMotor B", "", ""),
    );
    c.upsert(rec(4, "network/ethernet-switch.svg"), None);
    c.upsert(rec(5, "network/ethernet.svg"), None);
    c.upsert(
        rec(6, "diagrams/system-architecture.svg"),
        text("", "CPU\nEthernet\nGPU", "", ""),
    );
    c.upsert(
        rec(7, "diagrams/system-overview.svg"),
        text("Overview", "Ethernet Control Interface", "", ""),
    );
    c.upsert(rec(8, "legacy/network/old-switch.svg"), None);
    c.upsert(
        rec(9, "icons/Pump.SVG"),
        text("", "", "centrifugal pump icon", "pump-body cls-impeller"),
    );
    c.upsert(rec(10, "icons/valve.svg"), text("Gate Valve", "", "", ""));
    c
}

fn search(c: &Catalog, q: &str) -> Vec<AssetId> {
    c.search(q, None).unwrap()
}

fn sorted(mut v: Vec<AssetId>) -> Vec<AssetId> {
    v.sort();
    v
}

#[test]
fn empty_query_lists_all_in_case_insensitive_path_order() {
    let c = sample();
    assert_eq!(search(&c, ""), vec![1, 6, 7, 9, 10, 8, 4, 5, 2, 3]);
    assert_eq!(search(&c, "   "), search(&c, ""));
    assert_eq!(c.search("", Some(3)).unwrap(), vec![1, 6, 7]);
}

#[test]
fn and_tokens_and_case_insensitivity() {
    let c = sample();
    assert_eq!(sorted(search(&c, "ETHERNET")), vec![4, 5, 6, 7]);
    assert_eq!(search(&c, "ethernet switch"), vec![4]);
    assert_eq!(sorted(search(&c, "network switch")), vec![4, 8]);
    assert!(search(&c, "ethernet nonexistent").is_empty());
}

#[test]
fn ethernet_example_ranks_filename_before_content() {
    let c = sample();
    let r = search(&c, "ethernet");
    // exact stem first, then filename word, then content matches.
    assert_eq!(&r[..2], &[5, 4]);
    assert_eq!(sorted(r[2..].to_vec()), vec![6, 7]);
}

#[test]
fn motor_controller_ranking_matches_plan_example() {
    let c = sample();
    let r = search(&c, "motor controller");
    assert_eq!(r, vec![1, 2, 3]);
    // Explanations: filename hit has none; content hits explain.
    assert!(c.explain(1, "motor controller").is_none());
    let m = c.explain(2, "motor controller").unwrap();
    assert_eq!(m.field, MatchField::Text);
    assert_eq!(m.snippet, "Main Bus Motor Controller Breaker");
    let m = c.explain(3, "motor controller").unwrap();
    assert_eq!(m.field, MatchField::Text);
    assert!(m.snippet.contains("Motor A"));
}

#[test]
fn phrases_and_exclusion() {
    let c = sample();
    assert_eq!(search(&c, r#""ethernet control""#), vec![7]);
    assert_eq!(search(&c, r#""control ethernet""#), Vec::<AssetId>::new());
    assert_eq!(sorted(search(&c, "ethernet -switch")), vec![5, 6, 7]);
    assert_eq!(
        sorted(search(&c, r#"ethernet -"control interface""#)),
        vec![4, 5, 6]
    );
    assert_eq!(sorted(search(&c, "switch -legacy")), vec![4]);
    // Exclusion only: everything except matches, in path order.
    assert_eq!(search(&c, "-systems -network"), vec![1, 6, 7, 9, 10]);
}

#[test]
fn globs_on_filename_and_path() {
    let c = sample();
    assert_eq!(sorted(search(&c, "*switch*")), vec![4, 8]);
    assert_eq!(sorted(search(&c, "ethernet*.svg")), vec![4, 5]);
    assert_eq!(sorted(search(&c, "PUMP.svg?")), Vec::<AssetId>::new());
    assert_eq!(search(&c, "p?mp.svg"), vec![9]);
    // Path glob is anchored at any directory: legacy/network/ also matches.
    assert_eq!(sorted(search(&c, "network/*switch*.svg")), vec![4, 8]);
    // Leading '/' anchors at the library root.
    assert_eq!(search(&c, "/network/*switch*.svg"), vec![4]);
    // '*' does not cross directories; '**' does.
    assert!(search(&c, "/legacy/*.svg").is_empty());
    assert_eq!(search(&c, "/legacy/**/*.svg"), vec![8]);
    // Glob combined with a plain token and exclusion.
    assert_eq!(search(&c, "*.svg ethernet -switch").len(), 3);
    assert!(c.search("[", None).is_ok()); // plain token, not a glob
    assert!(matches!(c.search("a[*", None), Err(CoreError::Query(_))));
}

#[test]
fn regex_queries() {
    let c = sample();
    assert_eq!(search(&c, "re:ethernet.*switch"), vec![4]);
    assert_eq!(sorted(search(&c, "re:^ethernet")), vec![4, 5, 7]);
    assert_eq!(search(&c, "RE:  Motor.Controller "), vec![1, 2]);
    // Content match explains.
    let m = c.explain(7, "re:control\\s+interface").unwrap();
    assert_eq!(m.field, MatchField::Text);
    assert_eq!(m.snippet, "Ethernet Control Interface");
    assert!(matches!(
        c.search("re:(unclosed", None),
        Err(CoreError::Query(_))
    ));
    assert!(c.explain(7, "re:(unclosed").is_none());
}

#[test]
fn content_fields_title_desc_ids() {
    let c = sample();
    assert_eq!(search(&c, "gate"), vec![10]);
    assert_eq!(c.explain(10, "gate").unwrap().field, MatchField::Title);
    assert_eq!(c.explain(10, "gate").unwrap().snippet, "Gate Valve");
    assert_eq!(search(&c, "centrifugal"), vec![9]);
    assert_eq!(c.explain(9, "centrifugal").unwrap().field, MatchField::Desc);
    assert_eq!(search(&c, "impeller"), vec![9]);
    assert_eq!(c.explain(9, "impeller").unwrap().field, MatchField::IdClass);
    // Filename hit wins: no explanation for the card.
    assert!(c.explain(9, "pump").is_none());
    assert!(c.explain(9, "").is_none());
    assert!(c.explain(9, "nomatch").is_none());
    // Title outranks visible text, which outranks description, which outranks ids.
    let mut c2 = Catalog::new();
    c2.upsert(rec(1, "a.svg"), text("", "", "", "zeta"));
    c2.upsert(rec(2, "b.svg"), text("", "", "zeta", ""));
    c2.upsert(rec(3, "c.svg"), text("", "zeta", "", ""));
    c2.upsert(rec(4, "d.svg"), text("zeta", "", "", ""));
    c2.upsert(rec(5, "zeta/e.svg"), None);
    c2.upsert(rec(6, "f-zetas.svg"), None);
    c2.upsert(rec(7, "zeta-g.svg"), None);
    c2.upsert(rec(8, "zeta.svg"), None);
    c2.upsert(rec(9, "fzeta.svg"), None);
    assert_eq!(search(&c2, "zeta"), vec![8, 7, 6, 9, 5, 4, 3, 2, 1]);
}

#[test]
fn snippet_is_bounded_and_centered() {
    let mut c = Catalog::new();
    let long = format!(
        "{}needle in the haystack{}",
        "word ".repeat(100),
        " tail".repeat(100)
    );
    c.upsert(rec(1, "x.svg"), text("", &long, "", ""));
    let m = c.explain(1, "needle").unwrap();
    assert!(m.snippet.chars().count() <= 120, "{}", m.snippet);
    assert!(m.snippet.starts_with('…') && m.snippet.ends_with('…'));
    assert!(m.snippet.contains("needle in the haystack"));
}

#[test]
fn incremental_upsert_update_remove() {
    let mut c = sample();
    assert_eq!(c.len(), 10);
    let all = search(&c, "");
    c.remove(4);
    assert!(!c.remove(4));
    assert_eq!(c.len(), 9);
    assert!(c.get(4).is_none());
    assert_eq!(search(&c, "ethernet switch"), Vec::<AssetId>::new());
    assert_eq!(
        search(&c, ""),
        all.iter().copied().filter(|&i| i != 4).collect::<Vec<_>>()
    );

    c.upsert(rec(11, "aaa/first.svg"), None);
    assert_eq!(search(&c, "")[0], 11);

    // Text arrives later (analysis), then the record is updated without losing it.
    assert!(search(&c, "turbine").is_empty());
    c.set_text(11, text("", "Wind Turbine", "", "").unwrap());
    assert_eq!(search(&c, "turbine"), vec![11]);
    let mut r = c.get(11).unwrap().clone();
    r.state = ProcessingState::LimitExceeded;
    c.update_record(r);
    assert_eq!(c.get(11).unwrap().state, ProcessingState::LimitExceeded);
    assert_eq!(search(&c, "turbine"), vec![11]);
    // Upsert replaces text.
    c.upsert(rec(11, "aaa/first.svg"), None);
    assert!(search(&c, "turbine").is_empty());

    let s = c.summary(2).unwrap();
    assert_eq!(s.filename, "power-system.svg");
    assert_eq!(s.rel_dir, "systems");
    assert_eq!(s.fingerprint, "fp2");
    let pages = c.summaries(&[2, 999, 1], Some("motor controller"));
    assert_eq!(pages.len(), 2);
    assert!(pages[0].match_info.is_some());
    assert!(pages[1].match_info.is_none());

    c.clear();
    assert_eq!(c.len(), 0);
    assert!(search(&c, "").is_empty());
}

#[test]
fn unicode_text_and_snippets() {
    let mut c = Catalog::new();
    c.upsert(
        rec(1, "Ünïcode/Straße-Plan.svg"),
        text("", "İstanbul  Ölpumpe\tÜBERSICHT", "", ""),
    );
    assert_eq!(search(&c, "straße"), vec![1]);
    assert_eq!(search(&c, "ünïcode"), vec![1]);
    assert_eq!(search(&c, "ölpumpe übersicht"), vec![1]);
    let m = c.explain(1, "ölpumpe").unwrap();
    assert_eq!(m.field, MatchField::Text);
    assert_eq!(m.snippet, "İstanbul Ölpumpe ÜBERSICHT");
}

#[test]
fn catalog_is_shareable_across_threads() {
    fn assert_send_sync<T: Send + Sync>() {}
    fn assert_send<T: Send>() {}
    assert_send_sync::<Catalog>();
    assert_send::<svg_core::index::Database>();
}

#[test]
fn deterministic_ties_and_limit() {
    let mut c = Catalog::new();
    for i in 0..200u32 {
        c.upsert(
            rec(i + 1, &format!("d{}/icon-{:03}.svg", i % 7, (i * 37) % 200)),
            None,
        );
    }
    let full = search(&c, "icon");
    assert_eq!(full.len(), 200);
    assert_eq!(full, search(&c, "icon"));
    let top = c.search("icon", Some(25)).unwrap();
    assert_eq!(top, full[..25]);
    // Same depth and length → pure path order.
    let paths: Vec<String> = full
        .iter()
        .map(|&i| c.get(i).unwrap().relative_path.to_lowercase())
        .collect();
    let mut sorted_paths = paths.clone();
    sorted_paths.sort();
    assert_eq!(paths, sorted_paths);
}
