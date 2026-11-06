//! Compatibility entry points: Windows foreground Host has no startup installer.
use super::Registration;
use beekeeper_host_core::layout::Instance;
use std::path::{Path, PathBuf};
/// Placeholder namespace; never read or written on Windows.
pub const DAEMON_DIR: &str = "unsupported-launchd";
/// Refuse a platform-specific startup action.
pub fn supported() -> Result<(), String> {
    Err("Windows startup registration is unsupported; use beekeeper-host run in a terminal".into())
}
/// No launchd accounts exist on Windows.
pub fn validate_user_name(_name: &str) -> Result<(), String> {
    supported()
}
pub(crate) fn current_user_name() -> Option<String> {
    None
}
/// No launchd label exists on Windows.
pub fn label(_user: &str, _instance: Instance) -> String {
    "unsupported".into()
}
/// Inert path for shared registration diagnostics.
pub fn plist_path(dir: &Path, _user: &str, _instance: Instance) -> PathBuf {
    dir.join("unsupported")
}
/// Startup loading cannot be observed on this platform.
pub fn loaded(_user: &str, _instance: Instance) -> Option<bool> {
    None
}
pub(crate) fn registration(_dir: &Path, _user: &str, _instance: Instance) -> Option<Registration> {
    None
}
/// The foreground alternative to registration.
pub fn rerun_command() -> String {
    "beekeeper-host run".into()
}
/// Refuse system installation.
pub fn install(
    _user: &str,
    _instance: Instance,
    _program: &Path,
    _rerun: &str,
) -> Result<Registration, String> {
    supported()?;
    Err("unsupported".into())
}
/// Refuse system uninstallation.
pub fn uninstall(_user: &str, _instance: Instance, _rerun: &str) -> Result<(), String> {
    supported()
}
/// Refuse generating a launchd file.
pub fn render(_user: &str, _instance: Instance, _program: &Path) -> Result<String, String> {
    supported()?;
    Err("unsupported".into())
}
