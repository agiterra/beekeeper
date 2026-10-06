//! Validate a plan's source before the Files tab saves or commits it
//! (ledger 249(B)).
//!
//! The same `buzz-core` reader every other surface uses, so a plan the
//! desktop lets through is one `bee plans` and the relay would also read.

use beekeeper_core_pkg::project_plan::parse_plan;
use serde::Serialize;

/// The refusal, in the reader's own words; `None` when the plan reads.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlanSourceCheck {
    /// Stable refusal code, e.g. `missing_frontmatter`, or `None`.
    pub code: Option<String>,
    /// Where the defect is, e.g. `criteria[2].id`.
    pub path: Option<String>,
    /// One sentence a person can act on.
    pub message: Option<String>,
}

/// Check `text` against `beekeeper-plan/v1`. Pure: no I/O.
pub fn check_plan_source(text: &str) -> PlanSourceCheck {
    match parse_plan(text.as_bytes()) {
        Ok(_) => PlanSourceCheck {
            code: None,
            path: None,
            message: None,
        },
        Err(refusal) => PlanSourceCheck {
            code: Some(refusal.code.as_str().to_owned()),
            path: Some(refusal.path),
            message: Some(refusal.message),
        },
    }
}

/// Tauri boundary for [`check_plan_source`].
#[tauri::command]
pub fn validate_plan_source(text: String) -> PlanSourceCheck {
    check_plan_source(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAN: &str = include_str!("../../../tests/fixtures/kettle-control-2-plan.md");

    #[test]
    fn the_run_2_plan_reads_and_its_flattened_commit_does_not() {
        assert_eq!(check_plan_source(PLAN).code, None);
        // What 32cb99de committed: the opening `---` gone.
        let mangled = PLAN.replacen("---\n", "", 1);
        assert!(check_plan_source(&mangled).code.is_some());
    }
}
