//! The command-line interface against a fixture index.

use std::process::{Command, Output};

use iconarium_catalog::fixtures;
use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_iconarium-cli");

fn run(index: &std::path::Path, args: &[&str]) -> Output {
    Command::new(BIN)
        .arg("--index")
        .arg(index)
        .args(args)
        .env_remove("ICONARIUM_INDEX")
        .output()
        .unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn json(o: &Output) -> Value {
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", stdout(o)))
}

#[test]
fn search_show_tags_folders() {
    let dir = tempfile::tempdir().unwrap();
    let fx = fixtures::build(dir.path());

    let o = run(&fx.index, &["search", "router"]);
    assert!(o.status.success());
    assert!(stdout(&o).contains("Cisco edge router"));

    let o = run(&fx.index, &["--json", "search", "--tag", "networking"]);
    assert_eq!(json(&o)["total"], 2);

    let o = run(&fx.index, &["search", "zzzz-nothing"]);
    assert_eq!(o.status.code(), Some(3), "no results has its own exit code");

    let id = fx.ids["ui/home.svg"].to_string();
    let o = run(&fx.index, &["show", &id]);
    assert!(stdout(&o).contains("house (keywords)"), "{}", stdout(&o));
    let o = run(
        &fx.index,
        &[
            "--json",
            "show",
            "--library",
            &fx.main_id.to_string(),
            "ui/home.svg",
            "--svg",
        ],
    );
    assert!(json(&o)["svg"].as_str().unwrap().contains("<path"));
    let o = run(&fx.index, &["show", "not-a-number"]);
    assert_eq!(o.status.code(), Some(1));

    let o = run(&fx.index, &["tags", "--prefix", "corp"]);
    assert!(stdout(&o).contains("corporate"));

    let o = run(
        &fx.index,
        &["folders", "--library", &fx.main_id.to_string(), "ui"],
    );
    assert!(stdout(&o).contains("home.svg"));

    let o = run(&fx.index, &["--json", "libraries"]);
    assert_eq!(json(&o).as_array().unwrap().len(), 2);
}

#[test]
fn preview_export_tag() {
    let dir = tempfile::tempdir().unwrap();
    let fx = fixtures::build(dir.path());
    let a = fx.ids["network/router.svg"].to_string();
    let b = fx.ids["c4/person.svg"].to_string();

    let sheet = dir.path().join("sheet.png");
    let o = run(
        &fx.index,
        &["preview", &a, &b, "-o", sheet.to_str().unwrap()],
    );
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(&std::fs::read(&sheet).unwrap()[1..4], b"PNG");

    let out = dir.path().join("exported");
    let o = run(
        &fx.index,
        &[
            "export",
            &a,
            &b,
            "--to",
            out.to_str().unwrap(),
            "--png",
            "--size",
            "32",
        ],
    );
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(out.join("router.png").is_file() && out.join("person.png").is_file());

    let o = run(&fx.index, &["tag", &b, "--add", "people"]);
    assert!(o.status.success());
    let o = run(&fx.index, &["--json", "search", "--tag", "people"]);
    assert_eq!(json(&o)["total"], 1);
}

#[test]
fn doctor_and_missing_index() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("none.sqlite3");
    let o = run(&missing, &["search", "x"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("open a library in the Iconarium app"));
    assert!(!missing.exists());

    let o = run(&missing, &["--json", "doctor"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(json(&o)["readable"], false);

    let fx = fixtures::build(dir.path());
    let o = run(&fx.index, &["doctor"]);
    assert!(o.status.success());
    assert!(stdout(&o).contains("2 libraries, 6 icons"));
}
