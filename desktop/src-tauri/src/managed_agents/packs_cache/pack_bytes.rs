//! Are two pack directories the same bytes?
//!
//! The second half of [`super::shipped_pack_ref_for_dir`]'s recognition. A
//! development build's [`super::shipped_packs_dir`] is `tauri-build`'s copy of
//! the bundle resources under the desktop crate's target directory, while an
//! installed role pack points at the checkout that copy was made from. Path
//! equality says those are different packs; the bytes say they are one pack
//! (finding 72). This module answers from the bytes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Finder's per-directory metadata. Not part of any pack; a checkout that has
/// been opened in Finder carries one and the resource copy does not.
const IGNORED_FILE_NAMES: &[&str] = &[".DS_Store"];

/// How deep a pack may nest before the walk stops trusting it. Packs are a
/// persona file and a handful of skill directories; a tree deeper than this
/// is a symlink loop or something that is not a pack, and either way it is
/// not recognised as the shipped one.
const MAX_DEPTH: usize = 16;

/// `true` when `left` and `right` are both directories holding exactly the
/// same relative regular files with exactly the same contents.
///
/// Symlinks are followed (the copy has none; the checkout may). Any directory
/// that cannot be read, any file that cannot be read, and any tree deeper
/// than [`MAX_DEPTH`] answers `false`: a comparison that could not finish is
/// not a match. `.DS_Store` files are ignored on both sides.
pub(super) fn same_pack_bytes(left: &Path, right: &Path) -> bool {
    match (read_pack_files(left), read_pack_files(right)) {
        (Some(left), Some(right)) => !left.is_empty() && left == right,
        _ => false,
    }
}

/// Every regular file under `root`, keyed by its path relative to `root`.
fn read_pack_files(root: &Path) -> Option<BTreeMap<PathBuf, Vec<u8>>> {
    if !root.is_dir() {
        return None;
    }
    let mut files = BTreeMap::new();
    collect_files(root, PathBuf::new(), 0, &mut files)?;
    Some(files)
}

fn collect_files(
    dir: &Path,
    relative: PathBuf,
    depth: usize,
    files: &mut BTreeMap<PathBuf, Vec<u8>>,
) -> Option<()> {
    if depth > MAX_DEPTH {
        return None;
    }
    for entry in std::fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name();
        if IGNORED_FILE_NAMES
            .iter()
            .any(|ignored| name.as_os_str() == *ignored)
        {
            continue;
        }
        let path = entry.path();
        let relative = relative.join(&name);
        // `metadata` follows symlinks; a link to a directory is walked and a
        // link to a file is read, so the checkout's layout and the copy's
        // compare by what a reader would see.
        let metadata = std::fs::metadata(&path).ok()?;
        if metadata.is_dir() {
            collect_files(&path, relative, depth + 1, files)?;
        } else if metadata.is_file() {
            files.insert(relative, std::fs::read(&path).ok()?);
        } else {
            return None;
        }
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, relative: &str, bytes: &[u8]) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, bytes).expect("write");
    }

    fn pack(root: &Path) {
        write(root, ".plugin/plugin.json", b"{\"id\":\"com.test.lead\"}");
        write(
            root,
            "personas/lead.persona.md",
            b"---\nname: lead\nrole: lead\n---\n",
        );
        write(root, "skills/hire/SKILL.md", b"# hire\n");
    }

    #[test]
    fn a_copy_with_the_same_files_and_bytes_is_the_same_pack() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let shipped = tmp.path().join("target/debug/personas/roles/lead");
        let checkout = tmp.path().join("checkout/personas/roles/lead");
        pack(&shipped);
        pack(&checkout);
        // Finder metadata on the operator's side is not pack content.
        write(&checkout, ".DS_Store", b"\0\0");
        assert!(same_pack_bytes(&shipped, &checkout));
        assert!(same_pack_bytes(&checkout, &shipped));
    }

    #[test]
    fn one_changed_byte_one_extra_file_or_one_missing_file_is_a_different_pack() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let shipped = tmp.path().join("shipped/lead");
        pack(&shipped);

        let edited = tmp.path().join("edited/lead");
        pack(&edited);
        write(&edited, "skills/hire/SKILL.md", b"# hire, edited\n");
        assert!(!same_pack_bytes(&shipped, &edited));

        let extra = tmp.path().join("extra/lead");
        pack(&extra);
        write(&extra, "skills/new/SKILL.md", b"# new\n");
        assert!(!same_pack_bytes(&shipped, &extra));

        let missing = tmp.path().join("missing/lead");
        pack(&missing);
        std::fs::remove_file(missing.join("skills/hire/SKILL.md")).expect("remove");
        assert!(!same_pack_bytes(&shipped, &missing));
    }

    #[test]
    fn a_missing_directory_or_two_empty_ones_never_match() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let shipped = tmp.path().join("shipped/lead");
        pack(&shipped);
        assert!(!same_pack_bytes(&shipped, &tmp.path().join("nowhere")));
        // Two directories with nothing in them hold no pack to recognise.
        let empty_a = tmp.path().join("a");
        let empty_b = tmp.path().join("b");
        std::fs::create_dir_all(&empty_a).expect("mkdir");
        std::fs::create_dir_all(&empty_b).expect("mkdir");
        assert!(!same_pack_bytes(&empty_a, &empty_b));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_file_compares_by_what_it_points_at() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let shipped = tmp.path().join("shipped/lead");
        pack(&shipped);
        let linked = tmp.path().join("linked/lead");
        pack(&linked);
        std::fs::remove_file(linked.join("skills/hire/SKILL.md")).expect("remove");
        write(tmp.path(), "elsewhere/SKILL.md", b"# hire\n");
        std::os::unix::fs::symlink(
            tmp.path().join("elsewhere/SKILL.md"),
            linked.join("skills/hire/SKILL.md"),
        )
        .expect("symlink");
        assert!(same_pack_bytes(&shipped, &linked));
    }
}
