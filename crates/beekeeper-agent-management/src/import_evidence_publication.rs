//! Exclusive, durable publication of complete restricted import evidence.
use crate::Error;
use std::path::Path;

pub(super) fn sync_parent(path: &Path) -> Result<(), Error> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Other("import evidence directory missing".into()))?;
    std::fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| Error::Other("import evidence directory durability unconfirmed".into()))
}
pub(super) fn publish(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Other("import evidence directory missing".into()))?;
    let temporary = parent.join(format!(".import-evidence-{}.tmp", uuid::Uuid::new_v4()));
    beekeeper_host_core::atomic_write::atomic_write_json_restricted(&temporary, bytes)
        .map_err(|_| Error::Other("cannot stage restricted import evidence".into()))?;
    std::fs::File::open(&temporary)
        .and_then(|file| file.sync_all())
        .map_err(|_| Error::Other("import evidence staging durability unconfirmed".into()))?;
    #[cfg(test)]
    crash_cut(&temporary, "partial-temporary")?;
    // A hard link publishes the complete inode exclusively. Unlike rename it
    // cannot replace a concurrently published different signed artifact.
    let result = match std::fs::hard_link(&temporary, path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            match super::read(path)? {
                Some(old) if old == bytes => Ok(()),
                _ => Err(Error::Conflict("original import evidence conflicts".into())),
            }
        }
        Err(_) => Err(Error::Other(
            "cannot publish restricted import evidence".into(),
        )),
    };
    #[cfg(test)]
    if result.is_ok() {
        crash_cut(&temporary, "published-before-reply")?;
    }
    let _ = std::fs::remove_file(&temporary);
    result?;
    sync_parent(path)
}
#[cfg(test)]
fn crash_cut(temporary: &Path, phase: &str) -> Result<(), Error> {
    if std::env::var("BEEKEEPER_TEST_IMPORT_EVIDENCE_CRASH").as_deref() == Ok(phase) {
        if phase == "partial-temporary" {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .open(temporary)
                .map_err(|_| Error::Other("fixture temporary unavailable".into()))?;
            file.set_len(3)
                .and_then(|_| file.sync_all())
                .map_err(|_| Error::Other("fixture partial temporary unavailable".into()))?;
        }
        std::process::exit(86);
    }
    Ok(())
}
