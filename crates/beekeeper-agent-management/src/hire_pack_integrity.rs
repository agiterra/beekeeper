//! Bounded byte-level pin for the installed composed role, including skill files.
use crate::{management::local::fail, Error};
use sha2::{Digest, Sha256};
use std::path::Path;

pub(crate) fn digest(root: &Path) -> Result<String, Error> {
    fn visit(
        root: &Path,
        path: &Path,
        hash: &mut Sha256,
        count: &mut usize,
        bytes: &mut u64,
    ) -> Result<(), Error> {
        let mut entries = std::fs::read_dir(path)
            .map_err(fail)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(fail)?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            *count += 1;
            if *count > 4096 {
                return Err(Error::Refused("composed role exceeds 4096 entries".into()));
            }
            let path = entry.path();
            let meta = std::fs::symlink_metadata(&path).map_err(fail)?;
            let name = path
                .strip_prefix(root)
                .map_err(fail)?
                .to_str()
                .ok_or_else(|| fail("role path is not UTF-8"))?
                .as_bytes();
            hash.update((name.len() as u64).to_be_bytes());
            hash.update(name);
            if meta.is_dir() {
                hash.update([0]);
                visit(root, &path, hash, count, bytes)?;
            } else if meta.is_file() {
                *bytes += meta.len();
                if *bytes > 16 * 1024 * 1024 {
                    return Err(Error::Refused("composed role exceeds 16 MiB".into()));
                }
                hash.update([1]);
                let data = std::fs::read(&path).map_err(fail)?;
                hash.update((data.len() as u64).to_be_bytes());
                hash.update(data);
            } else {
                return Err(Error::Refused(
                    "composed role contains symlink or special file".into(),
                ));
            }
        }
        Ok(())
    }
    let mut hash = Sha256::new();
    visit(root, root, &mut hash, &mut 0, &mut 0)?;
    Ok(hex::encode(hash.finalize()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn skill_mutation_changes_pin_without_provenance_change() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("compose.json"), "same provenance").unwrap();
        std::fs::write(dir.path().join("SKILL.md"), "original").unwrap();
        let before = super::digest(dir.path()).unwrap();
        std::fs::write(dir.path().join("SKILL.md"), "changed").unwrap();
        assert_ne!(before, super::digest(dir.path()).unwrap());
    }
}
