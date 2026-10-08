//! The WebKit calls wry does not expose, for the session preview.
//!
//! Every `unsafe` block in the preview is here or in `snapshot_macos.rs`,
//! each with its justification, following `mouse_nav.rs`. They are all the
//! same kind of unsafe: objc2 marks most WebKit methods `unsafe` because the
//! generator cannot prove nullability or thread affinity, and the callers
//! here guarantee both — every function takes a `MainThreadMarker` or runs
//! inside a main-thread closure, and every pointer a completion block
//! receives is null-checked before use.

use std::cell::RefCell;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::MainThreadMarker;
use objc2_foundation::{NSDictionary, NSError, NSObjectProtocol, NSProcessInfo, NSString};
use objc2_web_kit::{
    WKContentRuleList, WKContentRuleListStore, WKContentWorld, WKUserContentController,
    WKUserScript, WKUserScriptInjectionTime, WKWebView,
};

/// The isolated script world the driver runs in. Shares the page's DOM, none
/// of its globals.
pub const DRIVER_WORLD: &str = "beekeeper-preview-driver";

/// Which JavaScript world a call runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum World {
    /// The isolated driver world.
    Driver,
    /// The page's own world.
    Page,
}

fn content_world(world: World, mtm: MainThreadMarker) -> Retained<WKContentWorld> {
    match world {
        // SAFETY: class methods on WKContentWorld (macOS 11+, below the app's
        // minimum), called on the main thread (`mtm`); the name is a valid
        // NSString.
        World::Driver => unsafe {
            WKContentWorld::worldWithName(&NSString::from_str(DRIVER_WORLD), mtm)
        },
        // SAFETY: as above.
        World::Page => unsafe { WKContentWorld::pageWorld(mtm) },
    }
}

/// The running macOS major version.
pub fn os_major_version() -> isize {
    NSProcessInfo::processInfo()
        .operatingSystemVersion()
        .majorVersion
}

fn error_text(error: *mut NSError) -> String {
    if error.is_null() {
        return "unknown WebKit error".to_string();
    }
    // SAFETY: non-null, and WebKit hands completion blocks a valid NSError
    // that lives for the duration of the block.
    let error = unsafe { &*error };
    // WebKit puts the thrown JavaScript message in the user info; the
    // localized description is only "A JavaScript exception occurred".
    let info = error.userInfo();
    let message = info
        .objectForKey(&NSString::from_str("WKJavaScriptExceptionMessage"))
        .and_then(|value| value.downcast::<NSString>().ok())
        .map(|value| value.to_string());
    message.unwrap_or_else(|| error.localizedDescription().to_string())
}

/// Compile the loopback-only content rule list. `done` gets the compiled
/// list or WebKit's reason, on the main thread.
pub fn compile_rule_list(
    mtm: MainThreadMarker,
    identifier: &str,
    json: &str,
    done: impl FnOnce(Result<Retained<WKContentRuleList>, String>) + 'static,
) {
    // SAFETY: main thread (`mtm`); the default store exists on every
    // supported macOS (11+).
    let Some(store) = (unsafe { WKContentRuleListStore::defaultStore(mtm) }) else {
        done(Err("WebKit has no content rule list store".to_string()));
        return;
    };
    let done = RefCell::new(Some(done));
    let block = RcBlock::new(move |list: *mut WKContentRuleList, error: *mut NSError| {
        let Some(done) = done.borrow_mut().take() else {
            return;
        };
        if list.is_null() {
            done(Err(error_text(error)));
            return;
        }
        // SAFETY: non-null; WebKit passes a valid, autoreleased list that
        // `retain` keeps alive past the block.
        match unsafe { Retained::retain(list) } {
            Some(list) => done(Ok(list)),
            None => done(Err("WebKit returned no rule list".to_string())),
        }
    });
    // SAFETY: main thread; both strings are valid NSStrings and the block
    // matches the declared completion signature.
    unsafe {
        store.compileContentRuleListForIdentifier_encodedContentRuleList_completionHandler(
            Some(&NSString::from_str(identifier)),
            Some(&NSString::from_str(json)),
            Some(&block),
        );
    }
}

