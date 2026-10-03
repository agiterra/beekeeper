//! Provider-wide isolation settings for coding sessions.
//!
//! Two settings a host may set on a dedicated provider instance (a lab, an
//! evaluation rig) to isolate every coding session it runs beyond the file
//! boundary every session already gets. Both are absent by default, and
//! absent means today's behaviour exactly.
//!
//! * `BUZZ_CSP_SESSION_OPERATOR_GIT=withhold` — no session receives the
//!   operator's Git credentials: no `credential.*` helper is staged or
//!   granted, the operator's `nostr.keyfile` is neither staged nor granted,
//!   no keychain grant is made for a helper, and the ssh agent socket is
//!   neither granted nor exported. `user.name`/`user.email` and filter
//!   drivers (Git LFS) are still staged. `grant` (or unset) keeps the
//!   default.
//! * `BUZZ_CSP_SESSION_EGRESS_PROXY=<loopback ip>:<port>` — the session's
//!   boundary denies every outbound network operation (TCP, UDP and DNS,
//!   Unix-domain connects) except TCP to that loopback port, and the session
//!   environment points every proxy variable at it with `NO_PROXY` emptied.
//!   The proxy itself is the host's to supply. A value that is not a
//!   loopback `ip:port` refuses provider startup; it never falls back to an
//!   open network.
//!
//! Both apply to coding sessions only (purpose `Session`), seated or not.
//! Host commands — action steps, host Git fetches — keep their own rights.
//! What was withheld is recorded on the session record and disclosed in the
//! transcript ([`isolation_status_items`]), never silently omitted.

use std::net::SocketAddr;

use buzz_acp::exec_boundary::Egress;

use crate::config::ConfigError;

/// Environment variable: `withhold` keeps the operator's Git credentials
/// out of every coding session.
pub const OPERATOR_GIT_ENV: &str = "BUZZ_CSP_SESSION_OPERATOR_GIT";
/// Environment variable: the loopback `ip:port` every coding session's
/// outbound network is confined to.
pub const EGRESS_PROXY_ENV: &str = "BUZZ_CSP_SESSION_EGRESS_PROXY";

/// Transcript status slug: this session holds none of the operator's Git
/// credentials.
pub const STATUS_OPERATOR_GIT_WITHHELD: &str = "operator_git_withheld";
/// Transcript status slug: this session's outbound network reaches only the
/// host's loopback proxy.
pub const STATUS_EGRESS_PROXY_ONLY: &str = "network_egress_proxy_only";

/// The recorded word for withheld operator Git auth.
pub const RECORDED_WITHHELD: &str = "withheld";
/// The recorded word for egress confined to a loopback proxy.
pub const RECORDED_LOOPBACK_PROXY: &str = "loopback-proxy";

/// Proxy variables a confined session receives, each set to the proxy URL.
pub const PROXY_VARS: &[&str] = &[
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "ALL_PROXY",
    "https_proxy",
    "http_proxy",
    "all_proxy",
];
/// Proxy-exemption variables a confined session receives empty: any exempt
/// host would be a direct connection the boundary refuses.
pub const NO_PROXY_VARS: &[&str] = &["NO_PROXY", "no_proxy"];

/// The provider's session isolation settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SessionIsolation {
    /// Keep the operator's Git credentials out of every coding session.
    pub withhold_operator_git: bool,
    /// What outbound network every coding session may open.
    pub egress: Egress,
}

