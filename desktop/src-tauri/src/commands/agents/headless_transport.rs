//! Desktop signing and authenticated relay transport for the shared service.
//! No participant lifecycle, private executor access or process ownership lives here.

use beekeeper_agent_management::{Error, RelayTransport};
use nostr::{Event, JsonUtil, Keys};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::app_state::AppState;

fn guarded_event_bytes(event: &Event) -> Result<Vec<u8>, Error> {
    let bytes = event.as_json().into_bytes();
    crate::egress_guard::assert_no_key_backup_bytes(&bytes, "shared agent management")
        .map_err(Error::Other)?;
    Ok(bytes)
}

pub(super) struct Transport {
    app: AppHandle,
    relay: String,
    api: String,
    keys: Keys,
    auth_tag: Option<String>,
}

impl Transport {
    pub(super) fn owner(app: &AppHandle, relay: &str) -> Result<Self, String> {
        let state = app.state::<AppState>();
        let keys = state.signing_keys()?;
        crate::relay::assert_expected_relay_scope(
            Some(relay),
            &crate::relay::relay_api_base_url_with_override(&state),
        )?;
        let mut url = url::Url::parse(relay).map_err(|_| "invalid management community")?;
        let scheme = match url.scheme() {
            "ws" | "http" => "http",
            "wss" | "https" => "https",
            _ => return Err("invalid management community scheme".into()),
        };
        url.set_scheme(scheme)
            .map_err(|_| "invalid management community scheme")?;
        Ok(Self {
            app: app.clone(),
            relay: relay.into(),
            api: url.to_string().trim_end_matches('/').into(),
            keys,
            auth_tag: None,
        })
    }
}

impl RelayTransport for Transport {
    fn keys(&self) -> &Keys {
        &self.keys
    }
    fn relay_url(&self) -> &str {
        &self.relay
    }
    async fn relay_identity(&self) -> Result<String, Error> {
        let state = self.app.state::<AppState>();
        let response = state
            .http_client
            .get(format!("{}/", self.api))
            .header("Accept", "application/nostr+json")
            .send()
            .await
            .map_err(|e| Error::Other(crate::relay::classify_request_error(&e)))?;
        if !response.status().is_success() {
            return Err(Error::Other(
                crate::relay::relay_error_message(response).await,
            ));
        }
        let metadata: Value = crate::relay::parse_json_response(response)
            .await
            .map_err(Error::Other)?;
        let identity = metadata
            .get("self")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                Error::Refused(
                    "relay identity is unavailable; membership evidence unconfirmed".into(),
                )
            })?;
        nostr::PublicKey::from_hex(identity)
            .map_err(|_| Error::Refused("relay identity is invalid".into()))?;
        Ok(identity.into())
    }
    fn verified_auth_tag_owner_hex(&self) -> Result<Option<String>, Error> {
        self.auth_tag
            .as_deref()
            .map(|auth| {
                beekeeper_sdk_pkg::nip_oa::verify_auth_tag(auth, &self.keys.public_key())
                    .map(|owner| owner.to_hex())
                    .map_err(|_| Error::Auth("invalid scoped owner attestation".into()))
            })
            .transpose()
    }
    fn scoped(&self, keys: Keys, auth_tag: String) -> Result<Self, Error> {
        beekeeper_sdk_pkg::nip_oa::verify_auth_tag(&auth_tag, &keys.public_key())
            .map_err(|_| Error::Auth("invalid actor attestation".into()))?;
        Ok(Self {
            app: self.app.clone(),
            relay: self.relay.clone(),
            api: self.api.clone(),
            keys,
            auth_tag: Some(auth_tag),
        })
    }
    async fn query_all(&self, mut filter: Value) -> Result<Vec<Value>, Error> {
        let state = self.app.state::<AppState>();
        let mut events = Vec::new();
        let mut previous_cursor = None;
        loop {
            filter["limit"] = json!(500);
            let page = crate::relay::query_relay_at_with_keys(
                &state,
                &self.api,
                &[filter.clone()],
                &self.keys,
                self.auth_tag.as_deref(),
            )
            .await
            .map_err(Error::Other)?;
            let done = page.len() < 500;
            if !done {
                let last = page
                    .last()
                    .ok_or_else(|| Error::Other("query page has no cursor".into()))?;
                let cursor = (last.created_at.as_secs(), last.id.to_hex());
                if previous_cursor.as_ref() == Some(&cursor) {
                    return Err(Error::Other(
                        "relay query cursor did not advance; incomplete evidence refused".into(),
                    ));
                }
                filter["until"] = json!(cursor.0);
                filter["before_id"] = json!(cursor.1);
                previous_cursor = Some(cursor);
            }
            for event in page {
                events.push(
                    serde_json::to_value(event)
                        .map_err(|_| Error::Other("invalid query event".into()))?,
                );
            }
            if done {
                return Ok(events);
            }
        }
    }
    async fn submit_event(&self, event: Event) -> Result<Value, Error> {
        if event.pubkey != self.keys.public_key() {
            return Err(Error::Auth(
                "event signer differs from scoped transport".into(),
            ));
        }
        crate::relay_admission::wait_for_rate_limit().await;
        let state = self.app.state::<AppState>();
        let url = format!("{}/events", self.api);
        let bytes = guarded_event_bytes(&event)?;
        let auth = crate::relay::build_nip98_auth_header_for_keys(
            &self.keys,
            &reqwest::Method::POST,
            &url,
            &bytes,
        )
        .map_err(Error::Other)?;
        let mut request = state
            .http_client
            .post(&url)
            .header("Authorization", auth)
            .header("Content-Type", "application/json")
            .body(bytes);
        if let Some(tag) = &self.auth_tag {
            request = request.header("x-auth-tag", tag);
        }
        let response = request
            .send()
            .await
            .map_err(|e| Error::Other(crate::relay::classify_request_error(&e)))?;
        if !response.status().is_success() {
            return Err(Error::Other(
                crate::relay::relay_error_message(response).await,
            ));
        }
        crate::relay::parse_json_response(response)
            .await
            .map_err(Error::Other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_management_body_blocks_injected_key_backup_before_network() {
        let keys = Keys::generate();
        let vector = crate::egress_guard::injection_fixture();
        for content in [vector.to_owned(), vector.to_uppercase()] {
            let event = nostr::EventBuilder::new(nostr::Kind::Custom(30177), content)
                .sign_with_keys(&keys)
                .expect("signed fixture");
            let error = guarded_event_bytes(&event).expect_err("must reject before HTTP");
            assert!(error.to_string().contains("key-backup material"));
        }
        let event = nostr::EventBuilder::new(nostr::Kind::Custom(30177), "ordinary configuration")
            .sign_with_keys(&keys)
            .expect("signed fixture");
        assert_eq!(
            guarded_event_bytes(&event).expect("ordinary body"),
            event.as_json().into_bytes()
        );
    }
}
