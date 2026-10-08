//! What a session preview may load: this machine's loopback servers, and
//! nothing else.
//!
//! The preview webview runs inside the desktop app, outside every seat's
//! sandbox, so it must not become the way around `Egress::LoopbackProxy`. The
//! same rule is enforced at three points that do not share code paths:
//!
//! 1. the broker and the Tauri commands, before any `open`/`navigate`
//!    reaches WebKit ([`check_preview_url`]);
//! 2. wry's navigation handler, for every navigation the page or the person
//!    starts, in any frame ([`navigation_allowed`]);
//! 3. a `WKContentRuleList` that blocks every non-loopback subresource
//!    ([`content_rule_list_json`]). If WebKit cannot compile it the preview
//!    does not open: failing closed is the point.
//!
//! `window.open` is denied outright by the new-window handler in `view.rs`.
//!
//! "Isolation, not security": this stops an agent's page from quietly
//! reaching the internet through the app, and keeps one session's cookies
//! out of another's. It is not a defence against a hostile local process.

use sha2::{Digest, Sha256};
use url::{Host, Url};

/// Largest `eval` result returned to an agent, in bytes of JSON.
pub const EVAL_RESULT_CAP_BYTES: usize = 64 * 1024;
/// Largest snapshot PNG returned to an agent, in bytes.
pub const PNG_CAP_BYTES: usize = 4 * 1024 * 1024;

/// The stable refusal code for every URL the policy turns away (WIRE-C4 §5).
pub const URL_REFUSED_CODE: &str = "preview_url_refused";

/// The sentence a refused URL carries (WIRE-C4 §2).
pub const URL_REFUSED_SENTENCE: &str =
    "The Browser only opens pages on this computer (localhost, 127.0.0.1, [::1]).";

/// The blank page a preview shows before anything is opened in it.
pub const ABOUT_BLANK: &str = "about:blank";

/// Why a URL was refused, as a stable machine reason plus a sentence a
/// person can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyRefusal {
    /// Stable wire code: always [`URL_REFUSED_CODE`] today, kept a field so
    /// a later reason does not change the type.
    pub code: &'static str,
    /// What happened and what is allowed instead.
    pub message: String,
}

/// The origin the app's own dev frontend is served from, which a preview must
/// never show: a page there would look like the app while having none of its
/// privileges, and a person could not tell the two apart. `None` in release
/// builds, whose frontend is not served over HTTP at all.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RefusedOrigin {
    /// Lowercased host of the dev frontend.
    pub host: String,
    /// Port of the dev frontend.
    pub port: u16,
}

impl RefusedOrigin {
    /// The refused origin for a Tauri `devUrl`, when there is one.
    pub fn from_dev_url(dev_url: Option<&Url>) -> Option<Self> {
        let url = dev_url?;
        let host = url.host_str()?.to_ascii_lowercase();
        let port = url.port_or_known_default()?;
        Some(Self { host, port })
    }
}

/// Whether `host` is one of the three loopback spellings a preview may load.
fn is_loopback_host(host: &Host<&str>) -> bool {
    match host {
        Host::Domain(domain) => domain.eq_ignore_ascii_case("localhost"),
        Host::Ipv4(addr) => *addr == std::net::Ipv4Addr::LOCALHOST,
        Host::Ipv6(addr) => *addr == std::net::Ipv6Addr::LOCALHOST,
    }
}

/// Every loopback host spelling counts as the same machine, so the dev
/// origin is refused whichever one a caller uses.
fn same_machine_origin(url: &Url, refused: &RefusedOrigin) -> bool {
    let Some(port) = url.port_or_known_default() else {
        return false;
    };
    if port != refused.port {
        return false;
    }
    let refused_is_loopback = matches!(
        refused.host.as_str(),
        "localhost" | "127.0.0.1" | "[::1]" | "::1"
    );
    refused_is_loopback
        || url
            .host_str()
            .is_some_and(|host| host.eq_ignore_ascii_case(&refused.host))
}

fn refusal(message: String) -> PolicyRefusal {
    PolicyRefusal {
        code: URL_REFUSED_CODE,
        message,
    }
}

