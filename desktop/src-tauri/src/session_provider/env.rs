//! The child environment — re-exported from the shared launcher contract.
//!
//! The definitions live in `buzz_session_host_core::env` because the desktop
//! app is no longer the only launcher: `buzz-host` assembles the same
//! environment for the same provider. This module stays as the desktop's name
//! for them, so call sites read the same as they always did.
//!
//! See that module for the two properties its tests assert and the spawn code
//! cannot show: `BUZZ_AUTH_TAG` is a JSON array of strings, and the owner's
//! secret key appears nowhere in the map.

pub(crate) use buzz_session_host_core::env::{
    app_checkout_dir, build_provider_env, ProviderEnvInputs, EMIT_RAW_SDK_FRAMES_VAR,
    INHERITED_KEYS_TO_CLEAR, PROJECTS_FILE_NAME,
};
