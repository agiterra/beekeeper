//! The Beekeeper tray menu: its rows, their labels, and the native subtitle
//! pass that decorates them.
//!
//! Two processes draw this menu. The desktop app has drawn it from live
//! webview state since it existed; the standalone menu bar app draws it from
//! the agent host's control socket, so the menu keeps working — and keeps
//! telling the truth — while no desktop app is running. Sharing the rows is
//! the point: two renderings of the same machine that could disagree would
//! disagree eventually, and a menu bar is exactly where nobody would notice.
//!
//! # Why `build_menu` returns its subtitle targets
//!
//! macOS 14+ can give a menu item a subtitle, which is how a row shows its
//! channel under the agent's name. The `NSMenuItem`s are reached by *index*,
//! and the code that applied them used to recompute those indices by counting
//! rows:
//!
//! ```text
//! let mut item_index = 1;
//! if running_count == 0 { item_index += 1 }
//! item_index += 2;  // separator and Recent heading
//! ```
//!
//! That is a second, implicit copy of this module's layout, kept in step with
//! the first by hand. Adding one row at the top of the menu — a status header,
//! say — silently shifts every index, and the symptom is subtitles landing on
//! the wrong rows or on separators. Nothing fails; the menu is just wrong.
//!
//! So [`build_menu`] records each index *as it appends that row*, and
//! [`apply_subtitles`] does nothing but set what it was handed. There is one
//! layout, and it is the code that builds it.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::{AppHandle, Runtime};

#[cfg(target_os = "macos")]
use objc2::MainThreadMarker;
#[cfg(target_os = "macos")]
use objc2_foundation::{NSProcessInfo, NSString};

/// Menu-item id prefix for "open this activity's channel".
pub const OPEN_CHANNEL_PREFIX: &str = "tray-open-channel:";
/// Separates the channel id from the activity id inside that item id.
pub const OPEN_CHANNEL_ACTIVITY_SEPARATOR: char = '|';
/// How wide the native menu is asked to be, so a subtitle has room.
pub const TRAY_MENU_MINIMUM_WIDTH: f64 = 320.0;

/// A running agent and the channel it is working in.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayAgentActivity {
    pub activity_id: String,
    pub agent_name: String,
    /// The agent's identity. Carried because the tray is a machine-wide list
    /// and agent names are unique per project, not per computer (ledger 246):
    /// two projects may each have a `Builder`, and the name alone would render
    /// two rows a person cannot tell apart. `#[serde(default)]` so an older
    /// sender that does not supply it still renders (unambiguously or not).
    #[serde(default)]
    pub agent_pubkey: String,
    pub channel_id: String,
    pub channel_name: String,
    /// How long this turn has been running, already formatted.
    ///
    /// A *formatted* string because whoever supplies the rows re-labels them
    /// from its own clock — see [`format_elapsed`]. The agent host deliberately
    /// does not send one of these: it sends an absolute start time, because a
    /// formatted duration is only true at the instant it was written.
    pub elapsed: String,
}

/// `12s`, `3m 12s`, `1h 04m 09s`.
pub fn format_elapsed(elapsed: Duration) -> String {
    let total_seconds = elapsed.as_secs();
    if total_seconds < 60 {
        return format!("{total_seconds}s");
    }

    let seconds = total_seconds % 60;
    let total_minutes = total_seconds / 60;
    if total_minutes < 60 {
        return format!("{total_minutes}m {seconds}s");
    }

    let minutes = total_minutes % 60;
    let hours = total_minutes / 60;
    format!("{hours}h {minutes}m {seconds}s")
}

