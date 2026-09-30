//! The macOS menu bar itself: an accessory app with a status item and no
//! window.
//!
//! Everything that decides *what* the menu says is in [`crate::model`], and
//! everything that decides *when* to ask is in [`crate::poll`]. This module
//! only draws and dispatches.
//!
//! # Threading
//!
//! `NSStatusItem` and `NSMenu` mutation is main-thread-only. Polling is a
//! socket round trip with timeouts, so it runs on a worker and the result is
//! applied through `run_on_main_thread`. A hung host therefore renders as
//! "not responding" rather than freezing the menu — which is the failure a
//! menu bar app absolutely must not have, because there is nowhere for a
//! person to see that it is stuck.

use std::sync::Mutex;

use beekeeper_host_core::layout::{self, Instance};
use beekeeper_tray::{
    apply_subtitles, build_menu, channel_id_from_item_id, tray_hat_icon, MenuRow, MenuSections,
    TrayActivityMenuItem,
};
use tauri::menu::MenuId;
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Runtime};

use crate::model::{model, HostView, MenuModel};
use crate::poll::{self, RELABEL_INTERVAL};

const TRAY_ID: &str = "beekeeper-menubar";
const OPEN_APP_ID: &str = "menubar-open-beekeeper";
const OPEN_LOG_ID: &str = "menubar-open-log";
const RESTART_ID: &str = "menubar-restart-host";
const STOP_ID: &str = "menubar-stop-host";
const START_ID: &str = "menubar-start-host";
const QUIT_ID: &str = "menubar-quit";

/// What the menu was last drawn from, so a relabel can reuse it.
struct MenubarState<R: Runtime> {
    /// The last model drawn, and the items it produced.
    drawn: Mutex<Option<Drawn<R>>>,
    /// The last view the host gave us, so the 1 Hz relabel can recompute
    /// elapsed times without asking again.
    view: Mutex<Option<HostView>>,
    socket: std::path::PathBuf,
    home: std::path::PathBuf,
    instance: Instance,
    log_path: Mutex<Option<std::path::PathBuf>>,
}

struct Drawn<R: Runtime> {
    model: MenuModel,
    items: Vec<TrayActivityMenuItem<R>>,
}

/// The rows below the sections. Written out here because the wording is the
/// product: the desktop tray's "Quit Buzz" ended every agent on the machine,
/// and this app's Quit must say — and do — something different.
fn trailing(model: &MenuModel) -> Vec<MenuRow<'static>> {
    let mut rows = vec![
        MenuRow::Separator,
        MenuRow::Action {
            id: OPEN_APP_ID,
            label: "Open Beekeeper",
        },
    ];
    if model.host_reachable {
        rows.push(MenuRow::Action {
            id: OPEN_LOG_ID,
            label: "Open Agent Log…",
        });
        rows.push(MenuRow::Separator);
        if model.provider_live {
            rows.push(MenuRow::Action {
                id: RESTART_ID,
                label: "Restart Agents",
            });
            rows.push(MenuRow::Action {
                id: STOP_ID,
                label: "Stop Agents…",
            });
        } else {
            rows.push(MenuRow::Action {
                id: START_ID,
                label: "Start Agents",
            });
        }
    }
    rows.push(MenuRow::Separator);
    rows.push(MenuRow::Action {
        id: QUIT_ID,
        label: "Quit Menu Bar",
    });
    // Said on its own row rather than trusted to the label: "Quit" next to a
    // list of running agents reads as "stop them", and it does not.
    rows.push(MenuRow::Disabled(
        "Quitting this leaves your agents running",
    ));
    rows
}

fn sections(model: &MenuModel) -> MenuSections<'_> {
    MenuSections {
        header: Some(&model.header),
        running_heading: None,
        // Only when the host answered. Over an unreachable host the header
        // already says what is wrong, and "no agents are running" would be a
        // claim this process cannot make.
        empty_label: if model.host_reachable {
            Some("Nothing is working right now")
        } else {
            None
        },
        recent_heading: Some("Recent"),
    }
}

/// Draw the menu from a model, replacing whatever is there.
fn draw<R: Runtime>(app: &AppHandle<R>, next: MenuModel) -> Result<(), String> {
    let state = app.state::<MenubarState<R>>();
    let trailing_rows = trailing(&next);
    let mut notices: Vec<MenuRow<'_>> = Vec::new();
    for notice in &next.notices {
        notices.push(MenuRow::Disabled(notice));
    }
    let mut all_trailing = notices;
    all_trailing.extend(trailing_rows);

    let built = build_menu(
        app,
        &sections(&next),
        &next.running,
        &next.recent,
        &all_trailing,
    )
    .map_err(|error| error.to_string())?;
    let tray = app
        .tray_by_id(TRAY_ID)
        .ok_or_else(|| "the menu bar item is not available".to_string())?;
    let subtitle_targets = built.subtitle_targets;
    tray.set_menu(Some(built.menu))
        .map_err(|error| error.to_string())?;
    apply_subtitles(&tray, &subtitle_targets)?;

    if let Ok(mut drawn) = state.drawn.lock() {
        *drawn = Some(Drawn {
            model: next,
            items: built.activity_items,
        });
    }
    Ok(())
}