/// Attach a compiled rule list to a webview's content controller.
pub fn add_rule_list(manager: &WKUserContentController, list: &WKContentRuleList) {
    // SAFETY: both objects are valid; called on the main thread where the
    // webview lives.
    unsafe { manager.addContentRuleList(list) };
}

/// Install `source` to run at document start in the driver world of every
/// top-level document the webview loads from now on.
pub fn add_driver_script(mtm: MainThreadMarker, manager: &WKUserContentController, source: &str) {
    let world = content_world(World::Driver, mtm);
    // SAFETY: main thread; `alloc` + `init…inContentWorld:` is WKUserScript's
    // designated initializer (macOS 11+) with valid arguments.
    let script = unsafe {
        WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld(
            mtm.alloc(),
            &NSString::from_str(source),
            WKUserScriptInjectionTime::AtDocumentStart,
            true,
            &world,
        )
    };
    // SAFETY: valid controller and script, main thread.
    unsafe { manager.addUserScript(&script) };
}

/// Run `body` as an async function in `world` with one string argument,
/// `payload`. `done` gets the returned string (the body must return a
/// string or nothing) or the error, including a thrown exception's message.
pub fn call_async(
    mtm: MainThreadMarker,
    webview: &WKWebView,
    body: &str,
    payload: &str,
    world: World,
    done: impl FnOnce(Result<Option<String>, String>) + 'static,
) {
    let world = content_world(world, mtm);
    let key = NSString::from_str("payload");
    let value = NSString::from_str(payload);
    let value_object: &AnyObject = &value;
    let arguments: Retained<NSDictionary<NSString, AnyObject>> =
        NSDictionary::from_slices(&[&*key], &[value_object]);
    let done = RefCell::new(Some(done));
    let block = RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
        let Some(done) = done.borrow_mut().take() else {
            return;
        };
        if !error.is_null() {
            done(Err(error_text(error)));
            return;
        }
        if value.is_null() {
            done(Ok(None));
            return;
        }
        // SAFETY: non-null, and WebKit passes a valid object for the block's
        // duration.
        let value = unsafe { &*value };
        match value.downcast_ref::<NSString>() {
            Some(text) => done(Ok(Some(text.to_string()))),
            None => done(Err("the driver returned a non-string value".to_string())),
        }
    });
    // SAFETY: main thread; valid body, arguments, world and a block matching
    // the declared completion signature. `None` frame = the main frame.
    unsafe {
        webview.callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(
            &NSString::from_str(body),
            Some(&arguments),
            None,
            &world,
            Some(&block),
        );
    }
}

/// Back/forward availability.
pub fn history(webview: &WKWebView) -> (bool, bool) {
    // SAFETY: plain property reads on a valid webview, main thread.
    unsafe { (webview.canGoBack(), webview.canGoForward()) }
}

/// Go back one entry, if there is one.
pub fn go_back(webview: &WKWebView) {
    // SAFETY: valid webview, main thread; a nil navigation is fine to drop.
    let _ = unsafe { webview.goBack() };
}

/// Go forward one entry, if there is one.
pub fn go_forward(webview: &WKWebView) {
    // SAFETY: as `go_back`.
    let _ = unsafe { webview.goForward() };
}

/// The committed URL, or `None` before the first commit. wry's own `url()`
/// unwraps the nil URL a fresh view has and would panic.
pub fn current_url(webview: &WKWebView) -> Option<String> {
    // SAFETY: property reads on a valid webview, main thread; both results
    // are nullable and checked.
    unsafe {
        webview
            .URL()
            .and_then(|url| url.absoluteString())
            .map(|url| url.to_string())
    }
}

/// The document title, when there is one.
pub fn title(webview: &WKWebView) -> Option<String> {
    // SAFETY: nullable property read on a valid webview, main thread.
    unsafe { webview.title() }
        .map(|title| title.to_string())
        .filter(|title| !title.is_empty())
}

/// Let Safari's Web Inspector attach (dev builds only; macOS 13.3+).
pub fn set_inspectable(webview: &WKWebView, inspectable: bool) {
    if !webview.respondsToSelector(objc2::sel!(setInspectable:)) {
        return;
    }
    // SAFETY: the selector exists on this OS (checked above); valid webview,
    // main thread.
    unsafe { webview.setInspectable(inspectable) };
}
