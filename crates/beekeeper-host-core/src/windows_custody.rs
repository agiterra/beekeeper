//! Fail-closed custody of an explicitly private Windows directory tree.
//!
//! Never rewrites existing ACLs. Each ancestor is opened as a reparse object,
//! checked through that handle, and held without write/delete sharing for the
//! process lifetime. Missing components may only inherit from an already
//! verified current-user-only parent. Provision that first private root outside
//! this API; a normal profile ACL is not silently promoted to private custody.
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf, Prefix};
use std::sync::{Mutex, OnceLock};
use windows_permissions::{
    constants::{SeObjectType, SecurityInformation},
    wrappers, Sid,
};

#[derive(Default)]
struct Custody {
    pins: HashMap<PathBuf, File>,
    validated: HashSet<PathBuf>,
}
static CUSTODY: OnceLock<Mutex<Custody>> = OnceLock::new();
const REPARSE: u32 = 0x400;
const DIRECTORY_FLAGS: u32 = 0x0200_0000 | 0x0020_0000; // BACKUP_SEMANTICS | OPEN_REPARSE_POINT

fn denied(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

fn open_directory(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .access_mode(0x0002_0080) // READ_CONTROL | FILE_READ_ATTRIBUTES
        .share_mode(1) // FILE_SHARE_READ: no mutation/rename of the inspected object
        .custom_flags(DIRECTORY_FLAGS)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.is_dir() || meta.file_attributes() & REPARSE != 0 {
        return Err(denied(
            "Windows custody refuses reparse points and non-directory ancestors",
        ));
    }
    Ok(file)
}

fn private(file: &File, user: &Sid, directory: bool) -> io::Result<bool> {
    let descriptor = wrappers::GetSecurityInfo(
        file,
        SeObjectType::SE_FILE_OBJECT,
        SecurityInformation::Owner | SecurityInformation::Dacl,
    )?;
    if descriptor.owner() != Some(user) {
        return Err(denied("private state must be owned by the current user"));
    }
    let dacl = descriptor
        .dacl()
        .ok_or_else(|| denied("private state must have a DACL"))?;
    if dacl.len() != 1 {
        return Err(denied("private state requires exactly one current-user allow ACE; pre-provision a private root"));
    }
    let ace = dacl
        .get_ace(0)
        .ok_or_else(|| denied("missing private-state ACE"))?;
    if ace.ace_type() as u32 != 0 || ace.sid() != Some(user) {
        return Err(denied(
            "private state grants access beyond the current user",
        ));
    }
    let mask = ace.mask().bits();
    if mask & 0x001f_01ff != 0x001f_01ff && mask & 0x1000_0000 == 0 {
        return Err(denied(
            "private state does not grant the current user full access",
        ));
    }
    // Inherit-only ACEs do not protect this object. Directories must pass the
    // same restriction to newly created files and directories.
    let flags = ace.flags().bits();
    if flags & 8 != 0 || (directory && (flags & 3 != 3 || flags & 4 != 0)) {
        return Err(denied(
            "private directory ACL must apply here and inherit to files and directories",
        ));
    }
    let sddl = wrappers::ConvertSecurityDescriptorToStringSecurityDescriptor(
        &descriptor,
        SecurityInformation::Dacl,
    )?;
    Ok(sddl.to_string_lossy().starts_with("D:P"))
}

fn audit_tree(path: &Path, user: &Sid, pins: &mut Vec<(PathBuf, File)>) -> io::Result<()> {
    for entry in std::fs::read_dir(path)? {
        let path = entry?.path();
        // Handle metadata, not a pathname precheck. The containing directory
        // is already private and pinned; another account cannot plant entries.
        let file = OpenOptions::new()
            .access_mode(0x0002_0080)
            .share_mode(1)
            .custom_flags(DIRECTORY_FLAGS)
            .open(&path)?;
        let meta = file.metadata()?;
        if meta.file_attributes() & REPARSE != 0 {
            return Err(denied("private state contains a reparse point"));
        }
        private(&file, user, meta.is_dir())?;
        if meta.is_dir() {
            pins.push((path.clone(), file));
            audit_tree(&path, user, pins)?;
        }
        // Files are replaced by atomic writers. Their private pinned parent
        // protects the namespace; no file handle is retained across replacement.
    }
    Ok(())
}

pub(super) fn ensure(dir: &Path) -> io::Result<()> {
    let mut custody = CUSTODY
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| io::Error::other("Windows custody lock poisoned"))?;
    if custody.validated.contains(dir) {
        return Ok(());
    }
    let user = windows_permissions::utilities::current_process_sid()?;
    let mut components = dir.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return Err(denied("private root must be an absolute local drive path"));
    };
    if !matches!(prefix.kind(), Prefix::Disk(_)) || components.next() != Some(Component::RootDir) {
        return Err(denied(
            "private root refuses UNC, device, relative and verbatim paths",
        ));
    }
    let mut path = PathBuf::from(prefix.as_os_str());
    path.push(Path::new(r"\"));
    let mut pins = vec![(path.clone(), open_directory(&path)?)];
    let mut secured = false;
    for component in components {
        let Component::Normal(name) = component else {
            return Err(denied("private root refuses parent traversal"));
        };
        path.push(name);
        let next = match open_directory(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if !secured {
                    return Err(denied(
                        "creation requires a pre-provisioned protected current-user-only root",
                    ));
                }
                let parent = pins.last().ok_or_else(|| denied("missing pinned parent"))?;
                private(&parent.1, &user, true)?;
                match std::fs::create_dir(&path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error),
                }
                open_directory(&path)?
            }
            Err(error) => return Err(error),
        };
        secured = match private(&next, &user, true) {
            Ok(protected) => protected || secured,
            Err(_) => false,
        };
        pins.push((path.clone(), next));
    }
    private(
        &pins.last().ok_or_else(|| denied("missing private root"))?.1,
        &user,
        true,
    )?;
    if !secured {
        return Err(denied(
            "private root requires a protected current-user-only custody anchor",
        ));
    }
    audit_tree(dir, &user, &mut pins)?;
    custody.pins.extend(pins);
    custody.validated.insert(dir.to_path_buf());
    Ok(())
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    // Test fixture only: an empty temp directory, never an operator's tree.
    pub(crate) fn provision_empty_fixture_root(path: &Path) {
        assert!(std::fs::read_dir(path)
            .expect("fixture directory")
            .next()
            .is_none());
        let user = windows_permissions::utilities::current_process_sid().expect("user SID");
        let sid = wrappers::ConvertSidToStringSid(&user).expect("SID string");
        let sd: windows_permissions::LocalBox<windows_permissions::SecurityDescriptor> =
            format!("D:P(A;OICI;FA;;;{})", sid.to_string_lossy())
                .parse()
                .expect("fixture descriptor");
        let mut file = OpenOptions::new()
            .access_mode(0x0006_0080)
            .share_mode(1)
            .custom_flags(DIRECTORY_FLAGS)
            .open(path)
            .expect("fixture ACL handle");
        wrappers::SetSecurityInfo(
            &mut file,
            SeObjectType::SE_FILE_OBJECT,
            SecurityInformation::Dacl | SecurityInformation::ProtectedDacl,
            None,
            None,
            sd.dacl(),
            None,
        )
        .expect("protect empty fixture root");
    }

    #[test]
    fn new_descendants_inherit_current_user_only_access() {
        let temp = tempfile::tempdir().expect("temp");
        provision_empty_fixture_root(temp.path());
        let nested = temp.path().join("private/child");
        ensure(&nested).expect("create beneath protected anchor");
        let path = nested.join("secret");
        std::fs::write(&path, b"fixture").expect("write");
        let file = File::open(&path).expect("open");
        let user = windows_permissions::utilities::current_process_sid().expect("user");
        private(&file, &user, false).expect("inherited user-only file ACL");
    }

    fn dacl(path: &Path) -> String {
        let file = OpenOptions::new()
            .access_mode(0x20080)
            .share_mode(7)
            .custom_flags(DIRECTORY_FLAGS)
            .open(path)
            .expect("read security handle");
        let sd = wrappers::GetSecurityInfo(
            &file,
            SeObjectType::SE_FILE_OBJECT,
            SecurityInformation::Owner | SecurityInformation::Dacl,
        )
        .expect("descriptor");
        wrappers::ConvertSecurityDescriptorToStringSecurityDescriptor(
            &sd,
            SecurityInformation::Owner | SecurityInformation::Dacl,
        )
        .expect("sddl")
        .to_string_lossy()
        .into_owned()
    }

    #[test]
    fn junction_ancestor_refusal_never_changes_target_acl_or_creates_child() {
        let temp = tempfile::tempdir().expect("temp");
        let target = temp.path().join("unrelated");
        let junction = temp.path().join("junction");
        std::fs::create_dir(&target).expect("target");
        let before = dacl(&target);
        let system = std::env::var_os("SystemRoot").expect("Windows system root");
        // Components, not "System32/cmd.exe": cmd.exe misbehaves on a mixed-slash path.
        let status =
            std::process::Command::new(Path::new(&system).join("System32").join("cmd.exe"))
                .args(["/d", "/c", "mklink", "/J"])
                .arg(&junction)
                .arg(&target)
                .status()
                .expect("create test junction");
        assert!(status.success(), "junction fixture setup must succeed");
        let error = ensure(&junction.join("child")).expect_err("refuse ancestor junction");
        assert!(error.to_string().contains("reparse"), "{error}");
        assert_eq!(dacl(&target), before);
        assert!(!target.join("child").exists());
        std::fs::remove_dir(&junction).expect("remove junction only");
    }

    #[test]
    fn unsupported_path_names_never_create_state() {
        for path in [r"relative\host", r"\\server\share\host", r"\\?\C:\host"] {
            assert!(ensure(Path::new(path)).is_err(), "{path}");
        }
    }
}
