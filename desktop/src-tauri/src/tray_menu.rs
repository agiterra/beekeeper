//! Native system-tray menu for the desktop app.
//!
//! The webview owns the live agent-turn state. It sends the small display
//! projection here so the native menu can remain useful while Beekeeper is
//! hidden.
//!
//! The rows, labels and the native subtitle pass live in `beekeeper-tray`,
//! shared with the standalone menu bar app: two processes drawing the same
//! machine two different ways would drift, and a menu bar is exactly where
//! nobody would notice.

// Mouse back/forward (X1/X2 buttons and swipe) is also macOS-only native I/O;
// group it here so both platform-layer init paths share one call site in lib.rs.
#[path = "mouse_nav.rs"]
pub(crate) mod mouse_nav;

use std::{
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use beekeeper_tray::{
    agent_item_label, ambiguous_agent_names, apply_subtitles, build_menu, channel_id_from_item_id,
    format_elapsed, tray_hat_icon, MenuRow, MenuSections, TrayActivityMenuItem, TrayAgentActivity,
};
use serde::{Deserialize, Serialize};
use tauri::{tray::TrayIconBuilder, AppHandle, Emitter, Manager, Runtime};

const TRAY_ID: &str = "buzz-tray";
const OPEN_BUZZ_ID: &str = "tray-open-buzz";
const NEW_CHANNEL_ID: &str = "tray-new-channel";
const QUIT_ID: &str = "tray-quit";

static PREVIEW_STARTED_AT: OnceLock<Instant> = OnceLock::new();

/// A local-only menu preview for demonstrating the working-agent section
/// without connecting to a relay. It is deliberately unavailable in release
/// builds and must be explicitly enabled when launching the debug app.
fn preview_activities() -> Option<Vec<TrayAgentActivity>> {
    if !cfg!(debug_assertions) || std::env::var("BUZZ_TRAY_MENU_DEMO").ok().as_deref() != Some("1")
    {
        return None;
    }

    let preview_elapsed = PREVIEW_STARTED_AT.get_or_init(Instant::now).elapsed();

    Some(vec![
        TrayAgentActivity {
            activity_id: "tray-preview-planning-scout".into(),
            agent_name: "Scout".into(),
            agent_pubkey: "11111111".repeat(8),
            channel_id: "tray-preview-planning".into(),
            channel_name: "planning".into(),
            elapsed: format_elapsed(Duration::from_secs(192) + preview_elapsed),
        },
        TrayAgentActivity {
            activity_id: "tray-preview-planning-builder".into(),
            agent_name: "Builder".into(),
            agent_pubkey: "22222222".repeat(8),
            channel_id: "tray-preview-planning".into(),
            channel_name: "planning".into(),
            elapsed: format_elapsed(Duration::from_secs(68) + preview_elapsed),
        },
        TrayAgentActivity {
            activity_id: "tray-preview-mobile-reviewer".into(),
            agent_name: "Reviewer".into(),
            agent_pubkey: "33333333".repeat(8),
            channel_id: "tray-preview-mobile".into(),
            channel_name: "mobile".into(),
            elapsed: format_elapsed(Duration::from_secs(31) + preview_elapsed),
        },
    ])
}

fn preview_recent_activities() -> Option<Vec<TrayAgentActivity>> {
    if !cfg!(debug_assertions) || std::env::var("BUZZ_TRAY_MENU_DEMO").ok().as_deref() != Some("1")
    {
        return None;
    }

    Some(vec![TrayAgentActivity {
        activity_id: "recent:tray-preview-design-architect".into(),
        agent_name: "Architect".into(),
        agent_pubkey: "44444444".repeat(8),
        channel_id: "tray-preview-design".into(),
        channel_name: "design".into(),
        elapsed: "4m 25s".into(),
    }])
}

/// This app's menu: no status header (the app itself is the status), and a
/// tail that acts on *this app* — `Quit Beekeeper` ends the desktop, which is
/// a different thing from ending the machine's agents.
fn desktop_sections() -> MenuSections<'static> {
    MenuSections {
        header: None,
        running_heading: Some("Running"),
        empty_label: Some("No agents are running"),
        recent_heading: Some("Recent"),
    }
}

const DESKTOP_TRAILING: &[MenuRow<'static>] = &[
    MenuRow::Separator,
    MenuRow::Action {
        id: NEW_CHANNEL_ID,
        label: "New Channel",
    },
    MenuRow::Separator,
    MenuRow::Action {
        id: OPEN_BUZZ_ID,
        label: "Open Beekeeper",
    },
    MenuRow::Separator,
    MenuRow::Action {
        id: QUIT_ID,
        label: "Quit Beekeeper",
    },
];