/// Check a URL a caller (agent or person) asked the preview to open.
///
/// Allowed: `http`/`https` to `localhost`, `127.0.0.1` or `[::1]`, any port,
/// with no user-info; and `about:blank`. Everything else is refused with a
/// reason, including `file:`, `tauri:`, `ipc:`, `buzz-media:`, and the app's
/// own dev frontend origin.
pub fn check_preview_url(raw: &str, refused: Option<&RefusedOrigin>) -> Result<Url, PolicyRefusal> {
    let trimmed = raw.trim();
    if trimmed.eq_ignore_ascii_case(ABOUT_BLANK) {
        return Url::parse(ABOUT_BLANK).map_err(|_| refusal(URL_REFUSED_SENTENCE.to_string()));
    }
    let not_local = || refusal(URL_REFUSED_SENTENCE.to_string());
    let url = Url::parse(trimmed).map_err(|_| not_local())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(not_local());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(not_local());
    }
    let Some(host) = url.host() else {
        return Err(not_local());
    };
    if !is_loopback_host(&host) {
        return Err(not_local());
    }
    if let Some(refused) = refused {
        if same_machine_origin(&url, refused) {
            return Err(refusal(format!(
                "{URL_REFUSED_SENTENCE} {trimmed} is where this Beekeeper build serves its own \
                 interface, so it is not shown either."
            )));
        }
    }
    Ok(url)
}

/// The navigation handler's verdict for a URL WebKit is about to load in any
/// frame. Same rule as [`check_preview_url`], plus the local document schemes
/// a page may legitimately navigate a frame to (`about:srcdoc`, `data:`,
/// `blob:`), none of which leaves the machine.
pub fn navigation_allowed(raw: &str, refused: Option<&RefusedOrigin>) -> bool {
    let lower = raw.trim_start().to_ascii_lowercase();
    if lower.starts_with("about:") || lower.starts_with("data:") || lower.starts_with("blob:") {
        return true;
    }
    check_preview_url(raw, refused).is_ok()
}

/// Escape a literal for a WebKit content-rule `url-filter` regular
/// expression.
fn escape_filter(literal: &str) -> String {
    let mut out = String::with_capacity(literal.len() * 2);
    for ch in literal.chars() {
        if matches!(
            ch,
            '.' | '[' | ']' | '(' | ')' | '*' | '+' | '?' | '^' | '$' | '\\' | '|' | '{' | '}'
        ) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// The `WKContentRuleList` source: block every load, then un-block loopback
/// HTTP(S) and WebSocket loads (Vite's HMR socket included) and local
/// document schemes.
///
/// WebKit's rule regexes have no disjunction, so each scheme × host × port
/// shape is its own rule. Each allow rule requires the authority to end at a
/// `/` (bare host) or at `:<digits>/`, which is what keeps
/// `http://localhost:x@example.com/` (user-info, host `example.com`) blocked.
pub fn content_rule_list_json() -> String {
    let mut rules = vec![serde_json::json!({
        "trigger": { "url-filter": ".*" },
        "action": { "type": "block" }
    })];
    for scheme in ["https?", "wss?"] {
        for host in ["localhost", "127.0.0.1", "[::1]"] {
            let host = escape_filter(host);
            for port in ["", ":[0-9]+"] {
                rules.push(serde_json::json!({
                    "trigger": { "url-filter": format!("^{scheme}://{host}{port}/") },
                    "action": { "type": "ignore-previous-rules" }
                }));
            }
        }
    }
    for scheme in ["about:", "data:", "blob:"] {
        rules.push(serde_json::json!({
            "trigger": { "url-filter": format!("^{scheme}") },
            "action": { "type": "ignore-previous-rules" }
        }));
    }
    serde_json::Value::Array(rules).to_string()
}

/// The identifier the compiled rule list is stored under in WebKit's rule
/// store. Versioned so a changed rule set is compiled fresh rather than an
/// old one looked up.
pub const CONTENT_RULE_LIST_ID: &str = "beekeeper-session-preview-loopback-v1";

/// The per-session WebKit data store id: the first 16 bytes of
/// `sha256(projectRef | channelId)`. Two sessions never share cookies or
/// storage; reopening the same session finds its own again.
pub fn data_store_id(project_ref: &str, channel_id: &str) -> [u8; 16] {
    let mut hasher = Sha256::new();
    hasher.update(project_ref.as_bytes());
    hasher.update(b"|");
    hasher.update(channel_id.as_bytes());
    let digest = hasher.finalize();
    let mut id = [0u8; 16];
    id.copy_from_slice(&digest[..16]);
    id
}

/// Truncate `text` to at most `cap` bytes on a character boundary. Returns
/// the text and whether anything was cut.
pub fn cap_text(text: &str, cap: usize) -> (String, bool) {
    if text.len() <= cap {
        return (text.to_string(), false);
    }
    let mut end = cap;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_string(), true)
}

#[cfg(test)]
#[path = "policy_tests.rs"]
mod tests;
