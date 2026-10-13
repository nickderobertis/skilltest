//! Compatibility corpus captured through the real loaders before the parser migration.
use super::*;
use serde_json::{json, Value};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

#[test]
fn committed_yaml_keeps_its_meaning() {
    let root = root();
    let paths: Vec<String> =
        serde_json::from_str(include_str!("../tests/yaml-corpus/paths.json")).unwrap();
    let mut readings = serde_json::Map::new();
    for relative in paths {
        let path = root.join(&relative);
        let reading = if relative.ends_with("SKILL.md") {
            let skill = skilltest_core::load_skill(path.parent().unwrap()).unwrap();
            json!({"name": skill.name, "description": skill.description, "instructions": skill.instructions})
        } else if relative.ends_with(".json") {
            serde_json::to_value(load_mocks_file(&path).unwrap()).unwrap()
        } else if relative == "examples/skilltest.yaml" || relative.contains("/config_") {
            serde_json::to_value(Config::load(&path).unwrap()).unwrap()
        } else {
            serde_json::to_value(TestCase::load(&path).unwrap()).unwrap()
        };
        // Loaded cases resolve skill paths relative to their source file.
        let normalized = serde_json::to_string(&reading)
            .unwrap()
            .replace(root.to_str().unwrap(), "<repo>");
        readings.insert(
            relative,
            serde_json::from_str::<Value>(&normalized).unwrap(),
        );
    }
    let actual = Value::Object(readings);
    let expected: Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("crates/skilltest-cli/tests/yaml-corpus/expected.json"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(actual, expected);
}

fn mocks_document(text: &str) -> Result<Vec<skilltest_core::MockDecl>> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "skilltest-json-mocks-{}-{}.json",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, text).unwrap();
    let result = load_mocks_file(&path);
    std::fs::remove_file(path).unwrap();
    result
}

#[test]
fn json_mocks_match_the_sdk_json_reader() {
    let documents = [
        r#"[{"name":"\u0061\ud83d\ude00","match":{"tool":"yes","contains":"on","input":{"null":"0x10","1_000":{"equals":"2001-12-14"}}},"stub":{"output":"\/\"\\\b\f\n\r\t","exit_code":-2}}]"#,
        r#"[{"match":{"tool":"read","input":{}},"rewrite":{"empty_list":[],"empty_map":{},"nested":[{"a":[null,true,false,1,1.0,1e3,2.5E-3]}],"text":"yes"}}]"#,
        r#"[{"match":{"tool":"bash"},"stub":[]},{"match":{"tool":"read"},"deny":{"message":"null"}}]"#,
        r#"[]"#,
    ];
    for text in documents {
        let expected: Vec<skilltest_core::MockDecl> = serde_json::from_str(text).unwrap();
        assert_eq!(mocks_document(text).unwrap(), expected, "{text}");
    }
    // Hand-written -0 is an integer in YAML; JSON retains a floating negative zero.
    let text = r#"[{"match":{"tool":"read"},"rewrite":{"zero":-0}}]"#;
    let actual = mocks_document(text).unwrap();
    assert_eq!(actual[0].rewrite.as_ref().unwrap()["zero"], json!(0));
    assert_eq!(
        serde_json::from_str::<Vec<skilltest_core::MockDecl>>(text).unwrap()[0]
            .rewrite
            .as_ref()
            .unwrap()["zero"],
        json!(-0.0)
    );
}

#[test]
fn json_mocks_refuse_schema_errors() {
    for text in [
        r#"[{"match":{"tool":{}}}]"#,
        r#"[{"match":{"tool":"bash"},"stub":{"exit_code":2147483648}}]"#,
        r#"[{"match":{"tool":"bash"},"stubb":"typo"}]"#,
    ] {
        assert!(
            serde_json::from_str::<Vec<skilltest_core::MockDecl>>(text).is_err(),
            "{text}"
        );
        assert!(mocks_document(text).is_err(), "{text}");
    }
}
