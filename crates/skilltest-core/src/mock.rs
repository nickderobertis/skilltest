//! Tool mocking and spying: the `mocks:` block of a test case (and the CLI's
//! `--mocks` file), compiled down to the oneharness mock ruleset, plus the
//! [`MockCall`] records that come back — one per tool call the harness's hook
//! observed, carrying the *original* pre-rewrite input and the verdict applied.
//!
//! Vocabulary (sinon's): a **spy** observes without intercepting (a declaration
//! with no action), a **stub** substitutes a canned shell result, **deny**
//! blocks with a model-visible message, and **rewrite** substitutes raw input
//! fields (the primitive under `stub`, and the way to mock file reads). First
//! matching action wins; a call no action-rule matches is allowed through and
//! still recorded.
//!
//! Two matching engines exist on purpose:
//!
//! * Action rules are matched *inside the harness's hook process* by
//!   `oneharness mock` — [`compile_rules`] renders that ruleset. skilltest
//!   mirrors the same semantics in [`decide`] so the bundled fake provider (and
//!   the gate) exercise the identical decision logic without a harness; the
//!   live per-harness e2e is the drift alarm between the two.
//! * Spy declarations and eval `where` clauses are matched *locally* over the
//!   returned records ([`decl_matches`] / [`where_matches`]).
//!
//! Everything here is validated loudly at load time — an invalid regex, an
//! empty needle, or a criterion-less matcher must abort the run, never degrade
//! to match-nothing (or match-everything).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{Error, Result};

/// The explicit form of a [`FieldPredicate`]; every given key must hold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FieldPredicateSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equals: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contains: Option<String>,
    /// Unanchored regex (linear-time; same engine oneharness uses).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
}

/// A predicate on one tool-input field. Written in YAML either as a bare
/// string (exact equality) or as a map with any of `equals` / `contains` /
/// `pattern` (all given forms must hold).
///
/// Newtype variants around `deny_unknown_fields` structs, so a typo'd key
/// inside a predicate is a loud parse error, never silently dropped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum FieldPredicate {
    /// Bare-string shorthand: the field must equal this exactly.
    Equals(String),
    /// The explicit form; every given key must hold.
    Spec(FieldPredicateSpec),
}

impl FieldPredicate {
    /// Validate the predicate for field `key` of declaration `who`.
    fn validate(&self, who: &str, key: &str) -> Result<()> {
        match self {
            FieldPredicate::Equals(_) => Ok(()),
            FieldPredicate::Spec(FieldPredicateSpec {
                equals,
                contains,
                pattern,
            }) => {
                if equals.is_none() && contains.is_none() && pattern.is_none() {
                    return Err(Error::Invalid(format!(
                        "{who}: `input.{key}` needs one of `equals`/`contains`/`pattern`"
                    )));
                }
                if contains.as_deref() == Some("") || pattern.as_deref() == Some("") {
                    return Err(Error::Invalid(format!(
                        "{who}: empty `contains`/`pattern` in `input.{key}` would match everything"
                    )));
                }
                if let Some(pattern) = pattern {
                    compile_pattern(pattern, &format!("{who}: `input.{key}.pattern`"))?;
                }
                Ok(())
            }
        }
    }

    /// Whether every given form holds for the field's string value.
    fn matches(&self, value: &str) -> bool {
        match self {
            FieldPredicate::Equals(want) => value == want,
            FieldPredicate::Spec(FieldPredicateSpec {
                equals,
                contains,
                pattern,
            }) => {
                if let Some(want) = equals {
                    if value != want {
                        return false;
                    }
                }
                if let Some(needle) = contains {
                    if needle.is_empty() || !value.contains(needle.as_str()) {
                        return false;
                    }
                }
                if let Some(pattern) = pattern {
                    // Validated at load; a compile failure here is a no-match,
                    // never an accidental match-everything.
                    match regex::Regex::new(pattern) {
                        Ok(re) if re.is_match(value) => {}
                        _ => return false,
                    }
                }
                true
            }
        }
    }

    /// The oneharness `StringPredicate` JSON this compiles to.
    fn to_oneharness(&self) -> Value {
        match self {
            FieldPredicate::Equals(want) => json!({ "equals": want }),
            FieldPredicate::Spec(FieldPredicateSpec {
                equals,
                contains,
                pattern,
            }) => {
                let mut spec = serde_json::Map::new();
                if let Some(v) = equals {
                    spec.insert("equals".into(), json!(v));
                }
                if let Some(v) = contains {
                    spec.insert("contains".into(), json!(v));
                }
                if let Some(v) = pattern {
                    spec.insert("regex".into(), json!(v));
                }
                Value::Object(spec)
            }
        }
    }
}

/// What a mock/spy declaration matches on. At least one criterion is required;
/// every given criterion must hold (AND).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MockMatch {
    /// Case-insensitive exact match on the tool name. Tool names are
    /// per-harness (`Bash` on claude-code, `bash` on opencode/crush), so
    /// cross-harness matchers usually prefer `contains`/`pattern`/`input`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// Substring match over the raw hook event JSON (the tool name and its
    /// input always serialize into it). Note the haystack is JSON: quotes and
    /// backslashes inside tool input appear escaped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contains: Option<String>,
    /// Unanchored regex over the same haystack as `contains`, for non-exact
    /// needles like `git push( --force)?`. Linear-time engine (no lookarounds).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    /// Per-field predicates on the tool's input: the key is the argument name
    /// (`command`, `file_path`, …), and every listed field must match (a field
    /// absent from the call fails the matcher). Non-string fields are compared
    /// against their compact JSON form.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub input: BTreeMap<String, FieldPredicate>,
}

