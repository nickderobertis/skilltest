//! Schema-derived adapters and the single layer/order-aware configuration merger.
use crate::config::{Config, EvaluatorOverrides, EvaluatorSettings, Overrides, ProviderConfig};
use crate::{Error, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

/// Draft-07 contract shared by the CLI, environment adapter and SDK generators.
pub fn schema() -> Value {
    let generator = schemars::generate::SchemaSettings::draft07().into_generator();
    serde_json::to_value(generator.into_root_schema_for::<Config>()).expect("Config schema is JSON")
}

fn dereference<'a>(node: &'a Value, root: &'a Value) -> &'a Value {
    node.get("$ref")
        .and_then(Value::as_str)
        .and_then(|reference| reference.strip_prefix('#'))
        .and_then(|pointer| root.pointer(pointer))
        .unwrap_or(node)
}

fn paths(node: &Value, root: &Value, prefix: &str, out: &mut BTreeMap<String, Vec<Value>>) {
    let node = dereference(node, root);
    if !prefix.is_empty() {
        out.entry(prefix.into()).or_default().push(node.clone());
    }
    if let Some(properties) = node.get("properties").and_then(Value::as_object) {
        for (name, property) in properties {
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}.{name}")
            };
            paths(property, root, &path, out);
        }
    }
    for branch in ["anyOf", "oneOf", "allOf"] {
        if let Some(variants) = node.get(branch).and_then(Value::as_array) {
            for variant in variants {
                paths(variant, root, prefix, out);
            }
        }
    }
}

fn record(value: &Value, prefix: &str, rank: u64, provenance: &mut BTreeMap<String, u64>) {
    if !prefix.is_empty() {
        provenance.insert(prefix.into(), rank);
    }
    if let Some(object) = value.as_object() {
        for (key, value) in object {
            record(
                value,
                &if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                },
                rank,
                provenance,
            );
        }
    }
}

fn merge(target: &mut Value, patch: Value) {
    if let (Some(target), Some(patch)) = (target.as_object_mut(), patch.as_object()) {
        if let Some(kind) = patch.get("kind") {
            if target.get("kind") != Some(kind) {
                target.clear();
            }
        }
        for (key, value) in patch {
            merge(target.entry(key).or_insert(Value::Null), value.clone());
        }
    } else {
        *target = patch;
    }
}

fn patch(path: &str, value: Value) -> Value {
    path.split('.')
        .rev()
        .fold(value, |value, key| json!({key:value}))
}