/// Update only the row text, when the rows themselves have not changed.
///
/// This is the 1 Hz path: the clock ticks without rebuilding a menu, and
/// without asking the host anything. Returns `false` when the rows differ, so
/// the caller rebuilds.
fn relabel<R: Runtime>(app: &AppHandle<R>, next: &MenuModel) -> bool {
    let state = app.state::<MenubarState<R>>();
    let Ok(drawn) = state.drawn.lock() else {
        return false;
    };
    let Some(drawn) = drawn.as_ref() else {
        return false;
    };
    // Same rows in the same order, and the same header and notices — anything
    // else is a layout change, and a layout change must go through the
    // builder so its subtitles come with it.
    if drawn.model.header != next.header
        || drawn.model.notices != next.notices
        || drawn.model.host_reachable != next.host_reachable
        || drawn.model.provider_live != next.provider_live
        || drawn.items.len() != next.running.len() + next.recent.len()
    {
        return false;
    }
    let all = next.running.iter().chain(next.recent.iter());
    if !drawn
        .items
        .iter()
        .zip(all.clone())
        .all(|(item, row)| item.activity_id == row.activity_id)
    {
        return false;
    }
    let ambiguous = beekeeper_tray::ambiguous_agent_names(next.running.iter().chain(&next.recent));
    for (item, row) in drawn.items.iter().zip(all) {
        if item
            .agent_item
            .set_text(beekeeper_tray::agent_item_label(row, &ambiguous))
            .is_err()
        {
            return false;
        }
    }
    true
}

/// Apply a fresh view: relabel if the rows are unchanged, else rebuild.
fn apply<R: Runtime>(app: &AppHandle<R>, view: HostView) {
    let state = app.state::<MenubarState<R>>();
    if let Ok(mut held) = state.view.lock() {
        *held = Some(view.clone());
    }
    if let HostView::Reachable(status) = &view {
        if let Ok(mut log) = state.log_path.lock() {
            *log = beekeeper_host_core::record::provider_log_path_in(
                status
                    .provider_state_dir
                    .parent()
                    .unwrap_or(&status.provider_state_dir),
                &status.provider_pubkey,
            )
            .ok();
        }
    }
    let next = model(&view, now_ms());
    if relabel(app, &next) {
        if let Ok(mut drawn) = state.drawn.lock() {
            if let Some(drawn) = drawn.as_mut() {
                drawn.model = next;
            }
        }
        return;
    }
    if let Err(error) = draw(app, next) {
        eprintln!("beekeeper-menubar: {error}");
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis() as i64)
        .unwrap_or(0)
}

fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, id: &MenuId) {
    let state = app.state::<MenubarState<R>>();
    match id.as_ref() {
        OPEN_APP_ID => open_beekeeper(None),
        OPEN_LOG_ID => {
            let path = state.log_path.lock().ok().and_then(|path| path.clone());
            match path {
                Some(path) => open_path(&path),
                // Named rather than silently doing nothing: a menu item that
                // does nothing when clicked is worse than one that says why.
                None => {
                    eprintln!("beekeeper-menubar: no agent log yet — the host has not reported one")
                }
            }
        }
        RESTART_ID => lifecycle(&state.socket, beekeeper_host::protocol::Request::Restart),
        STOP_ID => lifecycle(&state.socket, beekeeper_host::protocol::Request::Stop),
        START_ID => lifecycle(&state.socket, beekeeper_host::protocol::Request::Start),
        // Quits *this app*. The agents are the host's children and are not
        // touched — which the menu says out loud on its own row.
        QUIT_ID => app.exit(0),
        other => {
            if let Some(channel_id) = channel_id_from_item_id(other) {
                open_beekeeper(Some(channel_id));
            }
        }
    }
}

/// Open the desktop app, optionally on a channel, through its own deep link.
///
/// A deep link rather than anything cleverer: `beekeeper://` is already the
/// documented way in, the desktop validates the UUID itself, and this app
/// therefore needs no opinion about whether a channel still exists.
fn open_beekeeper(channel_id: Option<&str>) {
    let url = match channel_id {
        Some(channel_id) if !channel_id.is_empty() => {
            format!("beekeeper://channel?channel={channel_id}")
        }
        _ => "beekeeper://".to_string(),
    };
    open_url(&url);
}

