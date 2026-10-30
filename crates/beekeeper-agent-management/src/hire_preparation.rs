//! Durable preparation for a reserved, never-published responsibility.
use crate::{
    management::local::{fail, save},
    Error,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Preparation {
    binding: Value,
    complete: bool,
}

/// Complete a known unlaunched checkout. The caller must hold the request lock
/// and must never call this after publication may have begun. A preexisting tree
/// without our durable intent is not ours to reset. Completed trees are verified,
/// never reset, even if their content has subsequently changed.
#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare(
    journal: &Path,
    binding: Value,
    destination: &Path,
    root: &Path,
    create: &[&str],
    checkout: &[&str],
    revision: &str,
    mut git: impl FnMut(&Path, &[&str]) -> Result<String, Error>,
    verify_identity: impl Fn(
        &mut dyn FnMut(&Path, &[&str]) -> Result<String, Error>,
    ) -> Result<(), Error>,
) -> Result<bool, Error> {
    let mut record: Preparation = match crate::management::local::read_operation_value(journal)? {
        Some(value) => serde_json::from_value(value).map_err(fail)?,
        None => {
            if destination.exists() {
                return Err(Error::Conflict(
                    "preexisting tree has no unlaunched preparation record; refusing reset".into(),
                ));
            }
            let record = Preparation {
                binding: binding.clone(),
                complete: false,
            };
            save(journal, &record)?;
            record
        }
    };
    if record.binding != binding {
        return Err(Error::Conflict("preparation binding changed".into()));
    }
    let fresh = !record.complete;
    if !destination.exists() {
        if record.complete {
            return Err(Error::Conflict("completed tree disappeared".into()));
        }
        git(root, create)?;
    }
    let top = git(destination, &["rev-parse", "--show-toplevel"])?;
    if std::fs::canonicalize(top).map_err(fail)?
        != std::fs::canonicalize(destination).map_err(fail)?
    {
        return Err(Error::Conflict(
            "reserved path is not the repository root; refusing checkout".into(),
        ));
    }
    verify_identity(&mut git)?;
    if !record.complete {
        git(destination, checkout)?;
    }
    if git(destination, &["rev-parse", "HEAD"])? != revision
        || !git(destination, &["diff", "--name-only", revision, "--"])?.is_empty()
        || !git(
            destination,
            &["diff", "--cached", "--name-only", revision, "--"],
        )?
        .is_empty()
    {
        return Err(Error::Conflict(
            "reserved checkout is not fully materialized at its pinned revision; refusing reset"
                .into(),
        ));
    }
    record.complete = true;
    save(journal, &record)?;
    Ok(fresh)
}

#[cfg(test)]
#[path = "hire_preparation_tests.rs"]
mod tests;