impl SessionIsolation {
    /// Read both settings.
    ///
    /// # Errors
    /// [`ConfigError::Invalid`] for an operator-Git value other than
    /// `withhold`/`grant`, or an egress value that is not a loopback
    /// `ip:port` with a non-zero port.
    pub fn from_lookup(
        lookup: &impl Fn(&'static str) -> Option<String>,
    ) -> Result<Self, ConfigError> {
        let setting = |name| {
            lookup(name)
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        };
        let withhold_operator_git = match setting(OPERATOR_GIT_ENV).as_deref() {
            None | Some("grant") => false,
            Some("withhold") => true,
            Some(other) => {
                return Err(ConfigError::Invalid {
                    name: OPERATOR_GIT_ENV,
                    reason: format!("expected `withhold` or `grant`, got {other:?}"),
                })
            }
        };
        let egress = match setting(EGRESS_PROXY_ENV) {
            None => Egress::Unrestricted,
            Some(value) => {
                let invalid = |reason: String| ConfigError::Invalid {
                    name: EGRESS_PROXY_ENV,
                    reason,
                };
                let addr: SocketAddr = value.parse().map_err(|error| {
                    invalid(format!(
                        "expected a loopback ip:port such as 127.0.0.1:8899, got {value:?} \
                         ({error})"
                    ))
                })?;
                Egress::loopback_proxy(addr).map_err(|error| invalid(error.to_string()))?
            }
        };
        Ok(Self {
            withhold_operator_git,
            egress,
        })
    }

    /// Whether any setting differs from the default.
    #[must_use]
    pub fn is_configured(&self) -> bool {
        *self != Self::default()
    }
}

/// The proxy URL a confined session's environment names.
#[must_use]
pub fn proxy_url(addr: SocketAddr) -> String {
    format!("http://{addr}")
}

/// What a session's boundary withheld, as transcript status items, in a
/// stable order. Empty when nothing was withheld, so a default provider's
/// transcript is unchanged. Status items are additive by contract: a reader
/// that does not know the slug renders a generic status row.
#[must_use]
pub fn isolation_status_items(
    state: &crate::execution_scope::BoundaryState,
) -> Vec<serde_json::Value> {
    let crate::execution_scope::BoundaryState::Enforced { isolation, .. } = state else {
        return Vec::new();
    };
    let mut items = Vec::new();
    if isolation.withhold_operator_git {
        items.push(serde_json::json!({
            "kind": "status",
            "status": STATUS_OPERATOR_GIT_WITHHELD,
            "reason": "provider-setting",
        }));
    }
    if isolation.egress.proxy().is_some() {
        items.push(serde_json::json!({
            "kind": "status",
            "status": STATUS_EGRESS_PROXY_ONLY,
            "reason": RECORDED_LOOPBACK_PROXY,
        }));
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(pairs: &[(&'static str, &str)]) -> Result<SessionIsolation, ConfigError> {
        let pairs: Vec<(&'static str, String)> = pairs
            .iter()
            .map(|(name, value)| (*name, (*value).to_owned()))
            .collect();
        SessionIsolation::from_lookup(&|name| {
            pairs
                .iter()
                .find(|(known, _)| *known == name)
                .map(|(_, value)| value.clone())
        })
    }

    #[test]
    fn absent_settings_are_the_default() {
        let isolation = read(&[]).expect("default");
        assert_eq!(isolation, SessionIsolation::default());
        assert!(!isolation.is_configured());
        assert_eq!(
            read(&[(OPERATOR_GIT_ENV, "grant")]).expect("grant"),
            isolation
        );
        assert_eq!(read(&[(EGRESS_PROXY_ENV, "  ")]).expect("blank"), isolation);
    }

    #[test]
    fn withhold_and_a_loopback_proxy_are_read() {
        let isolation = read(&[
            (OPERATOR_GIT_ENV, "withhold"),
            (EGRESS_PROXY_ENV, "127.0.0.1:8899"),
        ])
        .expect("valid");
        assert!(isolation.withhold_operator_git);
        let proxy = isolation.egress.proxy().expect("proxy");
        assert_eq!(proxy_url(proxy), "http://127.0.0.1:8899");
        let v6 = read(&[(EGRESS_PROXY_ENV, "[::1]:8899")]).expect("v6");
        assert_eq!(
            v6.egress.proxy().map(proxy_url).as_deref(),
            Some("http://[::1]:8899")
        );
    }

    #[test]
    fn anything_but_a_loopback_ip_and_port_refuses_startup() {
        for bad in [
            "8899",
            "localhost:8899",
            "10.0.0.2:8899",
            "0.0.0.0:8899",
            "127.0.0.1:0",
            "http://127.0.0.1:8899",
        ] {
            assert!(
                matches!(
                    read(&[(EGRESS_PROXY_ENV, bad)]),
                    Err(ConfigError::Invalid {
                        name: EGRESS_PROXY_ENV,
                        ..
                    })
                ),
                "{bad} was accepted"
            );
        }
        assert!(matches!(
            read(&[(OPERATOR_GIT_ENV, "off")]),
            Err(ConfigError::Invalid {
                name: OPERATOR_GIT_ENV,
                ..
            })
        ));
    }
}
