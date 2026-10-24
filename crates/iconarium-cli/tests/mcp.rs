//! The MCP server as a client sees it: spawn `iconarium-cli mcp` over stdio and talk to
//! it with rmcp's client, on both supported protocol revisions.

use std::collections::HashSet;

use base64::Engine;
use iconarium_catalog::fixtures;
use rmcp::model::{
    CallToolRequestParams, ProtocolVersion, ReadResourceRequestParams, ResourceContents,
};
use rmcp::service::{serve_client_with_lifecycle, ClientLifecycleMode, RunningService};
use rmcp::transport::TokioChildProcess;
use rmcp::{RoleClient, ServiceExt};
use serde_json::{json, Value};

const BIN: &str = env!("CARGO_BIN_EXE_iconarium-cli");

fn command(index: &std::path::Path, extra: &[&str]) -> tokio::process::Command {
    let mut c = tokio::process::Command::new(BIN);
    c.arg("--index").arg(index).arg("mcp").args(extra);
    c.env("ICONARIUM_LOG", "error");
    c
}

async fn connect(index: &std::path::Path, extra: &[&str]) -> RunningService<RoleClient, ()> {
    ().serve(TokioChildProcess::new(command(index, extra)).unwrap())
        .await
        .expect("client connects (2025-11-25, initialize)")
}

fn args(v: Value) -> rmcp::model::JsonObject {
    v.as_object().unwrap().clone()
}

async fn call(
    c: &RunningService<RoleClient, ()>,
    name: &'static str,
    v: Value,
) -> rmcp::model::CallToolResult {
    c.call_tool(CallToolRequestParams::new(name).with_arguments(args(v)))
        .await
        .unwrap_or_else(|e| panic!("{name} failed at protocol level: {e}"))
}

