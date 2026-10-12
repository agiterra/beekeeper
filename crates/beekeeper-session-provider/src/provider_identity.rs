//! Scoped provider attestation discovery for relay-only management clients.
use super::*;

impl Provider {
    /// Publish an owner-attested scoped profile only when none is observed.
    pub(crate) async fn ensure_identity_profile(&mut self) -> anyhow::Result<()> {
        let Some(auth) = self.config.auth_tag.as_ref() else {
            return Ok(());
        };
        let owner = beekeeper_sdk::nip_oa::verify_auth_tag(
            &serde_json::to_string(auth.as_slice())?,
            &self.config.keys.public_key(),
        )
        .map_err(|_| {
            anyhow::anyhow!("provider owner attestation is invalid; profile not published")
        })?;
        let client = self
            .rest_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("provider profile transport unavailable"))?;
        let response = client
            .query(&[nostr::Filter::new()
                .kind(nostr::Kind::Metadata)
                .author(self.config.keys.public_key())
                .limit(1)])
            .await?;
        let rows = response
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("provider profile observation malformed"))?;
        if let Some(raw) = rows.first() {
            let existing: Event = serde_json::from_value(raw.clone())?;
            if existing.kind != nostr::Kind::Metadata
                || existing.pubkey != self.config.keys.public_key()
                || existing.verify().is_err()
            {
                return Err(anyhow::anyhow!(
                    "provider profile observation is not verified"
                ));
            }
            let tag = existing
                .tags
                .iter()
                .find(|tag| tag.as_slice().first().map(String::as_str) == Some("auth"))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "existing provider profile lacks owner attestation; preserved for repair"
                    )
                })?;
            let wire = serde_json::to_string(tag.as_slice())?;
            if beekeeper_sdk::nip_oa::verify_auth_tag(&wire, &existing.pubkey).ok() != Some(owner) {
                return Err(anyhow::anyhow!(
                    "existing provider profile owner differs; preserved for repair"
                ));
            }
            return Ok(()); // Preserve the accepted signed profile, including its human-facing fields.
        }
        let key = format!("provider-profile:{}", self.config.pubkey_hex());
        if self.outbox.contains(0, &key) {
            return Ok(());
        }
        let tag = auth.clone();
        let event = nostr::EventBuilder::new(
            nostr::Kind::Metadata,
            serde_json::json!({"name":"Beekeeper execution provider"}).to_string(),
        )
        .tags([tag])
        .sign_with_keys(&self.config.keys)?;
        self.outbox.enqueue(0, &key, Priority::Live, event)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::Keys;
    #[tokio::test]
    async fn scoped_profile_is_queued_publicly_existing_profile_preserved_and_unattested_skipped() {
        let dir = tempfile::tempdir().expect("state");
        let mut provider = crate::tests::provider(dir.path(), None);
        let owner = Keys::generate();
        let record = beekeeper_host_core::provision::mint_provider_record(
            &owner,
            &provider.config.relay_url,
        )
        .expect("scoped record");
        provider.config.keys = Keys::parse(&record.private_key_nsec).expect("scoped key");
        provider.config.auth_tag = Some(
            nostr::Tag::parse(
                serde_json::from_str::<Vec<String>>(
                    record.auth_tag.as_deref().expect("attestation"),
                )
                .expect("tag JSON"),
            )
            .expect("tag"),
        );
        // Reopen the outbox under the actual scoped identity, as startup does.
        provider.outbox =
            Outbox::open(dir.path(), &provider.config.pubkey_hex()).expect("scoped outbox");
        let (relay, queries, server) =
            crate::tests::spawn_test_relay_with_events(&provider.config.keys, Vec::new()).await;
        provider.set_rest_client(relay.rest_client());
        provider
            .ensure_identity_profile()
            .await
            .expect("bootstrap profile");
        provider
            .ensure_identity_profile()
            .await
            .expect("pending retry");
        let profiles: Vec<Event> = provider
            .outbox
            .pending_events()
            .filter(|event| event.kind.as_u16() == 0)
            .cloned()
            .collect();
        assert_eq!(profiles.len(), 1);
        let profile = &profiles[0];
        assert!(profile.verify().is_ok());
        assert_eq!(profile.pubkey, provider.config.keys.public_key());
        let auth = profile
            .tags
            .iter()
            .find(|tag| tag.as_slice()[0] == "auth")
            .expect("public attestation");
        assert_eq!(
            beekeeper_sdk::nip_oa::verify_auth_tag(
                &serde_json::to_string(auth.as_slice()).expect("wire"),
                &profile.pubkey
            )
            .expect("verified"),
            owner.public_key()
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&profile.content).expect("content"),
            serde_json::json!({"name":"Beekeeper execution provider"})
        );
        assert!(queries
            .lock()
            .expect("queries")
            .iter()
            .all(|query| query.to_string().contains("0")));
        server.abort();
        let other = tempfile::tempdir().expect("fresh existing state");
        let mut existing = Provider::new(Config {
            state_dir: other.path().into(),
            ..provider.config.clone()
        })
        .expect("existing provider");
        let custom =
            nostr::EventBuilder::new(nostr::Kind::Metadata, r#"{"name":"Preserved provider"}"#)
                .tags(profile.tags.iter().cloned())
                .sign_with_keys(&provider.config.keys)
                .expect("custom profile");
        let (relay, _, server) =
            crate::tests::spawn_test_relay_with_events(&existing.config.keys, vec![custom]).await;
        existing.set_rest_client(relay.rest_client());
        existing
            .ensure_identity_profile()
            .await
            .expect("preserved existing");
        assert!(!existing
            .outbox
            .pending_events()
            .any(|event| event.kind.as_u16() == 0));
        server.abort();
        existing.config.auth_tag =
            Some(nostr::Tag::parse(["auth", "invalid"]).expect("invalid attestation tag"));
        assert!(existing.ensure_identity_profile().await.is_err());
        existing.config.auth_tag = None;
        existing.rest_client = None;
        existing
            .ensure_identity_profile()
            .await
            .expect("unmanaged provider unchanged");
        assert!(!existing
            .outbox
            .pending_events()
            .any(|event| event.kind.as_u16() == 0));
    }
}