/// The standalone Beekeeper hat, as a transparent macOS template image.
///
/// The app icon includes a rounded square, which is useful for the Dock but
/// looks out of place beside the monochrome menu-bar icons. Template images
/// render from the alpha channel alone, so the embedded artifact is a raw
/// alpha mask — the hat silhouette, regenerated from the source drawing by
/// `desktop/scripts/make-tray-alpha.swift` — and macOS tints it correctly in
/// light and dark menu bars without a separate bitmap per theme.
pub fn tray_hat_icon() -> Image<'static> {
    const WIDTH: u32 = 37;
    const HEIGHT: u32 = 43;
    const ALPHA: &[u8] = include_bytes!("../assets/tray-hat-alpha.bin");
    const _: () = assert!(ALPHA.len() == (WIDTH * HEIGHT) as usize);

    let mut rgba = vec![0; (WIDTH * HEIGHT * 4) as usize];
    for (index, &alpha) in ALPHA.iter().enumerate() {
        rgba[index * 4 + 3] = alpha;
    }
    Image::new_owned(rgba, WIDTH, HEIGHT)
}

/// `abcd1234…` — enough to tell two same-named agents apart at a glance,
/// the same shape the mention autocomplete uses for the same job.
pub fn short_pubkey(pubkey: &str) -> String {
    let head: String = pubkey.chars().take(8).collect();
    format!("{head}\u{2026}")
}

/// Names carried by more than one activity in this menu, folded.
///
/// The tray spans every project and channel on this computer, and names are
/// unique per project rather than per computer, so two rows really can both
/// say `Builder`. Only the ones that actually collide are disambiguated — the
/// same rule the mention autocomplete uses ("name collisions are the
/// impersonation vector"), and for the same reason.
pub fn ambiguous_agent_names<'a>(
    activities: impl Iterator<Item = &'a TrayAgentActivity>,
) -> HashSet<String> {
    let mut seen: HashMap<String, &str> = HashMap::new();
    let mut ambiguous = HashSet::new();
    for activity in activities {
        let folded = activity.agent_name.trim().to_lowercase();
        match seen.get(&folded) {
            Some(pubkey) if *pubkey != activity.agent_pubkey => {
                ambiguous.insert(folded);
            }
            Some(_) => {}
            None => {
                seen.insert(folded, activity.agent_pubkey.as_str());
            }
        }
    }
    ambiguous
}

/// The visible text of one activity row.
pub fn agent_item_label(activity: &TrayAgentActivity, ambiguous: &HashSet<String>) -> String {
    let name = if ambiguous.contains(&activity.agent_name.trim().to_lowercase())
        && !activity.agent_pubkey.is_empty()
    {
        format!(
            "{} ({})",
            activity.agent_name,
            short_pubkey(&activity.agent_pubkey)
        )
    } else {
        activity.agent_name.clone()
    };
    let primary = format!("{name} · {}", activity.elapsed);

    // Without subtitles the channel has to go in the label, or the row does
    // not say where the work is happening.
    if supports_menu_item_subtitles() {
        primary
    } else {
        format!("{primary} — #{}", activity.channel_name)
    }
}