fn open_url(url: &str) {
    if let Err(error) = std::process::Command::new("open").arg(url).status() {
        eprintln!("beekeeper-menubar: could not open {url}: {error}");
    }
}

fn open_path(path: &std::path::Path) {
    if let Err(error) = std::process::Command::new("open").arg(path).status() {
        eprintln!(
            "beekeeper-menubar: could not open {}: {error}",
            path.display()
        );
    }
}

/// Send one lifecycle request, off the main thread.
///
/// Fire-and-forget with a logged failure: these take seconds (a stop waits for
/// the provider's SIGINT window), and blocking the main thread for that would
/// freeze the menu bar for every app on the machine.
fn lifecycle(socket: &std::path::Path, request: beekeeper_host::protocol::Request) {
    let socket = socket.to_path_buf();
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                eprintln!("beekeeper-menubar: {error}");
                return;
            }
        };
        if let Err(error) = runtime.block_on(beekeeper_host::client::call(
            &socket,
            &request,
            beekeeper_host::client::LIFECYCLE_READ_TIMEOUT,
        )) {
            eprintln!("beekeeper-menubar: {}", error.message());
        }
    });
}

/// Build the status item and start the poll and relabel loops.
pub fn run() {
    let Ok(home) = layout::home_dir() else {
        eprintln!("beekeeper-menubar: cannot resolve the home directory");
        return;
    };
    let instance = Instance::from_env().unwrap_or(Instance::Production);
    let socket = layout::host_socket_path(&home, instance);

    let app = tauri::Builder::default()
        .setup(move |app| {
            let handle = app.handle().clone();
            // An accessory app: no Dock icon, no menu bar of its own, no
            // window. `tauri.conf.json` declares no windows either; this is
            // the half that matters when running unbundled from `cargo run`,
            // where there is no `Info.plist` to read `LSUIElement` from.
            #[cfg(target_os = "macos")]
            let _ = handle.set_activation_policy(tauri::ActivationPolicy::Accessory);

            handle.manage(MenubarState::<tauri::Wry> {
                drawn: Mutex::new(None),
                view: Mutex::new(None),
                socket: socket.clone(),
                home: home.clone(),
                instance,
                log_path: Mutex::new(None),
            });

            // Built with an empty menu first: the status item must appear
            // immediately, before the first poll answers, or a slow socket
            // looks like an app that failed to launch.
            let initial = model(
                &HostView::Unreachable {
                    installed: false,
                    reason: "asking the agent host…".to_string(),
                },
                now_ms(),
            );
            let built = build_menu(&handle, &sections(&initial), &[], &[], &trailing(&initial))?;
            TrayIconBuilder::with_id(TRAY_ID)
                .menu(&built.menu)
                .icon(tray_hat_icon())
                .icon_as_template(true)
                .on_menu_event(|app, event| handle_menu_event(app, &event.id))
                .build(&handle)?;

            spawn_loops(handle);
            Ok(())
        })
        .build(tauri::generate_context!());

    match app {
        Ok(app) => app.run(|_, _| {}),
        Err(error) => eprintln!("beekeeper-menubar: failed to start: {error}"),
    }
}

/// The poll loop and the relabel loop, both applying on the main thread.
fn spawn_loops(handle: AppHandle) {
    let poll_handle = handle.clone();
    std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            eprintln!("beekeeper-menubar: could not start the poll runtime");
            return;
        };
        let state = poll_handle.state::<MenubarState<tauri::Wry>>();
        let socket = state.socket.clone();
        let home = state.home.clone();
        let instance = state.instance;
        let mut failures = 0u32;
        loop {
            let view = runtime.block_on(poll::poll(&socket, &home, instance));
            let reachable = matches!(view, HostView::Reachable(_));
            failures = if reachable {
                0
            } else {
                failures.saturating_add(1)
            };
            let working = match &view {
                HostView::Reachable(status) => {
                    !status.sessions.sessions.is_empty() || !status.app_activity.is_empty()
                }
                HostView::Unreachable { .. } => false,
            };
            let applied = poll_handle.clone();
            let _ = poll_handle.run_on_main_thread(move || apply(&applied, view));
            std::thread::sleep(poll::next_interval(reachable, working, failures));
        }
    });

    // The clock. Relabels from the local clock using the view already held, so
    // a running turn counts up without the host being asked once a second.
    std::thread::spawn(move || loop {
        std::thread::sleep(RELABEL_INTERVAL);
        let ticked = handle.clone();
        let _ = handle.run_on_main_thread(move || {
            let state = ticked.state::<MenubarState<tauri::Wry>>();
            let held = state.view.lock().ok().and_then(|view| view.clone());
            if let Some(view) = held {
                apply(&ticked, view);
            }
        });
    });
}