fn text(r: &rmcp::model::CallToolResult) -> String {
    r.content
        .iter()
        .filter_map(|c| serde_json::to_value(c).ok())
        .filter(|v| v["type"] == "text")
        .map(|v| v["text"].as_str().unwrap_or_default().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test(flavor = "multi_thread")]
async fn read_only_server_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let fx = fixtures::build(dir.path());
    let c = connect(&fx.index, &[]).await;

    let info = c.peer_info().unwrap();
    assert_eq!(info.server_info.as_ref().unwrap().name, "iconarium");
    assert!(info
        .instructions
        .as_deref()
        .unwrap()
        .contains("preview_icons"));

    // Tools: read-only set only; schemas and annotations present.
    let tools = c.list_all_tools().await.unwrap();
    let names: HashSet<_> = tools.iter().map(|t| t.name.to_string()).collect();
    assert_eq!(
        names,
        [
            "search_icons",
            "preview_icons",
            "get_icon",
            "list_libraries",
            "list_tags",
            "list_folders"
        ]
        .into_iter()
        .map(String::from)
        .collect()
    );
    for t in &tools {
        assert_eq!(
            t.annotations.as_ref().and_then(|a| a.read_only_hint),
            Some(true),
            "{}",
            t.name
        );
        if t.name != "preview_icons" {
            assert!(
                t.output_schema.is_some(),
                "{} needs an output schema",
                t.name
            );
        }
    }

    // search_icons: structured + readable text, no JSON duplicated in the text.
    let r = call(&c, "search_icons", json!({"query": "router"})).await;
    assert_ne!(r.is_error, Some(true), "{}", text(&r));
    let sc = r.structured_content.clone().unwrap();
    assert_eq!(sc["total"], 1);
    let id = sc["hits"][0]["id"].as_u64().unwrap();
    assert_eq!(sc["hits"][0]["name"], "Cisco edge router");
    assert!(text(&r).contains("Cisco edge router"));
    assert!(!text(&r).trim_start().starts_with('{'));

    // Filters and coverage warning.
    let r = call(
        &c,
        "search_icons",
        json!({"tags": ["networking"], "limit": 1}),
    )
    .await;
    let sc = r.structured_content.unwrap();
    assert_eq!(
        (sc["total"].as_u64(), sc["returned"].as_u64()),
        (Some(2), Some(1))
    );
    assert!(sc["nextCursor"].is_string());
    let r = call(&c, "search_icons", json!({"query": "star"})).await;
    assert!(text(&r).contains("note:"));

    // preview_icons: legend text + decodable PNG.
    let broken = fx.ids["ui/broken.svg"];
    let r = call(&c, "preview_icons", json!({"ids": [id, broken]})).await;
    assert_ne!(r.is_error, Some(true));
    assert!(text(&r).contains("not rendered"));
    let img = r
        .content
        .iter()
        .filter_map(|c| serde_json::to_value(c).ok())
        .find(|v| v["type"] == "image")
        .expect("image block");
    assert_eq!(img["mimeType"], "image/png");
    let png = base64::engine::general_purpose::STANDARD
        .decode(img["data"].as_str().unwrap())
        .unwrap();
    assert_eq!(&png[1..4], b"PNG");

    // get_icon by id and by library+path, with a resource link.
    let r = call(&c, "get_icon", json!({"id": id, "include_svg": true})).await;
    let sc = r.structured_content.clone().unwrap();
    assert!(sc["svg"].as_str().unwrap().starts_with("<svg"));
    assert!(serde_json::to_string(&r.content)
        .unwrap()
        .contains(&format!("iconarium://asset/{id}.svg")));
    let r = call(
        &c,
        "get_icon",
        json!({"library": fx.main_root.to_string_lossy(), "path": "ui/home.svg"}),
    )
    .await;
    assert_eq!(r.structured_content.unwrap()["path"], "ui/home.svg");

    // Errors are tool errors, readable by the model.
    let r = call(&c, "get_icon", json!({"id": 999999})).await;
    assert_eq!(r.is_error, Some(true));
    assert!(text(&r).contains("no icon"));
    let r = call(&c, "search_icons", json!({"libraries": ["icons"]})).await;
    assert_eq!(r.is_error, Some(true));
    assert!(text(&r).contains("ambiguous"));
    let r = call(&c, "preview_icons", json!({"ids": []})).await;
    assert_eq!(r.is_error, Some(true));

    // Listings.
    let r = call(&c, "list_libraries", json!({})).await;
    assert_eq!(
        r.structured_content.unwrap()["libraries"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let r = call(&c, "list_tags", json!({"prefix": "net"})).await;
    assert!(r.structured_content.unwrap()["tags"]
        .as_array()
        .unwrap()
        .iter()
        .all(|t| t["tag"].as_str().unwrap().starts_with("net")));
    let r = call(
        &c,
        "list_folders",
        json!({"library": fx.main_id.to_string()}),
    )
    .await;
    assert_eq!(
        r.structured_content.unwrap()["folders"][0]["name"],
        "network"
    );

    // Write tools are hidden and refused.
    let e = c
        .call_tool(
            CallToolRequestParams::new("export_icons")
                .with_arguments(args(json!({"ids": [id], "dest": "/tmp"}))),
        )
        .await;
    assert!(e.is_err() || e.unwrap().is_error == Some(true));

    // Resources.
    let templates = c.list_all_resource_templates().await.unwrap();
    assert_eq!(templates.len(), 2);
    let svg = c
        .read_resource(ReadResourceRequestParams::new(format!(
            "iconarium://asset/{id}.svg"
        )))
        .await
        .unwrap();
    match &svg.contents[0] {
        ResourceContents::TextResourceContents { text, .. } => assert!(text.contains("<rect")),
        other => panic!("expected text, got {other:?}"),
    }
    let png = c
        .read_resource(ReadResourceRequestParams::new(format!(
            "iconarium://asset/{id}/png/64"
        )))
        .await
        .unwrap();
    assert!(matches!(
        png.contents[0],
        ResourceContents::BlobResourceContents { .. }
    ));
    assert!(c
        .read_resource(ReadResourceRequestParams::new(
            "iconarium://asset/999999.svg"
        ))
        .await
        .is_err());

    c.cancel().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn write_tools_when_enabled() {
    let dir = tempfile::tempdir().unwrap();
    let fx = fixtures::build(dir.path());
    let out = dir.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let c = connect(
        &fx.index,
        &["--allow-write", out.to_str().unwrap(), "--allow-tagging"],
    )
    .await;
    let names: HashSet<_> = c
        .list_all_tools()
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(names.contains("export_icons") && names.contains("tag_icons"));

    let id = fx.ids["network/router.svg"];
    let r = call(&c, "export_icons", json!({"ids": [id], "dest": out.join("icons").to_string_lossy(), "format": "png", "size": 48})).await;
    assert_ne!(r.is_error, Some(true), "{}", text(&r));
    let written = r.structured_content.unwrap()["written"][0]["path"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(written.ends_with("cisco-edge-router.png"));
    let r = call(
        &c,
        "export_icons",
        json!({"ids": [id], "dest": dir.path().join("nope").to_string_lossy()}),
    )
    .await;
    assert_eq!(r.is_error, Some(true));
    assert!(text(&r).contains("outside"));

    let r = call(
        &c,
        "tag_icons",
        json!({"ids": [fx.ids["c4/person.svg"]], "add": ["Stakeholder"]}),
    )
    .await;
    assert_ne!(r.is_error, Some(true), "{}", text(&r));
    let r = call(&c, "search_icons", json!({"tags": ["stakeholder"]})).await;
    assert_eq!(r.structured_content.unwrap()["total"], 1);
    c.cancel().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn stateless_2026_07_28_client() {
    let dir = tempfile::tempdir().unwrap();
    let fx = fixtures::build(dir.path());
    let transport = TokioChildProcess::new(command(&fx.index, &[])).unwrap();
    let c = serve_client_with_lifecycle(
        (),
        transport,
        ClientLifecycleMode::Discover {
            preferred_versions: vec![ProtocolVersion::V_2026_07_28],
        },
    )
    .await
    .expect("discover lifecycle");
    let r = call(&c, "search_icons", json!({"query": "home"})).await;
    assert_eq!(
        r.structured_content.unwrap()["hits"][0]["path"],
        "ui/home.svg"
    );
    c.cancel().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_index_is_a_readable_tool_error() {
    let dir = tempfile::tempdir().unwrap();
    let c = connect(&dir.path().join("none.sqlite3"), &[]).await;
    let r = call(&c, "search_icons", json!({"query": "x"})).await;
    assert_eq!(r.is_error, Some(true));
    assert!(text(&r).contains("open a library folder in the Iconarium app"));
    assert!(!dir.path().join("none.sqlite3").exists());
    c.cancel().await.unwrap();
}
