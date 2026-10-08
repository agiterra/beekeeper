//! Slot rectangle → native view bounds.
//!
//! The React slot reports its DOM client rect in CSS pixels of the hosting
//! webview. The app pins the native webview zoom to 1 (Cmd +/- scales the
//! root font size instead, `app/useWebviewZoomShortcuts.ts`), and the main
//! webview fills the window's content view under `titleBarStyle: Overlay`,
//! so one CSS pixel is one logical point of the content view with the same
//! top-left origin. wry's `set_bounds` takes logical points relative to that
//! view, so the conversion is an inset and a sanity check, nothing more.
//!
//! The 2 pt inset keeps the slot's own border and any splitter beside it
//! grabbable: the native view sits above all DOM, so a view flush with the
//! slot would swallow the pointer on its edge.

use serde::{Deserialize, Serialize};

/// Points trimmed from each edge of the slot.
pub const SLOT_INSET: f64 = 2.0;

/// Below this the view is hidden rather than drawn as a sliver.
pub const MIN_VISIBLE_EDGE: f64 = 8.0;

/// Largest coordinate accepted, in points. Anything past it is a UI bug, not
/// a display.
pub const MAX_COORDINATE: f64 = 100_000.0;

/// A rectangle in logical points (CSS pixels of the hosting webview).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SlotRect {
    /// Left edge.
    pub x: f64,
    /// Top edge.
    pub y: f64,
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
}

/// What the native view should do for a reported slot rect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Placement {
    /// Draw at these bounds.
    Show(SlotRect),
    /// The slot is too small to draw into (collapsed, mid-animation).
    Collapse,
}

/// Why a reported rect was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeometryError(pub String);

/// Convert a reported slot rect to view bounds: inset, and refuse values no
/// layout produces (NaN, infinite, negative size, absurd coordinates).
pub fn view_bounds(slot: SlotRect) -> Result<Placement, GeometryError> {
    let values = [slot.x, slot.y, slot.width, slot.height];
    if values.iter().any(|v| !v.is_finite()) {
        return Err(GeometryError(format!("slot rect is not finite: {slot:?}")));
    }
    if slot.width < 0.0 || slot.height < 0.0 {
        return Err(GeometryError(format!(
            "slot rect has a negative size: {slot:?}"
        )));
    }
    if values.iter().any(|v| v.abs() > MAX_COORDINATE) {
        return Err(GeometryError(format!(
            "slot rect is out of range: {slot:?}"
        )));
    }
    let width = slot.width - 2.0 * SLOT_INSET;
    let height = slot.height - 2.0 * SLOT_INSET;
    if width < MIN_VISIBLE_EDGE || height < MIN_VISIBLE_EDGE {
        return Ok(Placement::Collapse);
    }
    Ok(Placement::Show(SlotRect {
        x: slot.x + SLOT_INSET,
        y: slot.y + SLOT_INSET,
        width,
        height,
    }))
}

/// Last-write-wins sequencing for `set_rect`: the UI numbers every report and
/// awaits nothing between measuring and sending, so an older `seq` arriving
/// after a newer one is stale and must not move the view back.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RectSequence {
    last: Option<u64>,
}

impl RectSequence {
    /// Record `seq` and say whether it should be applied. Equal `seq` is a
    /// resend and is applied (idempotent); older is dropped.
    pub fn accept(&mut self, seq: u64) -> bool {
        match self.last {
            Some(last) if seq < last => false,
            _ => {
                self.last = Some(seq);
                true
            }
        }
    }
}

#[cfg(test)]
#[path = "geometry_tests.rs"]
mod tests;