impl MockMatch {
    /// Validate the matcher for declaration `who`. Loud on any fault.
    fn validate(&self, who: &str) -> Result<()> {
        for (field, value) in [
            ("tool", &self.tool),
            ("contains", &self.contains),
            ("pattern", &self.pattern),
        ] {
            if value.as_deref() == Some("") {
                return Err(Error::Invalid(format!(
                    "{who}: empty `match.{field}` would match everything"
                )));
            }
        }
        if let Some(pattern) = &self.pattern {
            compile_pattern(pattern, &format!("{who}: `match.pattern`"))?;
        }
        for (key, pred) in &self.input {
            pred.validate(who, key)?;
        }
        if self.tool.is_none()
            && self.contains.is_none()
            && self.pattern.is_none()
            && self.input.is_empty()
        {
            return Err(Error::Invalid(format!(
                "{who}: `match` needs at least one of `tool`, `contains`, `pattern`, or `input`"
            )));
        }
        Ok(())
    }

    /// Whether this matcher covers a call with `tool` name and `input` args.
    /// The `contains`/`pattern` haystack is the synthesized compact event JSON
    /// (`{"tool_name":…,"tool_input":…}`), mirroring what the hook-side rules
    /// match against.
    #[must_use]
    pub fn matches_call(&self, tool: Option<&str>, input: Option<&Value>) -> bool {
        if let Some(want) = &self.tool {
            match tool {
                Some(name) if name.eq_ignore_ascii_case(want) => {}
                _ => return false,
            }
        }
        if self.contains.is_some() || self.pattern.is_some() {
            let event = event_haystack(tool, input);
            if let Some(needle) = &self.contains {
                if needle.is_empty() || !event.contains(needle.as_str()) {
                    return false;
                }
            }
            if let Some(pattern) = &self.pattern {
                match regex::Regex::new(pattern) {
                    Ok(re) if re.is_match(&event) => {}
                    _ => return false,
                }
            }
        }
        where_matches(&self.input, input)
    }

    /// The oneharness `MatchSpec` JSON this compiles to.
    fn to_oneharness(&self) -> Value {
        let mut spec = serde_json::Map::new();
        if let Some(tool) = &self.tool {
            spec.insert("tool".into(), json!(tool));
        }
        if let Some(contains) = &self.contains {
            spec.insert("event_contains".into(), json!(contains));
        }
        if let Some(pattern) = &self.pattern {
            spec.insert("event_regex".into(), json!(pattern));
        }
        if !self.input.is_empty() {
            let input: serde_json::Map<String, Value> = self
                .input
                .iter()
                .map(|(k, p)| (k.clone(), p.to_oneharness()))
                .collect();
            spec.insert("input".into(), Value::Object(input));
        }
        Value::Object(spec)
    }
}

/// The explicit form of a [`StubSpec`]: the canned output plus an exit code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StubOutput {
    pub output: String,
    /// Non-zero fakes a failing command. Default 0.
    #[serde(default)]
    pub exit_code: i32,
}

/// A `stub` action: fake a shell call's result by declaring only the output.
/// Written as a bare string (the output) or a map with `output` + `exit_code`.
/// A typo'd key inside the map form is a loud parse error (deny on the inner
/// struct), never a silently-applied default exit code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum StubSpec {
    /// Bare-string shorthand: the canned stdout, exit code 0.
    Text(String),
    Full(StubOutput),
}

impl StubSpec {
    /// The canned output text.
    #[must_use]
    pub fn output(&self) -> &str {
        match self {
            StubSpec::Text(output) | StubSpec::Full(StubOutput { output, .. }) => output,
        }
    }

    /// The stub's exit code (0 unless faked as failing).
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        match self {
            StubSpec::Text(_) => 0,
            StubSpec::Full(StubOutput { exit_code, .. }) => *exit_code,
        }
    }
}

/// The explicit form of a [`DenySpec`]: the model-visible message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DenyMessage {
    pub message: String,
}

/// A `deny` action: block the call with a model-visible message. Written as a
/// bare string (the message) or a map with `message`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum DenySpec {
    Text(String),
    Full(DenyMessage),
}

impl DenySpec {
    /// The message the model reads as the tool's feedback.
    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            DenySpec::Text(message) | DenySpec::Full(DenyMessage { message }) => message,
        }
    }
}

/// One mock or spy declaration — an entry of a test case's `mocks:` block, of
/// the CLI's `--mocks` file, or synthesized by an SDK. Exactly one of `stub` /
/// `deny` / `rewrite` makes it a **mock** (the call is intercepted); none makes
/// it a **spy** (observed only, matched locally against the returned records).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MockDecl {
    /// Name evals (`type: called` / `not_called`) reference this declaration
    /// by. Optional; unnamed declarations get a positional fallback
    /// (`mock_<i>` / `spy_<i>`) used in reports and error messages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "match")]
    pub matcher: MockMatch,
    /// Fake a shell call's result: the real command never runs and the model
    /// receives this output as the tool's genuine result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stub: Option<StubSpec>,
    /// Block the call; the model reads the message as the tool's feedback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deny: Option<DenySpec>,
    /// Substitute raw input fields (a JSON object) — the low-level escape
    /// hatch, and the way to mock file reads (rewrite `file_path` to a
    /// fixture).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rewrite: Option<Value>,
}

impl MockDecl {
    /// Whether this declaration intercepts (has an action) or only observes.
    #[must_use]
    pub fn is_mock(&self) -> bool {
        self.stub.is_some() || self.deny.is_some() || self.rewrite.is_some()
    }

