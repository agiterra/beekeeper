//! The child environment — re-exported from the shared launcher contract.
//!
//! The app no longer builds this map: `buzz-host` does, from
//! `buzz_session_host_core::env`, which is why the definitions moved there.
//! What the app still needs from it is the *name* of one file inside the
//! provider's state directory, because several app modules write into that
//! directory even though they no longer start anything.
//!
//! The module stays rather than being deleted so call sites keep reading the
//! same, and so the next person looking for "where the desktop builds the
//! provider env" finds this pointer instead of concluding it was lost.

pub(crate) use buzz_session_host_core::env::PROJECTS_FILE_NAME;
