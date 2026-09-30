//! `beekeeper-menubar` — see [`beekeeper_menubar_lib`].

// No console window on Windows, for consistency with the desktop app. This
// app does nothing on Windows, but it does it without a window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    beekeeper_menubar_lib::run();
}