struct TrayActionQueue {
    community_generation: u64,
    pending_actions: Vec<TrayAction>,
}

struct TrayMenuState<R: Runtime> {
    activity_items: Mutex<Vec<TrayActivityMenuItem<R>>>,
    action_queue: Mutex<TrayActionQueue>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum TrayAction {
    NewChannel,
    OpenChannel {
        #[serde(rename = "channelId")]
        channel_id: String,
        #[serde(rename = "communityGeneration")]
        community_generation: u64,
    },
}

pub(crate) fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if let Err(error) = window.unminimize() {
        eprintln!("buzz-desktop: failed to restore main window from tray: {error}");
        return;
    }
    if let Err(error) = window.show() {
        eprintln!("buzz-desktop: failed to show main window from tray: {error}");
        return;
    }
    if let Err(error) = window.set_focus() {
        eprintln!("buzz-desktop: failed to focus main window from tray: {error}");
    }
}

fn queue_tray_action<R: Runtime>(app: &AppHandle<R>, mut action: TrayAction) {
    let state = app.state::<TrayMenuState<R>>();
    let Ok(mut queue) = state.action_queue.lock() else {
        eprintln!("buzz-desktop: tray action queue is unavailable");
        return;
    };
    if let TrayAction::OpenChannel {
        community_generation,
        ..
    } = &mut action
    {
        *community_generation = queue.community_generation;
    }
    queue.pending_actions.push(action);
    drop(queue);

    if let Err(error) = app.emit("tray-action-available", ()) {
        eprintln!("buzz-desktop: failed to notify frontend of tray action: {error}");
    }
}

fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, id: &str) {
    match id {
        OPEN_BUZZ_ID => show_main_window(app),
        NEW_CHANNEL_ID => {
            show_main_window(app);
            queue_tray_action(app, TrayAction::NewChannel);
        }
        QUIT_ID => app.exit(0),
        _ => {
            let Some(channel_id) = channel_id_from_item_id(id) else {
                return;
            };
            show_main_window(app);
            queue_tray_action(
                app,
                TrayAction::OpenChannel {
                    channel_id: channel_id.into(),
                    community_generation: 0,
                },
            );
        }
    }
}

/// Installs the persistent Buzz tray icon with the initial empty activity menu.
pub fn init<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let preview_activities = preview_activities();
    let preview_recent_activities = preview_recent_activities();
    let activities = preview_activities.as_deref().unwrap_or(&[]);
    let recent_activities = preview_recent_activities.as_deref().unwrap_or(&[]);
    let built = build_menu(
        app,
        &desktop_sections(),
        activities,
        recent_activities,
        DESKTOP_TRAILING,
    )?;
    app.manage(TrayMenuState {
        activity_items: Mutex::new(built.activity_items),
        action_queue: Mutex::new(TrayActionQueue {
            community_generation: 0,
            pending_actions: Vec::new(),
        }),
    });
    let tray = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&built.menu)
        .icon(tray_hat_icon())
        .icon_as_template(true)
        .on_menu_event(|app, event| handle_menu_event(app, event.id.as_ref()))
        .build(app)?;
    if let Err(error) = apply_subtitles(&tray, &built.subtitle_targets) {
        eprintln!("buzz-desktop: failed to apply tray menu presentation: {error}");
    }
    mouse_nav::init(app);
    Ok(())
}

/// Drains actions selected from the tray while the frontend was unavailable.
#[tauri::command]
pub fn take_tray_actions<R: Runtime>(app: AppHandle<R>) -> Result<Vec<TrayAction>, String> {
    let state = app.state::<TrayMenuState<R>>();
    let mut queue = state
        .action_queue
        .lock()
        .map_err(|_| "Beekeeper tray action queue is unavailable".to_string())?;
    Ok(std::mem::take(&mut queue.pending_actions))
}

fn requeue_actions(queue: &mut TrayActionQueue, mut actions: Vec<TrayAction>) {
    actions.retain(|action| match action {
        TrayAction::NewChannel => true,
        TrayAction::OpenChannel {
            community_generation,
            ..
        } => *community_generation == queue.community_generation,
    });
    actions.append(&mut queue.pending_actions);
    queue.pending_actions = actions;
}

