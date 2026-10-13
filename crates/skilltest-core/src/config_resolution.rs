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

fn parse_value(
    path: &str,
    raw: &str,
    environment: bool,
    known: &BTreeMap<String, Vec<Value>>,
) -> Result<Value> {
    let nodes = known
        .get(path)
        .ok_or_else(|| Error::Invalid(format!("unknown config setting `{path}`")))?;
    if let Ok(value) = serde_json::from_str(raw) {
        return Ok(value);
    }
    let string = nodes.iter().any(|node| {
        node.get("type") == Some(&json!("string"))
            || node
                .get("type")
                .and_then(Value::as_array)
                .is_some_and(|types| types.contains(&json!("string")))
    });
    if environment && string && !raw.starts_with('"') {
        return Ok(Value::String(raw.into()));
    }
    Err(Error::Invalid(format!(
        "config setting `{path}` needs a valid JSON value; got `{raw}`"
    )))
}

pub fn resolve(
    path: Option<&Path>,
    assignments: &[String],
    overrides: Overrides,
) -> Result<Config> {
    let mut value = serde_json::to_value(Config::default()).expect("Config serializes");
    let mut provenance = BTreeMap::new();
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
        serde_saphyr::from_str_with_options::<Config>(&text, {
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
        record(&file, "", 1, &mut provenance);
        merge(&mut value, file);
    }
    let root = schema();
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
    for (name, raw) in std::env::vars() {
        if matches!(
            name.as_str(),
            "SKILLTEST_BIN"
                | "SKILLTEST_PROVIDER"
                | "SKILLTEST_ONEHARNESS_BIN"
                | "SKILLTEST_HISTORY_DIR"
        ) {
            continue;
        }
        if let Some(path) = env_names.get(&name) {
            settings.insert((*path).clone(), parse_value(path, &raw, true, &known)?);
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
        let setting = parse_value(path, raw, false, &known)?;
        let layer = patch(path, setting);
        record(&layer, "", index as u64 + 3, &mut provenance);
        merge(&mut value, layer);
    }
    let mut config: Config = serde_json::from_value(value)
        .map_err(|e| Error::Invalid(format!("resolved config setting: {e}")))?;
    config.provenance = provenance;
    config.apply_overrides(overrides)?;
    Ok(config)
}

pub fn resolve_evaluators(config: &mut Config) {
    let model = config.judge_model.clone();
    let rank = |path: &str| config.provenance.get(path).copied().unwrap_or(0);
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
pub fn validate_role_args(role: &str, args: &[String]) -> Result<()> {
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
        );
        if incompatible {
            return Err(Error::Invalid(format!("provider.{role}.args setting `{flag}` is incompatible with a single buffered skilltest verdict")));
        }
        if flag == "--run-mode" {
            let mode = inline.or_else(|| tokens.next().map(String::as_str));
            if mode != Some("fallback") {
                return Err(Error::Invalid(format!("provider.{role}.args setting `--run-mode` must be fallback for a single verdict")));
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