    /// Validate the declaration, identified as `who` in errors.
    ///
    /// # Errors
    /// [`Error::Invalid`] on an empty name, an invalid matcher, more than one
    /// action, or a non-object `rewrite`.
    pub fn validate(&self, who: &str) -> Result<()> {
        if self.name.as_deref() == Some("") {
            return Err(Error::Invalid(format!("{who}: `name` must not be empty")));
        }
        self.matcher.validate(who)?;
        let actions = usize::from(self.stub.is_some())
            + usize::from(self.deny.is_some())
            + usize::from(self.rewrite.is_some());
        if actions > 1 {
            return Err(Error::Invalid(format!(
                "{who}: declare at most one of `stub`/`deny`/`rewrite` (omit all three for a spy)"
            )));
        }
        if let Some(rewrite) = &self.rewrite {
            if !rewrite.is_object() {
                return Err(Error::Invalid(format!(
                    "{who}: `rewrite` must be a JSON object (the substituted tool arguments)"
                )));
            }
        }
        if let Some(StubSpec::Text(text)) = &self.stub {
            if text.is_empty() {
                return Err(Error::Invalid(format!(
                    "{who}: `stub` output must not be empty (use `deny` to block a call)"
                )));
            }
        }
        Ok(())
    }

    /// The oneharness rule action this compiles to; `None` for a spy.
    fn action_json(&self) -> Option<Value> {
        if let Some(stub) = &self.stub {
            return Some(json!({
                "stub": { "output": stub.output(), "exit_code": stub.exit_code() }
            }));
        }
        if let Some(deny) = &self.deny {
            return Some(json!({ "deny": { "message": deny.message() } }));
        }
        self.rewrite
            .as_ref()
            .map(|input| json!({ "rewrite": { "input": input } }))
    }

    /// The action's stable token (`stub`/`deny`/`rewrite`), or `None` for a spy.
    #[must_use]
    pub fn action_kind(&self) -> Option<&'static str> {
        if self.stub.is_some() {
            Some("stub")
        } else if self.deny.is_some() {
            Some("deny")
        } else if self.rewrite.is_some() {
            Some("rewrite")
        } else {
            None
        }
    }
}

/// One observed tool call, as recorded by the mock/spy channel: the harness
/// hook's spy log for real runs, or the provider's `mock_calls` response for
/// the command protocol. Carries the **original, pre-rewrite** input — the
/// transcript's `events` show post-rewrite reality (the stub that actually
/// ran); this shows what the skill *attempted*.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MockCall {
    /// Tool name as the harness reported it; `null` when the event named none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// The tool's original input arguments; `null` when the event carried none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    /// The verdict applied: `allow` (fell through every rule), `deny`,
    /// `rewrite`, or `stub`.
    pub action: String,
    /// Index of the compiled rule that intercepted; `null` for `allow`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<usize>,
    /// Name of the mock declaration the intercepting rule came from; `null`
    /// for `allow`. Filled by the runner, so SDKs bind records to mock objects
    /// by name instead of re-deriving rule indices.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mock: Option<String>,
}

impl MockCall {
    /// True iff a mock's verdict was applied to this call.
    #[must_use]
    pub fn mocked(&self) -> bool {
        self.action != "allow"
    }
}

/// The effective mock/spy set for one case run: the CLI/SDK-level declarations
/// (first, so a test-local rule shadows a shared one — first match wins)
/// followed by the case's own, validated as a whole and compiled to the
/// oneharness ruleset.
#[derive(Debug, Clone, Default)]
pub struct MockSet {
    decls: Vec<MockDecl>,
    /// The compiled oneharness ruleset (`{"rules": […]}`); `None` when no
    /// declaration carries an action.
    rules: Option<Value>,
    /// `rule_to_decl[rule_index]` = index into `decls`.
    rule_to_decl: Vec<usize>,
    /// The observation channel was requested even without any declarations
    /// (`spy: true` on the case, `--spy` on the CLI).
    spy_requested: bool,
}

impl MockSet {
    /// Build and validate the effective set. `shared` (CLI `--mocks` / SDK)
    /// declarations come first, then the case's own; names must be unique
    /// across the whole set.
    ///
    /// # Errors
    /// [`Error::Invalid`] on any invalid declaration or a duplicate name.
    pub fn build(shared: &[MockDecl], case: &[MockDecl], spy_requested: bool) -> Result<MockSet> {
        let decls: Vec<MockDecl> = shared.iter().chain(case.iter()).cloned().collect();
        let mut seen = std::collections::BTreeSet::new();
        for (i, decl) in decls.iter().enumerate() {
            decl.validate(&format!("mock `{}`", decl_label(decl, i)))?;
            if let Some(name) = &decl.name {
                if !seen.insert(name.clone()) {
                    return Err(Error::Invalid(format!(
                        "duplicate mock name `{name}` (names must be unique across --mocks and the case)"
                    )));
                }
            }
        }
        let mut rules = Vec::new();
        let mut rule_to_decl = Vec::new();
        for (i, decl) in decls.iter().enumerate() {
            if let Some(action) = decl.action_json() {
                rules.push(json!({ "match": decl.matcher.to_oneharness(), "action": action }));
                rule_to_decl.push(i);
            }
        }
        Ok(MockSet {
            decls,
            rules: (!rules.is_empty()).then(|| json!({ "rules": rules })),
            rule_to_decl,
            spy_requested,
        })
    }

    /// Whether the observation channel should be enabled for this run.
    #[must_use]
    pub fn active(&self) -> bool {
        self.spy_requested || !self.decls.is_empty()
    }

    /// The compiled oneharness ruleset, when any declaration intercepts.
    #[must_use]
    pub fn rules(&self) -> Option<&Value> {
        self.rules.as_ref()
    }

    /// The effective declarations (shared first, then the case's).
    #[must_use]
    pub fn decls(&self) -> &[MockDecl] {
        &self.decls
    }

    /// Fill each record's `mock` name from the rule index that intercepted it.
    #[must_use]
    pub fn resolve(&self, mut records: Vec<MockCall>) -> Vec<MockCall> {
        for record in &mut records {
            record.mock = record
                .rule
                .and_then(|rule| self.rule_to_decl.get(rule))
                .map(|&decl| decl_label(&self.decls[decl], decl));
        }
        records
    }

