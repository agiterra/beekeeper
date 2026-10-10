//! Rust webhook ingress plugins mounted under `/webhooks/<name>`.
//!
//! A plugin owns its vendor routes and signature algorithm. The relay binds
//! the request host to a community and verifies the original body before any
//! plugin handler runs. This surface does not grant event signing authority.

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    http::{header::HOST, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    Router,
};
use beekeeper_core::tenant::TenantContext;

use crate::state::AppState;
use crate::tenant::HostResolver;

const MAX_WEBHOOK_BODY_BYTES: usize = 1024 * 1024;

/// A vendor signature verifier. Inspect the original headers and body bytes;
/// return an error before the vendor handler performs any side effect.
#[async_trait]
pub trait WebhookVerifier: Send + Sync {
    /// Verify one request in its host-bound community.
    async fn verify(
        &self,
        tenant: &TenantContext,
        headers: &HeaderMap,
        raw_body: &[u8],
    ) -> Result<(), StatusCode>;
}

/// A compiled-in plugin and its vendor-owned Axum route tree.
///
/// The route tree can contain ordinary parameters and catch-all routes. Its
/// handlers obtain the resolved community using `Extension<TenantContext>`.
pub struct WebhookPlugin {
    /// Namespace segment under `/webhooks/`.
    pub name: String,
    /// Vendor routes, relative to the plugin namespace.
    pub router: Router,
    /// Vendor signature verifier.
    pub verifier: Arc<dyn WebhookVerifier>,
}

#[derive(Clone)]
struct GuardState {
    state: Arc<AppState>,
    verifier: Arc<dyn WebhookVerifier>,
}

async fn verify_before_handler(
    State(guard): State<GuardState>,
    request: Request,
    next: Next,
) -> Response {
    match authorize_webhook(&guard.state.db, guard.verifier.as_ref(), request).await {
        Ok(request) => next.run(request).await,
        Err(status) => status.into_response(),
    }
}

async fn authorize_webhook<R: HostResolver>(
    resolver: &R,
    verifier: &dyn WebhookVerifier,
    request: Request,
) -> Result<Request, StatusCode>
where
    R::Error: std::fmt::Debug,
{
    let (parts, body) = request.into_parts();
    let raw_host = parts
        .headers
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let tenant = match crate::tenant::bind_community(resolver, raw_host).await {
        Ok(tenant) => tenant,
        Err(error) => {
            tracing::warn!(?error, "webhook host binding failed");
            return Err(StatusCode::NOT_FOUND);
        }
    };
    let raw_body = match to_bytes(body, MAX_WEBHOOK_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(error) => {
            tracing::warn!(?error, "webhook body read failed");
            return Err(StatusCode::PAYLOAD_TOO_LARGE);
        }
    };
    verifier.verify(&tenant, &parts.headers, &raw_body).await?;
    let mut request = Request::from_parts(parts, Body::from(raw_body));
    request.extensions_mut().insert(tenant);
    Ok(request)
}

/// Mount plugin routes under exclusive namespaces. Invalid and duplicate
/// names are rejected before Axum sees the route trees.
pub fn plugin_router(state: Arc<AppState>, plugins: Vec<WebhookPlugin>) -> Result<Router, String> {
    let mut names = HashSet::new();
    let mut router = Router::new();
    for plugin in plugins {
        if !valid_plugin_name(&plugin.name) {
            return Err(format!("invalid webhook plugin name: {}", plugin.name));
        }
        if !names.insert(plugin.name.clone()) {
            return Err(format!("duplicate webhook plugin: {}", plugin.name));
        }
        let guard = GuardState {
            state: Arc::clone(&state),
            verifier: plugin.verifier,
        };
        let guarded = plugin
            .router
            .layer(middleware::from_fn_with_state(guard, verify_before_handler));
        router = router.nest(&format!("/webhooks/{}", plugin.name), guarded);
    }
    Ok(router)
}

fn valid_plugin_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use beekeeper_core::tenant::{CommunityId, TenantContext};

    use super::{authorize_webhook, valid_plugin_name, WebhookVerifier};
    use crate::tenant::HostResolver;

    struct MapResolver(HashMap<String, CommunityId>);

    impl HostResolver for MapResolver {
        type Error = &'static str;

        async fn resolve_host(
            &self,
            normalized_host: &str,
        ) -> Result<Option<CommunityId>, Self::Error> {
            Ok(self.0.get(normalized_host).copied())
        }
    }

    struct ExactVerifier {
        calls: AtomicUsize,
        expected_community: CommunityId,
    }

    #[async_trait::async_trait]
    impl WebhookVerifier for ExactVerifier {
        async fn verify(
            &self,
            tenant: &TenantContext,
            headers: &axum::http::HeaderMap,
            raw_body: &[u8],
        ) -> Result<(), StatusCode> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if tenant.community() != self.expected_community
                || headers
                    .get("x-vendor-signature")
                    .is_none_or(|v| v != "valid")
                || raw_body != br#"{"order":1} "#
            {
                return Err(StatusCode::UNAUTHORIZED);
            }
            Ok(())
        }
    }

    fn request(host: &str, signature: &str) -> Request<Body> {
        Request::builder()
            .uri("/webhooks/vendor/orders/123")
            .header("host", host)
            .header("x-vendor-signature", signature)
            .body(Body::from(br#"{"order":1} "#.to_vec()))
            .unwrap()
    }

    #[test]
    fn plugin_names_cannot_escape_their_namespace() {
        for invalid in ["", "../hooks", "a/b", "UPPER", "-vendor", "vendor-", "a.b"] {
            assert!(!valid_plugin_name(invalid), "{invalid}");
        }
        assert!(valid_plugin_name("github-2"));
    }

    #[tokio::test]
    async fn webhook_guard_binds_host_and_preserves_signed_bytes() {
        let first = CommunityId::from_uuid(uuid::Uuid::from_u128(1));
        let second = CommunityId::from_uuid(uuid::Uuid::from_u128(2));
        let resolver = MapResolver(HashMap::from([
            ("first.example".into(), first),
            ("second.example".into(), second),
        ]));
        let verifier = ExactVerifier {
            calls: AtomicUsize::new(0),
            expected_community: first,
        };

        let admitted = authorize_webhook(&resolver, &verifier, request("first.example", "valid"))
            .await
            .unwrap();
        assert_eq!(
            admitted
                .extensions()
                .get::<TenantContext>()
                .unwrap()
                .community(),
            first
        );
        assert_eq!(
            to_bytes(admitted.into_body(), 1024).await.unwrap().as_ref(),
            br#"{"order":1} "#
        );

        assert_eq!(
            authorize_webhook(&resolver, &verifier, request("first.example", "bad"))
                .await
                .unwrap_err(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            authorize_webhook(&resolver, &verifier, request("second.example", "valid"))
                .await
                .unwrap_err(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            authorize_webhook(&resolver, &verifier, request("unknown.example", "valid"))
                .await
                .unwrap_err(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(verifier.calls.load(Ordering::SeqCst), 3);
    }
}
