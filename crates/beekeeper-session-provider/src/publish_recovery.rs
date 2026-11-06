//! Validate complete outbox records before replay; quarantine an uncommitted tail.
//! The provider's state-directory lock serializes recovery and append. A newline
//! is the commit boundary of a record; a successful append always includes it.
#[cfg(unix)]
use std::fs::File;
use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::path::Path;

pub(super) fn recover<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<Vec<T>> {
    let mut file = match OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let boundary = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
    let mut rows = Vec::new();
    let mut offset = 0;
    // Validate the entire committed prefix BEFORE making any mutation. A bad
    // newline-terminated row is corruption, even when it is the final row.
    for line in bytes[..boundary].split_inclusive(|b| *b == b'\n') {
        if !line.iter().all(u8::is_ascii_whitespace) {
            rows.push(serde_json::from_slice(line).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "outbox corruption at byte {offset} in {}: {error}; ledger unchanged",
                        path.display()
                    ),
                )
            })?);
        }
        offset += line.len();
    }
    if boundary < bytes.len() {
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("outbox has no parent"))?;
        let quarantine = parent.join(format!(
            "outbox-torn-tail-{boundary}-{}.bin",
            uuid::Uuid::new_v4()
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.custom_flags(0x8000_0000);
        } // FILE_FLAG_WRITE_THROUGH
        let mut saved = options.open(&quarantine)?;
        saved.write_all(&bytes[boundary..])?;
        saved.sync_all()?;
        #[cfg(unix)]
        File::open(parent)?.sync_all()?;
        // If saving or syncing fails, leave the original bytes untouched.
        // If interrupted after this point, a repeat recovery may make another
        // quarantine copy, but never discards the only copy of damaged bytes.
        file.set_len(boundary as u64)?;
        file.sync_all()?;
        tracing::warn!(target: "csp::outbox", offset = boundary, bytes = bytes.len() - boundary,
            quarantine = %quarantine.display(), "recovered torn outbox tail; preserved bytes before truncation");
    }
    // Disclose retained evidence on later opens too, including a crash after
    // truncation but before the first recovery warning could be emitted.
    if let Some(parent) = path.parent() {
        for entry in std::fs::read_dir(parent)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("outbox-torn-tail-") && name.ends_with(".bin") {
                tracing::warn!(target: "csp::outbox", quarantine = %entry.path().display(),
                    "retained torn outbox bytes require inspection; recovery evidence has not been deleted");
            }
        }
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    #[derive(Debug, Deserialize, Serialize, PartialEq)]
    struct Row {
        id: String,
    }
    fn quarantines(dir: &Path) -> Vec<Vec<u8>> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "bin"))
            .map(|p| std::fs::read(p).unwrap())
            .collect()
    }
    #[test]
    fn every_byte_cut_including_split_utf8_preserves_prefix_and_next_append() {
        let committed = b"{\"id\":\"acknowledged\"}\n";
        let next = "{\"id\":\"unfinished-☃-é\"}\n".as_bytes();
        for cut in 1..next.len() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("outbox.jsonl");
            let mut original = committed.to_vec();
            original.extend_from_slice(&next[..cut]);
            std::fs::write(&path, &original).unwrap();
            assert_eq!(
                recover::<Row>(&path).unwrap(),
                vec![Row {
                    id: "acknowledged".into()
                }]
            );
            assert_eq!(quarantines(dir.path()), vec![next[..cut].to_vec()]);
            assert_eq!(std::fs::read(&path).unwrap(), committed);
            let mut append = OpenOptions::new().append(true).open(&path).unwrap();
            append
                .write_all(b"{\"id\":\"later-acknowledged\"}\n")
                .unwrap();
            append.sync_all().unwrap();
            assert_eq!(
                recover::<Row>(&path)
                    .unwrap()
                    .iter()
                    .map(|r| r.id.as_str())
                    .collect::<Vec<_>>(),
                ["acknowledged", "later-acknowledged"]
            );
            assert_eq!(quarantines(dir.path()).len(), 1);
        }
    }
    #[test]
    fn interior_corruption_and_newline_terminated_corruption_are_never_skipped() {
        for damage in [b"broken\n".as_slice(), b"{\"id\":\"\xff\"}\n", b"{}\n"] {
            for suffix in [b"".as_slice(), b"{\"id\":\"later\"}\n{\"torn\""] {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("outbox.jsonl");
                let mut bytes = b"{\"id\":\"first\"}\n".to_vec();
                bytes.extend_from_slice(damage);
                bytes.extend_from_slice(suffix);
                std::fs::write(&path, &bytes).unwrap();
                assert_eq!(
                    recover::<Row>(&path).unwrap_err().kind(),
                    io::ErrorKind::InvalidData
                );
                assert_eq!(std::fs::read(&path).unwrap(), bytes);
                assert!(quarantines(dir.path()).is_empty());
            }
        }
    }
    #[test]
    fn only_tail_and_empty_ledger_recover_without_inventing_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("outbox.jsonl");
        assert!(recover::<Row>(&path).unwrap().is_empty());
        std::fs::write(&path, [0xff, 0xe2]).unwrap();
        assert!(recover::<Row>(&path).unwrap().is_empty());
        assert_eq!(quarantines(dir.path()), [vec![0xff, 0xe2]]);
        assert!(recover::<Row>(&path).unwrap().is_empty());
    }
}
