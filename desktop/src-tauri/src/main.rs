// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // First, while the process is single threaded: read any legacy `BUZZ_*`
    // variable (an operator's shell, an old launch agent) as its
    // `BEEKEEPER_*` name, so the rest of the app reads one spelling.
    beekeeper_core_pkg::env_compat::adopt_legacy_env("beekeeper-desktop");

    if beekeeper_lib::print_agent_access_owner_only_probe_if_requested() {
        return;
    }

    // Before anything else: WebKitGTK reads its rendering environment once at
    // process start, and this is the only point where the process is still
    // single threaded and no GTK object exists yet, which is what makes
    // `std::env::set_var` sound.
    #[cfg(target_os = "linux")]
    if let Err(diagnostic) = beekeeper_lib::webkit_rendering::apply() {
        eprintln!("beekeeper-desktop: {diagnostic}");
        std::process::exit(1);
    }

    beekeeper_lib::run()
}
