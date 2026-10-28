//! Ledger 393(b): where the nest — and so every new workspace — lives.
use super::*;

/// Ledger 393(b): the workspace root follows the instance's nest. An explicit
/// absolute override wins over HOME, a relative one refuses rather than
/// falling back into another scope, and the defaults keep Dev and production
/// apart.
#[test]
fn nest_root_honours_an_absolute_override_and_keeps_instances_apart() {
    let home = Some(PathBuf::from("/Users/someone"));
    assert_eq!(
        resolve_nest_root(Some("/tmp/isolated/nest".into()), true, home.clone()),
        Some(PathBuf::from("/tmp/isolated/nest"))
    );
    assert_eq!(
        resolve_nest_root(Some("/tmp/isolated/nest".into()), false, home.clone()),
        Some(PathBuf::from("/tmp/isolated/nest"))
    );
    assert_eq!(
        resolve_nest_root(Some("relative/nest".into()), true, home.clone()),
        None
    );
    assert_eq!(
        resolve_nest_root(None, true, home.clone()),
        Some(PathBuf::from("/Users/someone/.beekeeper-dev"))
    );
    assert_eq!(
        resolve_nest_root(None, false, home),
        Some(PathBuf::from("/Users/someone/.beekeeper"))
    );
    assert_eq!(resolve_nest_root(None, true, None), None);
}