/// Validate a partial object layer against Config's schema. Arrays are complete
/// replacements, so their elements retain required fields and strict shapes.
fn valid_patch(value: &Value, node: &Value, root: &Value, partial: bool) -> bool {
    let node = dereference(node, root);
    for keyword in ["anyOf", "oneOf"] {
        if let Some(branches) = node.get(keyword).and_then(Value::as_array) {
            return branches
                .iter()
                .any(|branch| valid_patch(value, branch, root, partial));
        }
    }
    if let Some(branches) = node.get("allOf").and_then(Value::as_array) {
        if !branches
            .iter()
            .all(|branch| valid_patch(value, branch, root, partial))
        {
            return false;
        }
    }
    if node.get("const").is_some_and(|constant| constant != value) {
        return false;
    }
    if node
        .get("enum")
        .and_then(Value::as_array)
        .is_some_and(|choices| !choices.contains(value))
    {
        return false;
    }
    let matches_type = |kind: &str| match kind {
        "null" => value.is_null(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        _ => false,
    };
    let matches = match node.get("type") {
        Some(Value::String(kind)) => matches_type(kind),
        Some(Value::Array(kinds)) => kinds.iter().filter_map(Value::as_str).any(matches_type),
        _ => true,
    };
    if !matches {
        return false;
    }
    let fits_format = match node.get("format").and_then(Value::as_str) {
        Some("uint32") => value
            .as_u64()
            .is_some_and(|number| u32::try_from(number).is_ok()),
        Some("uint64") => value.as_u64().is_some(),
        Some("int32") => value
            .as_i64()
            .is_some_and(|number| i32::try_from(number).is_ok()),
        Some("int64") => value.as_i64().is_some(),
        _ => true,
    };
    if !fits_format {
        return false;
    }
    if let Some(number) = value.as_f64() {
        if node
            .get("minimum")
            .and_then(Value::as_f64)
            .is_some_and(|minimum| number < minimum)
            || node
                .get("maximum")
                .and_then(Value::as_f64)
                .is_some_and(|maximum| number > maximum)
        {
            return false;
        }
    }
    if let Some(array) = value.as_array() {
        if node
            .get("minItems")
            .and_then(Value::as_u64)
            .is_some_and(|minimum| array.len() < minimum as usize)
            || node
                .get("maxItems")
                .and_then(Value::as_u64)
                .is_some_and(|maximum| array.len() > maximum as usize)
        {
            return false;
        }
    }
    if let Some(object) = value.as_object() {
        let properties = node.get("properties").and_then(Value::as_object);
        if !partial
            && node
                .get("required")
                .and_then(Value::as_array)
                .is_some_and(|keys| {
                    keys.iter()
                        .filter_map(Value::as_str)
                        .any(|key| !object.contains_key(key))
                })
        {
            return false;
        }
        for (key, member) in object {
            if let Some(property) = properties.and_then(|properties| properties.get(key)) {
                if !valid_patch(member, property, root, partial) {
                    return false;
                }
            } else if node.get("additionalProperties") == Some(&json!(false)) {
                return false;
            } else if let Some(additional) = node
                .get("additionalProperties")
                .filter(|node| node.is_object())
            {
                if !valid_patch(member, additional, root, partial) {
                    return false;
                }
            }
        }
    }
    if let (Some(array), Some(items)) = (value.as_array(), node.get("items")) {
        if !array
            .iter()
            .all(|item| valid_patch(item, items, root, false))
        {
            return false;
        }
    }
    true
}

/// Rust-owned semantic hooks are declared on their schema fields, so recursive
/// patches cannot hide incompatible argv behind a later replacement.
fn patch_semantics(value: &Value, node: &Value, root: &Value, path: &str) -> Result<()> {
    let node = dereference(node, root);
    if node.get("x-skilltest-validate").and_then(Value::as_str) == Some("oneharness-args") {
        let args: Vec<String> = serde_json::from_value(value.clone())
            .map_err(|error| Error::Invalid(format!("config setting `{path}`: {error}")))?;
        validate_role_args(path, &args)?;
    }
    for keyword in ["anyOf", "oneOf", "allOf"] {
        if let Some(branches) = node.get(keyword).and_then(Value::as_array) {
            for branch in branches
                .iter()
                .filter(|branch| valid_patch(value, branch, root, true))
            {
                patch_semantics(value, branch, root, path)?;
            }
        }
    }
    if let (Some(object), Some(properties)) = (
        value.as_object(),
        node.get("properties").and_then(Value::as_object),
    ) {
        for (key, member) in object {
            if let Some(property) = properties.get(key) {
                patch_semantics(
                    member,
                    property,
                    root,
                    &if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{path}.{key}")
                    },
                )?;
            }
        }
    }
    Ok(())
}

fn parse_value(
    path: &str,
    raw: &str,
    environment: bool,
    known: &BTreeMap<String, Vec<Value>>,
    root: &Value,
) -> Result<Value> {
    let nodes = known
        .get(path)
        .ok_or_else(|| Error::Invalid(format!("unknown config setting `{path}`")))?;
    // llmlint: ignore-block[changed_behavior_has_e2e] every_schema_leaf_is_applied_by_file_assignments_and_environment feeds bare enum text through the env resolver subprocess; dropping the judge.vendor env mapping makes its env judge.vendor assertion fail.
    let string = nodes.iter().any(|node| {
        node.get("type") == Some(&json!("string"))
            || node
                .get("type")
                .and_then(Value::as_array)
                .is_some_and(|types| types.contains(&json!("string")))
    });
    let value = if environment && string && raw != "null" && !raw.starts_with('"') {
        Value::String(raw.into())
    } else {
        serde_json::from_str(raw).map_err(|_| {
            Error::Invalid(format!(
                "config setting `{path}` needs a valid JSON value; got `{raw}`"
            ))
        })?
    };
    // llmlint: ignore-end[changed_behavior_has_e2e]
    if !nodes
        .iter()
        .any(|node| valid_patch(&value, node, root, true))
    {
        return Err(Error::Invalid(format!(
            "config setting `{path}` has invalid fields, type or enum value: {value}"
        )));
    }
    for node in nodes
        .iter()
        .filter(|node| valid_patch(&value, node, root, true))
    {
        patch_semantics(&value, node, root, path)?;
    }
    Ok(value)
}

