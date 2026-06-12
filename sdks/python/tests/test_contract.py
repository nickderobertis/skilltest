"""Contract tests: the SDK models must stay in sync with the CLI's JSON Schema.

The golden schemas in `schemas/` are generated from the Rust report types
(`skilltest schema`, regenerated with `just gen-schemas`) and guarded against
drift by the Rust e2e suite. Here we compare every Pydantic model field-by-field
against those goldens, in both directions, so a field added or removed on either
side fails loudly with the exact difference.
"""

from __future__ import annotations

import json
import typing
from pathlib import Path
from typing import Any

from pydantic import BaseModel

from skilltest_sdk import (
    BooleanDetail,
    CaseRun,
    EvalOutcome,
    Message,
    NumericDetail,
    Report,
    Summary,
    Transcript,
    Usage,
    ValidationFinding,
    ValidationReport,
)
from skilltest_sdk.models import Comparator

#: Schema definition name -> the SDK model mirroring it. Object schemas only;
#: enums and tagged unions are checked separately below.
REPORT_DEF_MODELS: dict[str, type[BaseModel]] = {
    "CaseRun": CaseRun,
    "EvalOutcome": EvalOutcome,
    "Message": Message,
    "Summary": Summary,
    "Transcript": Transcript,
    "Usage": Usage,
}

VALIDATION_DEF_MODELS: dict[str, type[BaseModel]] = {
    "ValidationFinding": ValidationFinding,
}


def load_schema(schemas: Path, name: str) -> dict[str, Any]:
    return json.loads((schemas / name).read_text())


def assert_object_matches(model: type[BaseModel], schema: dict[str, Any], where: str) -> None:
    """Model fields and required-ness must match the schema object exactly."""
    schema_props = set(schema["properties"])
    schema_required = set(schema.get("required", []))
    model_fields = set(model.model_fields)
    model_required = {name for name, f in model.model_fields.items() if f.is_required()}
    assert model_fields == schema_props, (
        f"{where}: model fields {sorted(model_fields)} != schema properties "
        f"{sorted(schema_props)} — run `just gen-schemas` and update the models together"
    )
    assert model_required == schema_required, (
        f"{where}: required fields {sorted(model_required)} != schema required "
        f"{sorted(schema_required)}"
    )


def variant_consts(one_of: list[dict[str, Any]]) -> set[str]:
    """The `const` values of a schema enum rendered as a oneOf of consts."""
    return {v["const"] for v in one_of if "const" in v}


def test_report_models_match_golden_schema(schemas: Path) -> None:
    schema = load_schema(schemas, "report.schema.json")
    defs = schema["$defs"]

    assert_object_matches(Report, schema, "Report")
    covered = set(REPORT_DEF_MODELS) | {"EvalDetail", "Comparator", "Role"}
    assert set(defs) == covered, (
        f"schema $defs {sorted(defs)} != models covered by this test {sorted(covered)} — "
        "a type was added to or removed from the contract"
    )
    for name, model in REPORT_DEF_MODELS.items():
        assert_object_matches(model, defs[name], name)


def test_eval_detail_variants_match_golden_schema(schemas: Path) -> None:
    defs = load_schema(schemas, "report.schema.json")["$defs"]
    variants = {v["properties"]["kind"]["const"]: v for v in defs["EvalDetail"]["oneOf"]}
    assert set(variants) == {"boolean", "numeric"}
    assert_object_matches(BooleanDetail, variants["boolean"], "EvalDetail.boolean")
    assert_object_matches(NumericDetail, variants["numeric"], "EvalDetail.numeric")


def test_enums_match_golden_schema(schemas: Path) -> None:
    defs = load_schema(schemas, "report.schema.json")["$defs"]
    assert set(typing.get_args(Comparator)) == variant_consts(defs["Comparator"]["oneOf"])
    role_literal = Message.model_fields["role"].annotation
    assert set(typing.get_args(role_literal)) == variant_consts(defs["Role"]["oneOf"])


def test_validation_models_match_golden_schema(schemas: Path) -> None:
    schema = load_schema(schemas, "validation.schema.json")
    defs = schema["$defs"]

    assert_object_matches(ValidationReport, schema, "ValidationReport")
    assert set(defs) == set(VALIDATION_DEF_MODELS)
    for name, model in VALIDATION_DEF_MODELS.items():
        assert_object_matches(model, defs[name], name)