    /// The records belonging to the declaration named `name`: for a mock, the
    /// calls its rule intercepted; for a spy, every record its matcher covers
    /// (including calls other rules intercepted — a spy observes everything).
    ///
    /// # Errors
    /// [`Error::Invalid`] when no declaration has that name (listing the
    /// declared names, so a typo is caught loudly instead of matching nothing).
    pub fn records_for<'r>(
        &self,
        name: &str,
        records: &'r [MockCall],
    ) -> Result<Vec<&'r MockCall>> {
        let (index, decl) = self
            .decls
            .iter()
            .enumerate()
            .find(|(i, d)| decl_label(d, *i) == name)
            .ok_or_else(|| {
                let declared: Vec<String> = self
                    .decls
                    .iter()
                    .enumerate()
                    .map(|(i, d)| decl_label(d, i))
                    .collect();
                Error::Invalid(format!(
                    "eval references unknown mock `{name}` (declared: {})",
                    if declared.is_empty() {
                        "none".to_string()
                    } else {
                        declared.join(", ")
                    }
                ))
            })?;
        if decl.is_mock() {
            let label = decl_label(decl, index);
            Ok(records
                .iter()
                .filter(|r| r.mock.as_deref() == Some(label.as_str()))
                .collect())
        } else {
            Ok(records
                .iter()
                .filter(|r| {
                    decl.matcher
                        .matches_call(r.tool.as_deref(), r.input.as_ref())
                })
                .collect())
        }
    }
}

/// A declaration's effective name: its explicit `name`, else a positional
/// `mock_<i>` / `spy_<i>` fallback.
fn decl_label(decl: &MockDecl, index: usize) -> String {
    decl.name.clone().unwrap_or_else(|| {
        if decl.is_mock() {
            format!("mock_{index}")
        } else {
            format!("spy_{index}")
        }
    })
}

/// What the provider should do about mocking for one `respond` call: the
/// compiled ruleset to enforce (if any) and that the observation channel is on.
/// Absent entirely (the runner passes `None`) when the case declares nothing.
#[derive(Debug, Clone, Copy)]
pub struct MockPlan<'a> {
    /// The compiled oneharness ruleset; `None` for a spy-only run.
    pub rules: Option<&'a Value>,
}

// ---------------------------------------------------------------------------
// The reference decision engine (fake provider / gate parity)
// ---------------------------------------------------------------------------

/// A matched action, borrowed from the compiled ruleset.
#[derive(Debug, Clone, PartialEq)]
pub enum AppliedAction<'r> {
    Deny { message: &'r str },
    Rewrite { input: &'r Value },
    Stub { output: &'r str, exit_code: i32 },
}

impl AppliedAction<'_> {
    /// Stable token for records (`deny`/`rewrite`/`stub`).
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            AppliedAction::Deny { .. } => "deny",
            AppliedAction::Rewrite { .. } => "rewrite",
            AppliedAction::Stub { .. } => "stub",
        }
    }
}

/// Decide which rule of a compiled ruleset (the `{"rules": […]}` value built by
/// [`MockSet::rules`]) intercepts a call. First match wins; `None` means allow
/// through. This mirrors `oneharness mock`'s decision over the same shape so
/// the fake provider exercises identical semantics.
#[must_use]
pub fn decide<'r>(
    rules: &'r Value,
    tool: Option<&str>,
    input: Option<&Value>,
) -> Option<(usize, AppliedAction<'r>)> {
    let list = rules.get("rules")?.as_array()?;
    for (index, rule) in list.iter().enumerate() {
        if compiled_rule_matches(rule.get("match")?, tool, input) {
            return parse_action(rule.get("action")?).map(|action| (index, action));
        }
    }
    None
}

/// Whether a compiled oneharness `MatchSpec` covers the call.
fn compiled_rule_matches(spec: &Value, tool: Option<&str>, input: Option<&Value>) -> bool {
    if let Some(want) = spec.get("tool").and_then(Value::as_str) {
        match tool {
            Some(name) if name.eq_ignore_ascii_case(want) => {}
            _ => return false,
        }
    }
    let needs_event = spec.get("event_contains").is_some() || spec.get("event_regex").is_some();
    if needs_event {
        let event = event_haystack(tool, input);
        if let Some(needle) = spec.get("event_contains").and_then(Value::as_str) {
            if needle.is_empty() || !event.contains(needle) {
                return false;
            }
        }
        if let Some(pattern) = spec.get("event_regex").and_then(Value::as_str) {
            match regex::Regex::new(pattern) {
                Ok(re) if re.is_match(&event) => {}
                _ => return false,
            }
        }
    }
    if let Some(preds) = spec.get("input").and_then(Value::as_object) {
        for (key, pred) in preds {
            let Some(value) = input.and_then(|i| i.get(key)) else {
                return false;
            };
            let text = coerce_field(value);
            if let Some(want) = pred.get("equals").and_then(Value::as_str) {
                if text != want {
                    return false;
                }
            }
            if let Some(needle) = pred.get("contains").and_then(Value::as_str) {
                if needle.is_empty() || !text.contains(needle) {
                    return false;
                }
            }
            if let Some(pattern) = pred.get("regex").and_then(Value::as_str) {
                match regex::Regex::new(pattern) {
                    Ok(re) if re.is_match(&text) => {}
                    _ => return false,
                }
            }
        }
    }
    true
}

fn parse_action(action: &Value) -> Option<AppliedAction<'_>> {
    if let Some(deny) = action.get("deny") {
        return Some(AppliedAction::Deny {
            message: deny.get("message")?.as_str()?,
        });
    }
    if let Some(rewrite) = action.get("rewrite") {
        return Some(AppliedAction::Rewrite {
            input: rewrite.get("input")?,
        });
    }
    if let Some(stub) = action.get("stub") {
        return Some(AppliedAction::Stub {
            output: stub.get("output")?.as_str()?,
            exit_code: stub
                .get("exit_code")
                .and_then(Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .unwrap_or(0),
        });
    }
    None
}