/// Whether this OS can give a menu item a subtitle (macOS 14+).
pub fn supports_menu_item_subtitles() -> bool {
    #[cfg(target_os = "macos")]
    {
        use std::sync::OnceLock;
        static SUPPORTS_SUBTITLES: OnceLock<bool> = OnceLock::new();
        *SUPPORTS_SUBTITLES.get_or_init(|| {
            NSProcessInfo::processInfo()
                .operatingSystemVersion()
                .majorVersion
                >= 14
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// The menu-item id that opens an activity's channel.
pub fn channel_item_id(activity: &TrayAgentActivity) -> String {
    format!(
        "{OPEN_CHANNEL_PREFIX}{}{OPEN_CHANNEL_ACTIVITY_SEPARATOR}{}",
        activity.channel_id, activity.activity_id
    )
}

/// The channel id inside an id built by [`channel_item_id`], if it is one.
pub fn channel_id_from_item_id(id: &str) -> Option<&str> {
    let rest = id.strip_prefix(OPEN_CHANNEL_PREFIX)?;
    Some(
        rest.split_once(OPEN_CHANNEL_ACTIVITY_SEPARATOR)
            .map(|(channel_id, _)| channel_id)
            .unwrap_or(rest),
    )
}

/// One row below the activity sections, as the embedder wants it.
///
/// Passed in rather than hard-coded because the two apps genuinely differ
/// here: the desktop app's menu quits the desktop app, and the menu bar app's
/// menu must not — "Quit" meaning "end every agent on this machine" is the
/// conflation this whole split exists to remove.
#[derive(Debug, Clone)]
pub enum MenuRow<'a> {
    Separator,
    /// A clickable row with a stable id.
    Action {
        id: &'a str,
        label: &'a str,
    },
    /// A row that shows something and does nothing.
    Disabled(&'a str),
}

/// One activity row's menu item, kept so its text can be updated in place.
pub struct TrayActivityMenuItem<R: Runtime> {
    pub activity_id: String,
    pub channel_id: String,
    pub agent_item: MenuItem<R>,
}

/// What [`build_menu`] produced.
pub struct BuiltMenu<R: Runtime> {
    pub menu: Menu<R>,
    pub activity_items: Vec<TrayActivityMenuItem<R>>,
    /// `(menu index, subtitle)` for every row that should carry one, recorded
    /// by the code that appended the row. **The only permitted input to
    /// [`apply_subtitles`].** See the module docs.
    pub subtitle_targets: Vec<(isize, String)>,
}

/// The sections above the trailing rows.
#[derive(Debug, Clone, Default)]
pub struct MenuSections<'a> {
    /// A disabled row at the very top — the agent host's state, for the menu
    /// bar app. `None` reproduces a menu with no header.
    pub header: Option<&'a str>,
    /// Heading above the running rows. `None` omits it.
    pub running_heading: Option<&'a str>,
    /// Shown when there are no running rows.
    pub empty_label: Option<&'a str>,
    /// Heading above the recent rows.
    pub recent_heading: Option<&'a str>,
}

/// One row of the menu, as planned before anything is appended.
///
/// The layout is computed as data first so it can be *tested*. `build_menu`
/// needs a real `AppHandle` to make a `Menu`, which is why the index
/// arithmetic this replaces had no test and could quietly go wrong: there was
/// nowhere to assert it from. A plan is assertable without a Tauri runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlannedRow {
    /// A row that shows something and does nothing.
    Disabled(String),
    Separator,
    /// An activity row. The payload indexes the running rows followed by the
    /// recent rows, concatenated — the order [`build_menu`] appends them in.
    Activity(usize),
    /// A clickable row with a stable id.
    Action {
        id: String,
        label: String,
    },
}

/// Lay out the menu for these counts, without building anything.
///
/// Every consumer of a row index — the subtitles, the tests — reads it from
/// here, so there is one layout rather than one layout and one copy of its
/// arithmetic.
pub fn plan_menu(
    sections: &MenuSections<'_>,
    running_count: usize,
    recent_count: usize,
    trailing: &[MenuRow<'_>],
) -> Vec<PlannedRow> {
    let mut rows = Vec::new();
    if let Some(header) = sections.header {
        rows.push(PlannedRow::Disabled(header.to_string()));
    }
    if let Some(heading) = sections.running_heading {
        rows.push(PlannedRow::Disabled(heading.to_string()));
    }
    if running_count == 0 {
        if let Some(empty) = sections.empty_label {
            rows.push(PlannedRow::Disabled(empty.to_string()));
        }
    } else {
        rows.extend((0..running_count).map(PlannedRow::Activity));
    }
    if recent_count > 0 {
        rows.push(PlannedRow::Separator);
        if let Some(heading) = sections.recent_heading {
            rows.push(PlannedRow::Disabled(heading.to_string()));
        }
        rows.extend((running_count..running_count + recent_count).map(PlannedRow::Activity));
    }
    for row in trailing {
        rows.push(match row {
            MenuRow::Separator => PlannedRow::Separator,
            MenuRow::Action { id, label } => PlannedRow::Action {
                id: (*id).to_string(),
                label: (*label).to_string(),
            },
            MenuRow::Disabled(label) => PlannedRow::Disabled((*label).to_string()),
        });
    }
    rows
}

/// Build the whole menu, recording where each subtitle belongs.
pub fn build_menu<R: Runtime>(
    app: &AppHandle<R>,
    sections: &MenuSections<'_>,
    activities: &[TrayAgentActivity],
    recent_activities: &[TrayAgentActivity],
    trailing: &[MenuRow<'_>],
) -> tauri::Result<BuiltMenu<R>> {
    let all: Vec<&TrayAgentActivity> = activities.iter().chain(recent_activities).collect();
    // Ambiguity is folded over *every* row in the menu, running and recent
    // together: two rows a person can see at once must be told apart whichever
    // section they are in.
    let ambiguous = ambiguous_agent_names(all.iter().copied());
    let plan = plan_menu(
        sections,
        activities.len(),
        recent_activities.len(),
        trailing,
    );

    let menu = Menu::new(app)?;
    let mut activity_items = Vec::with_capacity(all.len());
    let mut subtitle_targets = Vec::new();

    for (position, row) in plan.iter().enumerate() {
        // The index is the row's position in the plan. Nothing counts rows a
        // second time, which is the whole reason the plan exists.
        let index = position as isize;
        match row {
            PlannedRow::Separator => menu.append(&PredefinedMenuItem::separator(app)?)?,
            PlannedRow::Disabled(label) => {
                menu.append(&MenuItem::new(app, label, false, None::<&str>)?)?
            }
            PlannedRow::Action { id, label } => {
                menu.append(&MenuItem::with_id(app, id, label, true, None::<&str>)?)?
            }
            PlannedRow::Activity(which) => {
                let Some(activity) = all.get(*which) else {
                    // Unreachable: `plan_menu` was given these counts. Skipped
                    // rather than panicking, because a tray that draws one row
                    // short beats a desktop that will not start.
                    continue;
                };
                let agent_item = MenuItem::with_id(
                    app,
                    channel_item_id(activity),
                    agent_item_label(activity, &ambiguous),
                    true,
                    None::<&str>,
                )?;
                menu.append(&agent_item)?;
                subtitle_targets.push((index, format!("#{}", activity.channel_name)));
                activity_items.push(TrayActivityMenuItem {
                    activity_id: activity.activity_id.clone(),
                    channel_id: activity.channel_id.clone(),
                    agent_item,
                });
            }
        }
    }

    Ok(BuiltMenu {
        menu,
        activity_items,
        subtitle_targets,
    })
}

/// Set the subtitles [`build_menu`] recorded, and nothing else.
///
/// No arithmetic, no row counting: the pairs came from the builder. On a macOS
/// without subtitle support, or off macOS, this is a no-op.
#[cfg(target_os = "macos")]
pub fn apply_subtitles<R: Runtime>(
    tray: &tauri::tray::TrayIcon<R>,
    targets: &[(isize, String)],
) -> Result<(), String> {
    if !supports_menu_item_subtitles() {
        return Ok(());
    }
    let targets = targets.to_vec();
    tray.with_inner_tray_icon(move |inner| {
        let Some(status_item) = inner.ns_status_item() else {
            return;
        };
        // `NSStatusItem`/`NSMenu` mutation is main-thread-only.
        let Some(main_thread) = MainThreadMarker::new() else {
            return;
        };
        let Some(menu) = status_item.menu(main_thread) else {
            return;
        };
        menu.setMinimumWidth(TRAY_MENU_MINIMUM_WIDTH);
        for (index, subtitle) in &targets {
            if let Some(item) = menu.itemAtIndex(*index) {
                item.setSubtitle(Some(&NSString::from_str(subtitle)));
            }
        }
    })
    .map_err(|error| error.to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn apply_subtitles<R: Runtime>(
    _tray: &tauri::tray::TrayIcon<R>,
    _targets: &[(isize, String)],
) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
