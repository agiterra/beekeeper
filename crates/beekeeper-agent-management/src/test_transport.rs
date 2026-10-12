//! Wire fixture adapter only; production clients keep their existing authenticated transport.
use crate::{Error, RelayTransport};
use nostr::{Event, Keys, Tag};
use serde_json::{json, Value};
pub(crate) struct Client {
    url: String,
    keys: Keys,
    auth: Option<String>,
    http: reqwest::Client,
}
impl Client {
    pub(crate) fn new(
        url: String,
        keys: Keys,
        _tag: Option<Tag>,
        auth: Option<String>,
    ) -> Result<Self, Error> {
        Ok(Self {
            url,
            keys,
            auth,
            http: reqwest::Client::new(),
        })
    }
    pub(crate) fn keys(&self) -> &Keys {
        &self.keys
    }
}
impl RelayTransport for Client {
    fn keys(&self) -> &Keys {
        &self.keys
    }
    fn relay_url(&self) -> &str {
        &self.url
    }
    fn verified_auth_tag_owner_hex(&self) -> Result<Option<String>, Error> {
        self.auth
            .as_ref()
            .map(|auth| {
                beekeeper_sdk::nip_oa::verify_auth_tag(auth, &self.keys.public_key())
                    .map(|key| key.to_hex())
                    .map_err(|_| Error::Auth("attestation invalid".into()))
            })
            .transpose()
    }
    fn scoped(&self, keys: Keys, auth: String) -> Result<Self, Error> {
        Self::new(self.url.clone(), keys, None, Some(auth))
    }
    async fn query_all(&self, filter: Value) -> Result<Vec<Value>, Error> {
        let response = self
            .http
            .post(format!("{}/query", self.url))
            .json(&json!([filter]))
            .send()
            .await
            .map_err(|e| Error::Other(e.to_string()))?;
        if !response.status().is_success() {
            return Err(Error::Auth("fixture query refused".into()));
        }
        response
            .json()
            .await
            .map_err(|e| Error::Other(e.to_string()))
    }
    async fn submit_event(&self, event: Event) -> Result<Value, Error> {
        let response = self
            .http
            .post(format!("{}/events", self.url))
            .json(&event)
            .send()
            .await
            .map_err(|e| Error::Other(e.to_string()))?;
        if !response.status().is_success() {
            return Err(Error::Refused("fixture publication refused".into()));
        }
        response
            .json()
            .await
            .map_err(|e| Error::Other(e.to_string()))
    }
}