/// The shell command a `stub` compiles to — a safely single-quoted `printf` of
/// the declared output, exactly mirroring oneharness's `stub_input` so the fake
/// provider's post-rewrite `events` look like the real thing.
#[must_use]
pub fn stub_command(output: &str, exit_code: i32) -> String {
    let quoted = format!("'{}'", output.replace('\'', "'\\''"));
    let mut command = format!("printf '%s\\n' {quoted}");
    if exit_code != 0 {
        command.push_str(&format!("; exit {exit_code}"));
    }
    command
}

/// Apply an eval's `where` clause (per-field input predicates) to a call's
/// input. Every listed field must exist and match; an empty clause matches.
#[must_use]
pub fn where_matches(clause: &BTreeMap<String, FieldPredicate>, input: Option<&Value>) -> bool {
    for (key, pred) in clause {
        let Some(value) = input.and_then(|i| i.get(key)) else {
            return false;
        };
        if !pred.matches(&coerce_field(value)) {
            return false;
        }
    }
    true
}

/// Whether a spy declaration's matcher covers a record (local matching — the
/// hook never sees spies).
#[must_use]
pub fn decl_matches(matcher: &MockMatch, call: &MockCall) -> bool {
    matcher.matches_call(call.tool.as_deref(), call.input.as_ref())
}