pub fn resolve(
    path: Option<&Path>,
    assignments: &[String],
    overrides: Overrides,
) -> Result<Config> {
    let mut value = serde_json::to_value(Config::default()).expect("Config serializes");
    let mut provenance = BTreeMap::new();
    let root = schema();
    if let Some(path) = path {
        let text = std::fs::read_to_string(path).map_err(|source| Error::Io {
            path: path.into(),
            source,
        })?;
        let file: Value = serde_saphyr::from_str_with_options(&text, {
            let mut options = serde_saphyr::Options::default();
            options.strict_booleans = true;
            options.merge_keys = serde_saphyr::MergeKeyPolicy::Error;
            options.with_snippet = false;
            options
        })
        .map_err(|source| Error::Yaml {
            path: path.into(),
            message: source.to_string(),
        })?;
        // Validate the file independently, so an unknown field cannot disappear
        // behind a later variant reset or be masked by an overriding layer.
        let typed = serde_saphyr::from_str_with_options::<Config>(&text, {
            let mut options = serde_saphyr::Options::default();
            options.strict_booleans = true;
            options.merge_keys = serde_saphyr::MergeKeyPolicy::Error;
            options.with_snippet = false;
            options
        });
        if let Err(source) = typed {
            let message = source.to_string();
            if !message.contains("missing field") || !valid_patch(&file, &root, &root, true) {
                return Err(Error::Yaml {
                    path: path.into(),
                    message,
                });
            }
        }
        if !valid_patch(&file, &root, &root, true) {
            return Err(Error::Yaml {
                path: path.into(),
                message: "config contains invalid fields or values; check skilltest schema config"
                    .into(),
            });
        }
        patch_semantics(&file, &root, &root, "")?;
        record(&file, "", 1, &mut provenance);
        merge(&mut value, file);
    }
    let mut known = BTreeMap::new();
    paths(&root, &root, "", &mut known);
    let env_names: BTreeMap<_, _> = known
        .keys()
        .map(|path| {
            (
                format!("SKILLTEST_{}", path.replace('.', "__").to_uppercase()),
                path,
            )
        })
        .collect();
    let mut environment = Value::Object(Default::default());
    // Build a whole layer first; process parent paths before children to make
    // nullable block clears and their leaves deterministic, regardless of env order.
    let mut settings = BTreeMap::new();
    for (name, raw) in std::env::vars_os() {
        let Some(name) = name.to_str() else {
            continue;
        };
        if matches!(
            name,
            "SKILLTEST_BIN"
                | "SKILLTEST_PROVIDER"
                | "SKILLTEST_ONEHARNESS_BIN"
                | "SKILLTEST_HISTORY_DIR"
        ) {
            continue;
        }
        if let Some(path) = env_names.get(name) {
            let raw = raw.into_string().map_err(|_| {
                Error::Invalid(format!(
                    "config environment setting `{name}` must be Unicode"
                ))
            })?;
            settings.insert(
                (*path).clone(),
                parse_value(path, &raw, true, &known, &root)?,
            );
        }
    }
    for (path, setting) in settings {
        let mut cursor = &mut environment;
        let parts: Vec<_> = path.split('.').collect();
        for part in &parts[..parts.len() - 1] {
            if !cursor.is_object() {
                *cursor = json!({});
            }
            cursor = cursor
                .as_object_mut()
                .expect("object")
                .entry(*part)
                .or_insert(json!({}));
        }
        if !cursor.is_object() {
            *cursor = json!({});
        }
        cursor
            .as_object_mut()
            .expect("object")
            .insert(parts[parts.len() - 1].into(), setting);
    }
    record(&environment, "", 2, &mut provenance);
    merge(&mut value, environment);
    for (index, assignment) in assignments.iter().enumerate() {
        let (path, raw) = assignment.split_once('=').ok_or_else(|| {
            Error::Invalid(format!(
                "config assignment `{assignment}` needs dotted.path=<JSON-value>"
            ))
        })?;
        let setting = parse_value(path, raw, false, &known, &root)?;
        let layer = patch(path, setting);
        record(&layer, "", index as u64 + 3, &mut provenance);
        merge(&mut value, layer);
    }
    let mut config: Config = serde_json::from_value(value)
        .map_err(|e| Error::Invalid(format!("resolved config setting: {e}")))?;
    config.provenance = crate::config::ConfigProvenance {
        sources: provenance,
    };
    config.apply_overrides(overrides)?;
    Ok(config)
}

