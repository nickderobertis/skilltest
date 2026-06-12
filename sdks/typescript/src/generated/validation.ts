/* eslint-disable */
/**
 * Generated from the golden JSON Schemas in schemas/ by `just gen-contract`.
 * DO NOT MODIFY BY HAND — change the Rust report types and regenerate; the
 * contract drift gate fails while this file is stale.
 */

/**
 * The top-level report for a `skilltest validate` invocation.
 */
export interface ValidationReport {
  /**
   * Every finding, in discovery order.
   */
  findings: ValidationFinding[];
  /**
   * True iff no findings were produced.
   */
  valid: boolean;
}
/**
 * One problem found while validating a skill, as serialized in the
 * `skilltest validate --format json` output.
 */
export interface ValidationFinding {
  /**
   * What is wrong and how to fix it.
   */
  message: string;
  /**
   * The skill directory the finding is about.
   */
  skill: string;
}