/// Restores actions that were drained as the frontend unmounted. Channel
/// actions from a previous community generation are discarded.
#[tauri::command]
pub fn requeue_tray_actions<R: Runtime>(
    app: AppHandle<R>,
    actions: Vec<TrayAction>,
) -> Result<(), String> {
    let state = app.state::<TrayMenuState<R>>();
    let mut queue = state
        .action_queue
        .lock()
        .map_err(|_| "Beekeeper tray action queue is unavailable".to_string())?;
    requeue_actions(&mut queue, actions);
    drop(queue);
    app.emit("tray-action-available", ())
        .map_err(|error| error.to_string())
}

/// Clears community-scoped agent activity and queued channel navigation from
/// the native tray menu.
#[tauri::command]
pub fn clear_tray_agent_activity<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let state = app.state::<TrayMenuState<R>>();
    let mut queue = state
        .action_queue
        .lock()
        .map_err(|_| "Beekeeper tray action queue is unavailable".to_string())?;
    queue.community_generation = queue.community_generation.wrapping_add(1);
    queue
        .pending_actions
        .retain(|action| matches!(action, TrayAction::NewChannel));
    drop(queue);

    update_tray_agent_activity(app, Vec::new(), Vec::new())
}

/// Replaces the native menu's activity section with the current live work.
#[tauri::command]
pub fn update_tray_agent_activity<R: Runtime>(
    app: AppHandle<R>,
    activities: Vec<TrayAgentActivity>,
    recent_activities: Vec<TrayAgentActivity>,
) -> Result<(), String> {
    let preview_activities = preview_activities();
    let preview_recent_activities = preview_recent_activities();
    let activities = preview_activities.as_deref().unwrap_or(&activities);
    let recent_activities = preview_recent_activities
        .as_deref()
        .unwrap_or(&recent_activities);
    let state = app.state::<TrayMenuState<R>>();
    let mut activity_items = state
        .activity_items
        .lock()
        .map_err(|_| "Beekeeper tray menu state is unavailable".to_string())?;

    if activity_items.len() == activities.len().saturating_add(recent_activities.len())
        && activity_items
            .iter()
            .zip(activities.iter().chain(recent_activities))
            .all(|(item, activity)| {
                item.activity_id == activity.activity_id && item.channel_id == activity.channel_id
            })
    {
        let ambiguous = ambiguous_agent_names(activities.iter().chain(recent_activities));
        for (item, activity) in activity_items
            .iter()
            .zip(activities.iter().chain(recent_activities))
        {
            item.agent_item
                .set_text(agent_item_label(activity, &ambiguous))
                .map_err(|error| error.to_string())?;
        }
        // The rows did not change, so neither did the layout: the subtitles
        // already sitting on these items are still the right ones, and
        // re-deriving positions here is what the shared builder exists to stop.
        return Ok(());
    }

    let built = build_menu(
        &app,
        &desktop_sections(),
        activities,
        recent_activities,
        DESKTOP_TRAILING,
    )
    .map_err(|error| error.to_string())?;
    let tray = app
        .tray_by_id(TRAY_ID)
        .ok_or_else(|| "Beekeeper tray icon is not available".to_string())?;
    let subtitle_targets = built.subtitle_targets;
    tray.set_menu(Some(built.menu))
        .map_err(|error| error.to_string())?;
    apply_subtitles(&tray, &subtitle_targets)?;
    *activity_items = built.activity_items;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{requeue_actions, TrayAction, TrayActionQueue};

    #[test]
    fn open_channel_action_serializes_with_frontend_field_names() {
        let action = TrayAction::OpenChannel {
            channel_id: "channel-123".into(),
            community_generation: 7,
        };

        assert_eq!(
            serde_json::to_value(action).expect("tray action should serialize"),
            serde_json::json!({
                "kind": "openChannel",
                "channelId": "channel-123",
                "communityGeneration": 7,
            })
        );
    }

    #[test]
    fn stale_channel_actions_are_not_requeued_after_community_change() {
        let mut queue = TrayActionQueue {
            community_generation: 2,
            pending_actions: Vec::new(),
        };

        requeue_actions(
            &mut queue,
            vec![TrayAction::OpenChannel {
                channel_id: "old-channel".into(),
                community_generation: 1,
            }],
        );

        assert!(queue.pending_actions.is_empty());
    }

    #[test]
    fn new_channel_actions_survive_community_change() {
        let mut queue = TrayActionQueue {
            community_generation: 2,
            pending_actions: Vec::new(),
        };

        requeue_actions(&mut queue, vec![TrayAction::NewChannel]);

        assert_eq!(queue.pending_actions, vec![TrayAction::NewChannel]);
    }

    // The label and ambiguity assertions moved to `beekeeper-tray` with the
    // functions they cover. What stays here is this app's own: the action
    // queue's wire shape, and what survives a community change.
}