pub fn resolve_evaluators(config: &mut Config) {
    // Only a higher-layer explicit judge_model source qualifies below. If it
    // explicitly clears the model, preserve the legacy effective-model fallback.
    let model = config.effective_judge_model().to_string();
    let rank = |path: &str| config.provenance.sources.get(path).copied().unwrap_or(0);
    if let ProviderConfig::Oneharness(oh) = &mut config.provider {
        let settings = |role_rank| EvaluatorSettings {
            harness: (rank("provider.judge_harness") > role_rank).then(|| oh.judge_harness.clone()),
            model: (rank("judge_model") > role_rank).then(|| model.clone()),
            timeout: (rank("provider.timeout_secs") > role_rank).then_some(oh.timeout_secs),
        };
        let judge_rank = rank("provider.judge");
        let user_rank = if oh.simulated_user.is_some() {
            rank("provider.simulated_user")
        } else {
            judge_rank
        };
        *oh.evaluator_overrides = EvaluatorOverrides {
            judge: settings(judge_rank),
            simulated_user: settings(user_rank),
        };
    }
}

/// Framing belongs to skilltest; compatible oneharness settings remain opaque.
pub fn validate_role_args(setting: &str, args: &[String]) -> Result<()> {
    let mut tokens = args.iter();
    while let Some(token) = tokens.next() {
        if token == "--" {
            break;
        }
        let (flag, inline) = token
            .split_once('=')
            .map_or((token.as_str(), None), |(key, value)| (key, Some(value)));
        let incompatible = matches!(
            flag,
            "--all"
                | "--prompt"
                | "--prompt-file"
                | "--system"
                | "--system-file"
                | "--stream"
                | "--format"
                | "--compact"
                | "--schema"
                | "--batch-strategy"
                | "--batch-concurrency"
                | "--print-command"
        );
        if incompatible {
            return Err(Error::Invalid(format!("config setting `{setting}`: `{flag}` is incompatible with a single buffered skilltest verdict")));
        }
        if flag == "--run-mode" {
            let mode = inline.or_else(|| tokens.next().map(String::as_str));
            if mode != Some("fallback") {
                return Err(Error::Invalid(format!(
                    "config setting `{setting}`: `--run-mode` must be fallback for a single verdict"
                )));
            }
        } else if token.starts_with("--")
            && inline.is_none()
            && tokens
                .clone()
                .next()
                .is_some_and(|next| !next.starts_with("--"))
        {
            // Values are opaque: do not reinterpret a model/env/path value as a framing option.
            tokens.next();
        }
    }
    Ok(())
}

