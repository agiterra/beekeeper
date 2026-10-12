//! Restricted immutable input snapshots for every enrollment operation.
use super::*;
fn operation_path(dir: &Path, operation: &str) -> Result<PathBuf, String> {
    let id = Uuid::parse_str(operation).map_err(|_| "enrollment operation invalid")?;
    Ok(dir.join("enrollment-inputs").join(format!("{id}.json")))
}
pub(super) fn enrollment_stage(dir: &Path, operation: &str) -> Result<PathBuf, String> {
    let exact = operation_path(dir, operation)?;
    for path in [exact, dir.join("active-stage.json"), dir.join("stage.json")] {
        if !path.exists() {
            continue;
        }
        let staged: ImportStage = serde_json::from_slice(&private_bytes(&path)?)
            .map_err(|_| "host enrollment journal invalid")?;
        if staged.operation_id.to_string() == operation {
            return Ok(path);
        }
    }
    Err("no matching protected enrollment inputs; retain backup for explicit repair".into())
}
fn capture(dir: &Path, staged: &ImportStage) -> Result<PathBuf, String> {
    let path = operation_path(dir, &staged.operation_id.to_string())?;
    let bytes = serde_json::to_vec(staged).map_err(|_| "enrollment inputs invalid")?;
    if path.exists() {
        if private_bytes(&path)? != bytes {
            return Err("enrollment operation inputs changed; protected original retained".into());
        }
    } else {
        save(&path, staged)?;
    }
    Ok(path)
}
pub(super) fn save_enrollment_stage(dir: &Path, staged: &ImportStage) -> Result<(), String> {
    let active = dir.join("active-stage.json");
    // Retain the predecessor before publishing a successor pointer. This also
    // upgrades an interrupted earlier journal without losing its exact inputs.
    if active.exists() {
        let previous: ImportStage = serde_json::from_slice(&private_bytes(&active)?)
            .map_err(|_| "active enrollment inputs invalid")?;
        capture(dir, &previous)?;
    }
    capture(dir, staged)?;
    save(&active, staged)
}
pub(super) fn pending_successor(
    dir: &Path,
    operation: Uuid,
) -> Result<Option<ImportStage>, String> {
    let path = operation_path(dir, &operation.to_string())?;
    if !path.exists() {
        return Ok(None);
    }
    let stage: ImportStage = serde_json::from_slice(&private_bytes(&path)?)
        .map_err(|_| "pending successor inputs invalid")?;
    if stage.operation_id != operation {
        return Err("successor journal scope differs".into());
    }
    Ok(Some(stage))
}
