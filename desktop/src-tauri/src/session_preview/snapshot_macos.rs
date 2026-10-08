//! A picture of the preview: `WKWebView.takeSnapshot`, encoded to PNG (for
//! agents) or JPEG (for the freeze frame the slot shows while an overlay
//! hides the view). Works while the view is hidden, which is what makes the
//! freeze frame possible: hide first, so the overlay is never drawn under the
//! view, then take the picture.

use std::cell::RefCell;

use block2::RcBlock;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
use objc2_foundation::{NSDictionary, NSError};
use objc2_web_kit::WKWebView;

/// Image encodings a snapshot can be returned in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// Lossless, for agents comparing against a fixture.
    Png,
    /// Small, for the freeze frame.
    Jpeg,
}

/// An encoded snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// Encoded bytes.
    pub bytes: Vec<u8>,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

fn encode(image: &NSImage, encoding: Encoding) -> Option<Snapshot> {
    let tiff = image.TIFFRepresentation()?;
    let rep = NSBitmapImageRep::imageRepWithData(&tiff)?;
    let file_type = match encoding {
        Encoding::Png => NSBitmapImageFileType::PNG,
        Encoding::Jpeg => NSBitmapImageFileType::JPEG,
    };
    // SAFETY: valid bitmap rep and an empty (valid) property dictionary;
    // AppKit's documented encoder entry point.
    let data = unsafe { rep.representationUsingType_properties(file_type, &NSDictionary::new()) }?;
    Some(Snapshot {
        bytes: data.to_vec(),
        width: u32::try_from(rep.pixelsWide()).unwrap_or(0),
        height: u32::try_from(rep.pixelsHigh()).unwrap_or(0),
    })
}

/// Take a snapshot of the visible viewport. `done` runs on the main thread.
pub fn take(
    webview: &WKWebView,
    encoding: Encoding,
    done: impl FnOnce(Result<Snapshot, String>) + 'static,
) {
    let done = RefCell::new(Some(done));
    let block = RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
        let Some(done) = done.borrow_mut().take() else {
            return;
        };
        if image.is_null() {
            let reason = if error.is_null() {
                "WebKit returned no image".to_string()
            } else {
                // SAFETY: non-null NSError, valid for the block's duration.
                unsafe { &*error }.localizedDescription().to_string()
            };
            done(Err(reason));
            return;
        }
        // SAFETY: non-null NSImage, valid for the block's duration.
        let image = unsafe { &*image };
        match encode(image, encoding) {
            Some(snapshot) => done(Ok(snapshot)),
            None => done(Err("the snapshot could not be encoded".to_string())),
        }
    });
    // SAFETY: valid webview on the main thread; `None` configuration = the
    // visible viewport; the block matches the declared signature.
    unsafe { webview.takeSnapshotWithConfiguration_completionHandler(None, &block) };
}
