//! ACL guard for player commands: each `commands::player::` handler in lib.rs generate_handler! must be
//! listed in build.rs AppManifest commands and allowed in capabilities/default.json. Mirrors acl_sync.rs.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

fn read(rel: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

fn player_handlers(lib: &str) -> BTreeSet<String> {
    let start = lib
        .find("generate_handler![")
        .expect("generate_handler![ not found in lib.rs");
    let body = &lib[start..];
    let body = &body[..body.find("])").expect("end of generate_handler! not found")];
    body.split("commands::player::")
        .skip(1)
        .map(|rest| {
            rest.chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect::<String>()
        })
        .collect()
}

fn app_manifest_commands(build: &str) -> String {
    let after = build
        .split("commands(&[")
        .nth(1)
        .expect("commands(&[ not found in build.rs");
    after.split(']').next().unwrap().to_string()
}

#[test]
fn every_player_handler_is_in_app_manifest_and_allowed() {
    let lib = read("src/lib.rs");
    let build = read("build.rs");
    let caps: serde_json::Value = serde_json::from_str(&read("capabilities/default.json"))
        .expect("default.json is not valid JSON");
    let perms: Vec<&str> = caps["permissions"]
        .as_array()
        .expect("permissions array missing in default.json")
        .iter()
        .filter_map(|p| p.as_str())
        .collect();
    let manifest = app_manifest_commands(&build);

    let handlers = player_handlers(&lib);
    assert_eq!(
        handlers.len(),
        19,
        "expected 19 player handlers in generate_handler!, found {handlers:?}"
    );

    let mut missing = Vec::new();
    for name in &handlers {
        let quoted = format!("\"{name}\"");
        let allow = format!("allow-{}", name.replace('_', "-"));
        if !manifest.contains(&quoted) {
            missing.push(format!("build.rs AppManifest commands lacks {quoted}"));
        }
        if !perms.contains(&allow.as_str()) {
            missing.push(format!(
                "capabilities/default.json lacks permission {allow}"
            ));
        }
    }
    assert!(
        missing.is_empty(),
        "ACL gap for player commands:\n{}",
        missing.join("\n")
    );
}