/// Validate an eval `where` clause, identified as `who` in errors.
///
/// # Errors
/// [`Error::Invalid`] on an invalid predicate (see [`FieldPredicate`]).
pub fn validate_where(clause: &BTreeMap<String, FieldPredicate>, who: &str) -> Result<()> {
    for (key, pred) in clause {
        pred.validate(who, key)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Spy-log parsing (the oneharness `--spy-file` JSONL)
// ---------------------------------------------------------------------------

/// One line of the oneharness spy log, as `oneharness mock` appends it.
#[derive(Deserialize)]
struct SpyLine {
    /// The raw hook event (parsed JSON, or a string for a non-JSON event).
    event: Value,
    action: String,
    #[serde(default)]
    rule: Option<usize>,
}

/// Parse an oneharness spy-log (JSONL) into records. Unparseable lines are an
/// error — a truncated log must not silently read as "fewer calls".
///
/// # Errors
/// [`Error::Provider`] when a line is not valid JSON.
pub fn parse_spy_log(text: &str) -> Result<Vec<MockCall>> {
    let mut records = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parsed: SpyLine = serde_json::from_str(line).map_err(|e| {
            Error::provider(
                "oneharness",
                format!("invalid spy-log line: {e}; got: {line}"),
            )
        })?;
        records.push(MockCall {
            tool: event_tool_name(&parsed.event),
            input: event_tool_input(&parsed.event).cloned(),
            action: parsed.action,
            rule: parsed.rule,
            mock: None,
        });
    }
    Ok(records)
}

/// Best-effort tool name from a hook event (`tool_name`, Copilot's `toolName`,
/// or `tool`) — the same keys `oneharness mock` extracts.
fn event_tool_name(event: &Value) -> Option<String> {
    for key in ["tool_name", "toolName", "tool"] {
        if let Some(name) = event.get(key).and_then(Value::as_str) {
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// The tool's input object from a hook event (`tool_input`, or Copilot's
/// `toolArgs`).
fn event_tool_input(event: &Value) -> Option<&Value> {
    for key in ["tool_input", "toolArgs"] {
        if let Some(input) = event.get(key) {
            if input.is_object() {
                return Some(input);
            }
        }
    }
    None
}

/// The synthesized event JSON `contains`/`pattern` match against — compact, in
/// the same field order every gated harness serializes (`tool_name` first).
fn event_haystack(tool: Option<&str>, input: Option<&Value>) -> String {
    let mut event = serde_json::Map::new();
    if let Some(tool) = tool {
        event.insert("tool_name".into(), json!(tool));
    }
    if let Some(input) = input {
        event.insert("tool_input".into(), input.clone());
    }
    Value::Object(event).to_string()
}

/// A non-string input field is matched against its compact JSON form (the same
/// coercion oneharness applies).
fn coerce_field(value: &Value) -> String {
    match value.as_str() {
        Some(s) => s.to_string(),
        None => value.to_string(),
    }
}

/// Compile a regex pattern, mapping the failure to a loud [`Error::Invalid`].
fn compile_pattern(pattern: &str, who: &str) -> Result<regex::Regex> {
    regex::Regex::new(pattern)
        .map_err(|e| Error::Invalid(format!("{who} is not a valid regex: {e}")))
}

/// A compact one-line description of observed calls for failure messages,
/// e.g. `bash(git status), read({"file_path":"x"}) [deny]` — capped so a chatty
/// run doesn't flood the report.
#[must_use]
pub fn describe_records(records: &[MockCall]) -> String {
    const CAP: usize = 5;
    let mut parts: Vec<String> = records
        .iter()
        .take(CAP)
        .map(|r| {
            let tool = r.tool.as_deref().unwrap_or("?");
            let input = r
                .input
                .as_ref()
                .map(|i| match i.get("command").and_then(Value::as_str) {
                    Some(command) => command.to_string(),
                    None => i.to_string(),
                })
                .unwrap_or_default();
            if r.mocked() {
                format!("{tool}({input}) [{}]", r.action)
            } else {
                format!("{tool}({input})")
            }
        })
        .collect();
    if records.len() > CAP {
        parts.push(format!("… {} more", records.len() - CAP));
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decl(yaml: &str) -> MockDecl {
        serde_yaml::from_str(yaml).expect("test decl must parse")
    }

    #[test]
    fn parses_stub_deny_rewrite_and_spy_shorthands() {
        let stub = decl("match: { contains: git push }\nstub: Everything up-to-date\n");
        assert_eq!(
            stub.stub.as_ref().unwrap().output(),
            "Everything up-to-date"
        );
        assert_eq!(stub.stub.as_ref().unwrap().exit_code(), 0);
        assert_eq!(stub.action_kind(), Some("stub"));

        let stub_full = decl("match: { tool: bash }\nstub: { output: boom, exit_code: 2 }\n");
        assert_eq!(stub_full.stub.as_ref().unwrap().exit_code(), 2);

        let deny = decl("match: { contains: rm -rf }\ndeny: blocked\n");
        assert_eq!(deny.deny.as_ref().unwrap().message(), "blocked");

        let deny_full = decl("match: { tool: bash }\ndeny: { message: nope }\n");
        assert_eq!(deny_full.deny.as_ref().unwrap().message(), "nope");

        let rewrite = decl("match: { tool: read }\nrewrite: { file_path: /tmp/fixture }\n");
        assert_eq!(rewrite.action_kind(), Some("rewrite"));

        let spy = decl("name: git\nmatch: { tool: bash, pattern: \"\\\\bgit\\\\b\" }\n");
        assert!(!spy.is_mock());
        assert_eq!(spy.action_kind(), None);
    }

    #[test]
    fn typoed_keys_inside_untagged_forms_are_parse_errors() {
        // The map forms of `stub`/`deny` and a field predicate deny unknown
        // keys: a typo must not silently degrade to a default (e.g. a stub
        // whose `exit_cod: 2` quietly ran with exit 0).
        assert!(serde_yaml::from_str::<MockDecl>(
            "match: { tool: bash }\nstub: { output: boom, exit_cod: 2 }\n"
        )
        .is_err());
        assert!(serde_yaml::from_str::<MockDecl>(
            "match: { tool: bash }\ndeny: { mesage: nope }\n"
        )
        .is_err());
        assert!(serde_yaml::from_str::<MockDecl>(
            "match: { input: { command: { contians: origin } } }\nstub: ok\n"
        )
        .is_err());
    }

    #[test]
    fn validate_rejects_faults_loudly() {
        // No criteria at all.
        let d = MockDecl {
            name: None,
            matcher: MockMatch::default(),
            stub: None,
            deny: None,
            rewrite: None,
        };
        assert!(d.validate("mock `x`").is_err());
        // Empty needles.
        for yaml in [
            "match: { tool: \"\" }\n",
            "match: { contains: \"\" }\n",
            "match: { pattern: \"\" }\n",
        ] {
            assert!(decl(yaml).validate("m").is_err(), "{yaml}");
        }
        // Invalid regex is a load-time fault.
        let err = decl("match: { pattern: \"git push(\" }\n")
            .validate("mock `p`")
            .unwrap_err();
        assert!(err.to_string().contains("not a valid regex"), "{err}");
        // More than one action.
        assert!(decl("match: { tool: bash }\nstub: x\ndeny: y\n")
            .validate("m")
            .is_err());
        // Rewrite must be an object.
        assert!(decl("match: { tool: bash }\nrewrite: 3\n")
            .validate("m")
            .is_err());
        // Input predicate faults.
        assert!(decl("match: { input: { command: {} } }\n")
            .validate("m")
            .is_err());
        assert!(decl("match: { input: { command: { contains: \"\" } } }\n")
            .validate("m")
            .is_err());
        assert!(decl("match: { input: { command: { pattern: \"(\" } } }\n")
            .validate("m")
            .is_err());
        // Unknown fields are rejected (a typo must never become a spy).
        assert!(serde_yaml::from_str::<MockDecl>("match: { tool: bash }\nstubb: x\n").is_err());
    }

    fn set(yaml: &str) -> MockSet {
        let decls: Vec<MockDecl> = serde_yaml::from_str(yaml).expect("decl list must parse");
        MockSet::build(&[], &decls, false).expect("set must build")
    }

    const DECLS: &str = r#"
- name: push
  match: { tool: bash, pattern: "git push( --force)?\\b" }
  stub: Everything up-to-date
- name: danger
  match: { contains: "rm -rf" }
  deny: destructive commands are blocked
- name: git
  match: { tool: bash, pattern: "\\bgit\\b" }
"#;

    #[test]
    fn compiles_actions_only_in_order_with_oneharness_field_names() {
        let set = set(DECLS);
        assert!(set.active());
        let rules = set.rules().expect("two action decls");
        let list = rules["rules"].as_array().unwrap();
        // The spy (`git`) is not compiled — only the two actions, in order.
        assert_eq!(list.len(), 2);
        assert_eq!(list[0]["match"]["tool"], "bash");
        assert_eq!(list[0]["match"]["event_regex"], "git push( --force)?\\b");
        assert_eq!(list[0]["action"]["stub"]["output"], "Everything up-to-date");
        assert_eq!(list[0]["action"]["stub"]["exit_code"], 0);
        assert_eq!(list[1]["match"]["event_contains"], "rm -rf");
        assert_eq!(
            list[1]["action"]["deny"]["message"],
            "destructive commands are blocked"
        );
    }

    #[test]
    fn compiles_input_predicates_with_pattern_renamed_to_regex() {
        let set = set(r#"
- name: read
  match:
    tool: read
    input:
      file_path: { pattern: "secrets" }
      mode: r
  rewrite: { file_path: /tmp/fixture }
"#);
        let rule = &set.rules().unwrap()["rules"][0];
        assert_eq!(rule["match"]["input"]["file_path"]["regex"], "secrets");
        assert_eq!(rule["match"]["input"]["mode"]["equals"], "r");
        assert_eq!(
            rule["action"]["rewrite"]["input"]["file_path"],
            "/tmp/fixture"
        );
    }

    #[test]
    fn spy_only_set_has_no_rules_but_is_active() {
        let set = set("- name: git\n  match: { tool: bash }\n");
        assert!(set.rules().is_none());
        assert!(set.active());
        // And a bare `spy: true` (no decls) still activates the channel.
        let bare = MockSet::build(&[], &[], true).unwrap();
        assert!(bare.active());
        assert!(!MockSet::build(&[], &[], false).unwrap().active());
    }

    #[test]
    fn build_rejects_duplicate_names_across_shared_and_case() {
        let shared: Vec<MockDecl> =
            serde_yaml::from_str("- name: push\n  match: { tool: bash }\n  stub: x\n").unwrap();
        let case: Vec<MockDecl> =
            serde_yaml::from_str("- name: push\n  match: { tool: bash }\n").unwrap();
        let err = MockSet::build(&shared, &case, false).unwrap_err();
        assert!(err.to_string().contains("duplicate mock name"), "{err}");
    }

    fn call(tool: &str, command: &str, action: &str, rule: Option<usize>) -> MockCall {
        MockCall {
            tool: Some(tool.into()),
            input: Some(json!({ "command": command })),
            action: action.into(),
            rule,
            mock: None,
        }
    }

    #[test]
    fn resolve_fills_mock_names_and_records_for_binds_by_name() {
        let set = set(DECLS);
        let records = set.resolve(vec![
            call("Bash", "git push origin", "stub", Some(0)),
            call("Bash", "git status", "allow", None),
            call("Bash", "rm -rf /", "deny", Some(1)),
            call("Read", "n/a", "allow", None),
        ]);
        assert_eq!(records[0].mock.as_deref(), Some("push"));
        assert!(records[1].mock.is_none());
        assert_eq!(records[2].mock.as_deref(), Some("danger"));

        // A mock binds the calls its rule intercepted.
        let push = set.records_for("push", &records).unwrap();
        assert_eq!(push.len(), 1);
        assert!(push[0].mocked());
        // The spy observes every matching call — including the mocked push
        // (tool matches case-insensitively) but not the Read.
        let git = set.records_for("git", &records).unwrap();
        assert_eq!(git.len(), 2);
        // An unknown name is loud and lists what exists.
        let err = set.records_for("psuh", &records).unwrap_err();
        assert!(err.to_string().contains("unknown mock `psuh`"), "{err}");
        assert!(err.to_string().contains("push, danger, git"), "{err}");
    }

    #[test]
    fn decide_first_match_wins_and_mirrors_matcher_semantics() {
        let set = set(DECLS);
        let rules = set.rules().unwrap();
        // Regex with the optional group: both spellings intercepted.
        for command in ["git push origin", "git push --force origin"] {
            let (rule, action) =
                decide(rules, Some("Bash"), Some(&json!({ "command": command }))).unwrap();
            assert_eq!(rule, 0, "{command}");
            assert!(
                matches!(action, AppliedAction::Stub { output, exit_code: 0 }
                if output == "Everything up-to-date")
            );
        }
        // Substring rule.
        let (rule, action) = decide(
            rules,
            Some("Bash"),
            Some(&json!({ "command": "rm -rf /tmp" })),
        )
        .unwrap();
        assert_eq!(rule, 1);
        assert_eq!(action.kind(), "deny");
        // Near-miss falls through (word boundary) and wrong tool falls through.
        assert!(decide(
            rules,
            Some("Bash"),
            Some(&json!({ "command": "git pushy" }))
        )
        .is_none());
        assert!(decide(rules, Some("Read"), Some(&json!({ "command": "git push" }))).is_none());
        // No tool name at all cannot match a tool criterion.
        assert!(decide(rules, None, Some(&json!({ "command": "git push" }))).is_none());
    }

    #[test]
    fn decide_applies_input_predicates_and_coerces_non_strings() {
        let set = set(r#"
- match:
    input:
      file_path: { contains: secrets }
      depth: { equals: "3" }
  deny: no secrets
"#);
        let rules = set.rules().unwrap();
        // `depth` is a number — matched against its compact JSON form.
        let input = json!({ "file_path": "/etc/secrets.json", "depth": 3 });
        assert!(decide(rules, Some("read"), Some(&input)).is_some());
        // A missing field fails the rule.
        let input = json!({ "file_path": "/etc/secrets.json" });
        assert!(decide(rules, Some("read"), Some(&input)).is_none());
    }

    #[test]
    fn stub_command_quotes_posix_safely() {
        assert_eq!(stub_command("clean", 0), "printf '%s\\n' 'clean'");
        assert_eq!(
            stub_command("it's done", 2),
            "printf '%s\\n' 'it'\\''s done'; exit 2"
        );
    }

    #[test]
    fn where_matches_field_predicates() {
        let clause: BTreeMap<String, FieldPredicate> =
            serde_yaml::from_str("command: { contains: \"--force\" }\n").unwrap();
        assert!(where_matches(
            &clause,
            Some(&json!({ "command": "git push --force" }))
        ));
        assert!(!where_matches(
            &clause,
            Some(&json!({ "command": "git push" }))
        ));
        assert!(!where_matches(&clause, None));
        // Bare-string predicate = exact equality.
        let exact: BTreeMap<String, FieldPredicate> =
            serde_yaml::from_str("command: git status\n").unwrap();
        assert!(where_matches(
            &exact,
            Some(&json!({ "command": "git status" }))
        ));
        assert!(!where_matches(
            &exact,
            Some(&json!({ "command": "git status -s" }))
        ));
        // The empty clause matches anything.
        assert!(where_matches(&BTreeMap::new(), None));
    }

    #[test]
    fn parse_spy_log_reads_oneharness_lines_and_rejects_garbage() {
        let log = concat!(
            r#"{"harness":"claude-code","event":{"tool_name":"Bash","tool_input":{"command":"git push"}},"action":"stub","rule":0}"#,
            "\n\n",
            r#"{"harness":"claude-code","event":{"toolName":"shell","toolArgs":{"command":"ls"}},"action":"allow","rule":null}"#,
            "\n",
        );
        let records = parse_spy_log(log).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].tool.as_deref(), Some("Bash"));
        assert_eq!(records[0].input.as_ref().unwrap()["command"], "git push");
        assert_eq!(records[0].action, "stub");
        assert_eq!(records[0].rule, Some(0));
        assert!(records[0].mocked());
        // Copilot-shaped keys are extracted too; allow records carry no rule.
        assert_eq!(records[1].tool.as_deref(), Some("shell"));
        assert_eq!(records[1].rule, None);
        assert!(!records[1].mocked());
        // A truncated line is loud, never a silently shorter log.
        assert!(parse_spy_log("{\"event\":").is_err());
    }

    #[test]
    fn field_predicate_spec_forms_all_hold_locally() {
        // equals + contains + pattern in one Spec — every form must hold.
        let clause: BTreeMap<String, FieldPredicate> = serde_yaml::from_str(
            "command: { equals: \"git push\", contains: push, pattern: \"^git\" }\n",
        )
        .unwrap();
        assert!(where_matches(
            &clause,
            Some(&json!({ "command": "git push" }))
        ));
        // equals mismatch, contains miss, and pattern miss each fail.
        assert!(!where_matches(
            &clause,
            Some(&json!({ "command": "git pushx" }))
        ));
        let pat_only: BTreeMap<String, FieldPredicate> =
            serde_yaml::from_str("command: { pattern: \"^git\" }\n").unwrap();
        assert!(!where_matches(
            &pat_only,
            Some(&json!({ "command": "use git" }))
        ));
    }

    #[test]
    fn matches_call_contains_matches_over_event_haystack() {
        let matcher: MockMatch = serde_yaml::from_str("contains: \"git push\"\n").unwrap();
        assert!(matcher.matches_call(Some("Bash"), Some(&json!({ "command": "git push" }))));
        assert!(!matcher.matches_call(Some("Bash"), Some(&json!({ "command": "ls" }))));
    }

    #[test]
    fn unnamed_decls_get_positional_labels_and_empty_set_reports_none() {
        // Unnamed action decl -> mock_<i>; unnamed spy -> spy_<i>.
        let decls: Vec<MockDecl> =
            serde_yaml::from_str("- match: { tool: bash }\n  stub: x\n- match: { tool: read }\n")
                .unwrap();
        let set = MockSet::build(&[], &decls, false).unwrap();
        let records = set.resolve(vec![call("bash", "ls", "stub", Some(0))]);
        assert_eq!(records[0].mock.as_deref(), Some("mock_0"));
        assert_eq!(set.records_for("spy_1", &records).unwrap().len(), 0);
        assert_eq!(set.decls().len(), 2);
        // No declarations at all: the unknown-name error says "none".
        let empty = MockSet::build(&[], &[], true).unwrap();
        let err = empty.records_for("ghost", &[]).unwrap_err();
        assert!(err.to_string().contains("declared: none"), "{err}");
    }

    #[test]
    fn decide_applies_rewrite_actions_and_reports_kind() {
        let set = set(
            "- match: { tool: read, input: { file_path: { pattern: \"secrets\" } } }\n  rewrite: { file_path: /tmp/fixture }\n",
        );
        let rules = set.rules().unwrap();
        let input = json!({ "file_path": "/etc/secrets.json" });
        let (rule, action) = decide(rules, Some("read"), Some(&input)).unwrap();
        assert_eq!(rule, 0);
        assert_eq!(action.kind(), "rewrite");
        assert!(matches!(action, AppliedAction::Rewrite { input }
            if input["file_path"] == "/tmp/fixture"));
        // The regex input-predicate must actually gate: a non-matching path
        // falls through.
        assert!(decide(
            rules,
            Some("read"),
            Some(&json!({ "file_path": "/etc/ok" }))
        )
        .is_none());
    }

    #[test]
    fn describe_records_caps_and_shows_verdicts() {
        let mut records: Vec<MockCall> = (0..7)
            .map(|i| call("bash", &format!("cmd{i}"), "allow", None))
            .collect();
        records[0] = call("bash", "git push", "deny", Some(0));
        // A record with a non-command input falls back to its compact JSON.
        records[1] = MockCall {
            tool: Some("read".into()),
            input: Some(json!({ "file_path": "/x" })),
            action: "allow".into(),
            rule: None,
            mock: None,
        };
        let text = describe_records(&records);
        assert!(text.contains("bash(git push) [deny]"), "{text}");
        assert!(text.contains("read({\"file_path\":\"/x\"})"), "{text}");
        assert!(text.contains("… 2 more"), "{text}");
        // And an input-less record renders without a detail.
        let bare = MockCall {
            tool: None,
            input: None,
            action: "allow".into(),
            rule: None,
            mock: None,
        };
        assert_eq!(describe_records(&[bare]), "?()");
    }

    #[test]
    fn matches_call_composes_all_criteria() {
        let matcher: MockMatch = serde_yaml::from_str(
            "tool: bash\npattern: \"git (status|diff)\"\ninput: { command: { contains: git } }\n",
        )
        .unwrap();
        assert!(matcher.matches_call(Some("Bash"), Some(&json!({ "command": "git status" }))));
        assert!(!matcher.matches_call(Some("Bash"), Some(&json!({ "command": "git push" }))));
        assert!(!matcher.matches_call(Some("read"), Some(&json!({ "command": "git status" }))));
        assert!(!matcher.matches_call(None, Some(&json!({ "command": "git status" }))));
    }
}