#[cfg(test)]
mod surface_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Leaf {
        path: String,
        value: Value,
        context: Value,
    }
    fn specimen(node: &Value, root: &Value) -> Value {
        let node = dereference(node, root);
        for branch in ["oneOf", "anyOf", "allOf"] {
            if let Some(variants) = node.get(branch).and_then(Value::as_array) {
                return specimen(
                    variants
                        .iter()
                        .find(|node| node.get("type") != Some(&json!("null")))
                        .unwrap(),
                    root,
                );
            }
        }
        if let Some(value) = node.get("const") {
            return value.clone();
        }
        if let Some(value) = node.get("default").filter(|value| !value.is_null()) {
            return value.clone();
        }
        let kind = node.get("type").and_then(|kind| {
            kind.as_str().or_else(|| {
                kind.as_array()?
                    .iter()
                    .filter_map(Value::as_str)
                    .find(|kind| *kind != "null")
            })
        });
        match kind {
            Some("object") => Value::Object(
                node["properties"]
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(key, node)| (key.clone(), specimen(node, root)))
                    .collect(),
            ),
            Some("array") => json!([]),
            Some("boolean") => json!(false),
            Some("integer" | "number") => json!(2),
            _ => json!("surface value"),
        }
    }
    fn leaves(node: &Value, root: &Value, prefix: &str, context: &Value, output: &mut Vec<Leaf>) {
        let node = dereference(node, root);
        for branch in ["oneOf", "anyOf", "allOf"] {
            if let Some(variants) = node.get(branch).and_then(Value::as_array) {
                for variant in variants
                    .iter()
                    .filter(|node| node.get("type") != Some(&json!("null")))
                {
                    leaves(variant, root, prefix, context, output);
                }
                return;
            }
        }
        if let Some(properties) = node.get("properties").and_then(Value::as_object) {
            let mut context = context.clone();
            if !prefix.is_empty() {
                merge(&mut context, patch(prefix, specimen(node, root)));
            }
            for (name, child) in properties {
                leaves(
                    child,
                    root,
                    &if prefix.is_empty() {
                        name.clone()
                    } else {
                        format!("{prefix}.{name}")
                    },
                    &context,
                    output,
                );
            }
        } else {
            let mut value = specimen(node, root);
            if prefix == "provider.command" {
                value = json!(["fixture-provider"]);
            }
            output.push(Leaf {
                path: prefix.into(),
                value,
                context: context.clone(),
            });
        }
    }

    #[test]
    fn resolver_child() {
        let Ok(input) = std::env::var("CONFIG_SURFACE_CHILD") else {
            return;
        };
        let input: Value = serde_json::from_str(&input).unwrap();
        let assignments: Vec<String> =
            serde_json::from_value(input["assignments"].clone()).unwrap();
        let config = Config::resolve(
            Some(Path::new(input["file"].as_str().unwrap())),
            &assignments,
            Overrides::default(),
        )
        .unwrap();
        std::fs::write(
            input["output"].as_str().unwrap(),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn every_schema_leaf_is_applied_by_file_assignments_and_environment() {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "skilltest-config-surfaces-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        let root = schema();
        let mut cases = Vec::new();
        leaves(&root, &root, "", &json!({}), &mut cases);
        // Prove ordinary enum leaves before discriminants that need companion leaves.
        cases.sort_by_key(|leaf| leaf.path.ends_with(".kind"));
        for leaf in cases {
            let mut context = leaf.context;
            if context.pointer("/provider/kind") == Some(&json!("command")) {
                merge(
                    &mut context,
                    json!({"provider":{"command":["fixture-provider"]}}),
                );
            }
            let pointer = format!("/{}", leaf.path.replace('.', "/"));
            merge(&mut context, patch(&leaf.path, leaf.value.clone()));
            let file = dir.join("config.json");
            std::fs::write(&file, context.to_string()).unwrap();
            let actual = serde_json::to_value(Config::load(&file).unwrap()).unwrap();
            assert_eq!(
                actual.pointer(&pointer),
                Some(&leaf.value),
                "YAML {}",
                leaf.path
            );
            let mut known = BTreeMap::new();
            paths(&root, &root, "", &mut known);
            let mut support = BTreeMap::new();
            if leaf.path.ends_with(".kind") {
                let parent = leaf.path.strip_suffix(".kind").unwrap();
                let target = context
                    .pointer(&format!("/{}", parent.replace('.', "/")))
                    .unwrap()
                    .clone();
                let alternative = known[parent]
                    .iter()
                    .map(|node| specimen(node, &root))
                    .find(|value| value.get("kind").is_some_and(|kind| kind != &leaf.value));
                if let Some(mut alternative) = alternative {
                    if alternative.get("kind") == Some(&json!("command")) {
                        alternative["command"] = json!(["fixture-provider"]);
                    }
                    merge(&mut context, patch(parent, alternative));
                } else {
                    // A single-valued discriminant has no second enum value.
                    // Exercise creation from its nullable parent instead, explicitly.
                    eprintln!(
                        "single-valued schema leaf {}: testing from null parent",
                        leaf.path
                    );
                    assert!(known[parent].iter().any(|node| valid_patch(&Value::Null, node, &root, true)),
                        "{} has no alternate value or nullable parent; add explicit sensitivity proof", leaf.path);
                    merge(&mut context, patch(parent, Value::Null));
                }
                for (name, value) in target.as_object().unwrap() {
                    if name != "kind" {
                        support.insert(format!("{parent}.{name}"), value.clone());
                    }
                }
            } else {
                let candidates = known[&leaf.path]
                    .iter()
                    .map(|node| specimen(node, &root))
                    .chain([
                        json!(false),
                        json!(true),
                        json!(3),
                        json!("lower value"),
                        json!([]),
                        json!(["lower"]),
                        json!(["--model", "lower"]),
                        json!([{"name":"lower","match":{"tool":"lower"}}]),
                        Value::Null,
                    ]);
                let lower = candidates.into_iter().find(|candidate| candidate != &leaf.value
                    && known[&leaf.path].iter().any(|node| valid_patch(candidate, node, &root, true))
                    && {
                        let mut full = serde_json::to_value(Config::default()).unwrap();
                        merge(&mut full, context.clone());
                        merge(&mut full, patch(&leaf.path, candidate.clone()));
                        serde_json::from_value::<Config>(full).is_ok_and(|config| config.validate().is_ok())
                    })
                    .unwrap_or_else(|| panic!("schema leaf {} has no second valid value; add explicit sensitivity proof", leaf.path));
                merge(&mut context, patch(&leaf.path, lower));
            }
            assert_ne!(
                context.pointer(&pointer),
                Some(&leaf.value),
                "lower {}",
                leaf.path
            );
            std::fs::write(&file, context.to_string()).unwrap();
            Config::load(&file)
                .unwrap_or_else(|error| panic!("invalid lower {}: {error}", leaf.path));
            let mut assignments = vec![format!("{}={}", leaf.path, leaf.value)];
            assignments.extend(
                support
                    .iter()
                    .map(|(path, value)| format!("{path}={value}")),
            );
            let actual = serde_json::to_value(
                Config::resolve(Some(&file), &assignments, Overrides::default()).unwrap(),
            )
            .unwrap();
            assert_eq!(
                actual.pointer(&pointer),
                Some(&leaf.value),
                "--set {}",
                leaf.path
            );
            let output = dir.join("resolved.json");
            let mut child = std::process::Command::new(std::env::current_exe().unwrap());
            for (path, value) in &support {
                child.env(
                    format!("SKILLTEST_{}", path.replace('.', "__").to_uppercase()),
                    value.to_string(),
                );
            }
            let status = child
                .args([
                    "--exact",
                    "config_resolution::surface_tests::resolver_child",
                ])
                .env(
                    "CONFIG_SURFACE_CHILD",
                    json!({"file":file,"output":output,"assignments":[]}).to_string(),
                )
                .env(
                    format!("SKILLTEST_{}", leaf.path.replace('.', "__").to_uppercase()),
                    leaf.value
                        .as_str()
                        .map_or_else(|| leaf.value.to_string(), str::to_owned),
                )
                .output()
                .unwrap();
            assert!(
                status.status.success(),
                "env {}: {}",
                leaf.path,
                String::from_utf8_lossy(&status.stdout)
            );
            let actual: Value = serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
            assert_eq!(
                actual.pointer(&pointer),
                Some(&leaf.value),
                "env {}",
                leaf.path
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
