use std::fs;

fn manifest() -> toml::Value {
    let source = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/herdr-plugin.toml"))
        .expect("read plugin manifest");
    source.parse().expect("parse plugin manifest")
}

fn command(value: &toml::Value) -> Vec<&str> {
    value
        .get("command")
        .and_then(toml::Value::as_array)
        .expect("manifest hook command")
        .iter()
        .map(|part| part.as_str().expect("manifest command argument"))
        .collect()
}

#[test]
fn manifest_declares_runtime_hooks_without_a_picker_binding() {
    let manifest = manifest();

    assert_eq!(
        manifest.get("id").and_then(toml::Value::as_str),
        Some("user.herdr-sessionizer")
    );
    assert_eq!(
        manifest
            .get("min_herdr_version")
            .and_then(toml::Value::as_str),
        Some("0.9.3")
    );

    let platforms = manifest
        .get("platforms")
        .and_then(toml::Value::as_array)
        .expect("manifest platforms");
    assert!(
        platforms
            .iter()
            .any(|platform| platform.as_str() == Some("linux"))
    );
    assert!(
        platforms
            .iter()
            .any(|platform| platform.as_str() == Some("macos"))
    );

    let startup = manifest
        .get("startup")
        .and_then(toml::Value::as_array)
        .expect("manifest startup hooks");
    assert!(
        startup
            .iter()
            .any(|hook| command(hook) == ["./herdr-sessionizer", "track"])
    );

    let events = manifest
        .get("events")
        .and_then(toml::Value::as_array)
        .expect("manifest event hooks");
    assert!(events.iter().any(|hook| {
        hook.get("on").and_then(toml::Value::as_str) == Some("workspace.focused")
            && command(hook) == ["./herdr-sessionizer", "track"]
    }));
    assert!(events.iter().any(|hook| {
        hook.get("on").and_then(toml::Value::as_str) == Some("pane.exited")
            && command(hook) == ["./herdr-sessionizer", "after-pane-exit"]
    }));

    assert!(manifest.get("bindings").is_none());
}
