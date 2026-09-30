//! First-frame window reveal helpers, and bringing the window back.

use tauri::{AppHandle, Manager, Runtime};

/// Unminimise, show and focus the main window.
///
/// Lived in `tray_menu` while the tray was this app's, for want of another
/// macOS-native home. Its callers are the Dock icon (`RunEvent::Reopen`) and
/// a clicked notification — both about *this window*, which is why it moved
/// here rather than to the menu bar app.
pub(crate) fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if let Err(error) = window.unminimize() {
        eprintln!("buzz-desktop: failed to restore the main window: {error}");
        return;
    }
    if let Err(error) = window.show() {
        eprintln!("buzz-desktop: failed to show the main window: {error}");
        return;
    }
    if let Err(error) = window.set_focus() {
        eprintln!("buzz-desktop: failed to focus the main window: {error}");
    }
}

#[cfg(target_os = "macos")]
pub(crate) const INITIAL_RENDER_READY_EVENT: &str = "initial-render-ready";

pub(crate) fn reveal_initial_window<R: tauri::Runtime>(window: &tauri::Window<R>) {
    if let Err(error) = window.show() {
        eprintln!("buzz-desktop: failed to reveal main window: {error}");
        return;
    }
    if let Err(error) = window.set_focus() {
        eprintln!("buzz-desktop: failed to focus main window: {error}");
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn set_initial_window_backing<R: tauri::Runtime>(window: &tauri::Window<R>) {
    // Both this write and the deferred clear target the Window (NSWindow)
    // backing color only; they never touch the webview canvas or the
    // NSVisualEffectView, so they are not load-bearing for glass. Glass state
    // — the effect view and webview-canvas transparency — is managed entirely
    // by `set_window_vibrancy`, which the ThemeProvider calls after mount. The
    // 250ms-delayed clear cannot clobber a persisted-glass-on cold boot
    // regardless of ordering with that call.
    //
    // Write an opaque dark backing so the previous app cannot show through
    // before WebKit submits its first composited surface.
    if let Err(error) = window.set_background_color(Some(tauri::window::Color(17, 21, 24, 255))) {
        eprintln!("buzz-desktop: failed to set initial window backing: {error}");
    }
}

#[cfg(target_os = "macos")]
pub(crate) async fn clear_initial_window_backing<R: tauri::Runtime>(window: &tauri::Window<R>) {
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    // Restore the default system window background so fast-resize gutter
    // flashes match the platform theme rather than the hardcoded dark color
    // written at reveal. Targets the Window (NSWindow) layer only; webview
    // canvas and glass state are unaffected.
    if let Err(error) = window.set_background_color(None) {
        eprintln!("buzz-desktop: failed to clear initial window backing: {error}");
    }
}

#[cfg(target_os = "macos")]
pub(crate) async fn wait_for_stable_initial_window_geometry<R: tauri::Runtime>(
    window: &tauri::Window<R>,
) {
    const MAX_POLLS: usize = 120;
    const REQUIRED_STABLE_POLLS: usize = 4;

    let mut previous_bounds = None;
    let mut stable_polls = 0;

    for _ in 0..MAX_POLLS {
        // Accept whatever geometry the window-state plugin restores — maximized
        // or a normal saved size. macOS applies the restore asynchronously, so
        // consecutive identical outer bounds are enough to know it settled.
        let bounds = match (window.outer_position(), window.outer_size()) {
            (Ok(position), Ok(size)) => Some((position.x, position.y, size.width, size.height)),
            _ => None,
        };

        if bounds.is_some() && bounds == previous_bounds {
            stable_polls += 1;
            if stable_polls >= REQUIRED_STABLE_POLLS {
                return;
            }
        } else {
            stable_polls = 0;
        }
        previous_bounds = bounds;

        tokio::time::sleep(std::time::Duration::from_millis(16)).await;
    }

    eprintln!("buzz-desktop: initial window geometry did not settle before reveal timeout");
}
