use super::*;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting {
    keys: Keys,
    reads: AtomicUsize,
}

impl RelayTransport for Counting {
    fn keys(&self) -> &Keys {
        &self.keys
    }
    fn relay_url(&self) -> &str {
        "ws://127.0.0.1:1"
    }
    fn verified_auth_tag_owner_hex(&self) -> Result<Option<String>, Error> {
        Ok(None)
    }
    fn scoped(&self, _: Keys, _: String) -> Result<Self, Error> {
        Err(Error::Other("unused".into()))
    }
    async fn query_all(&self, filter: Value) -> Result<Vec<Value>, Error> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(vec![filter])
    }
    async fn submit_event(&self, _: nostr::Event) -> Result<Value, Error> {
        Ok(json!({"accepted": true}))
    }
}

/// Eight rows of one provider in one channel used to cost eight identical
/// receipt reads and sixteen identical profile reads; now each filter once.
#[tokio::test]
async fn identical_inventory_reads_reach_the_relay_once_and_writes_never_do() {
    let relay = Counting {
        keys: Keys::generate(),
        reads: AtomicUsize::new(0),
    };
    let cache = ReadCache::new(&relay);
    let receipts = json!({"kinds":[44224],"authors":["p"],"#h":["c"]});
    let profile = json!({"kinds":[0],"authors":["a"],"limit":1});
    for _ in 0..8 {
        assert_eq!(
            cache.query_all(receipts.clone()).await.expect("receipts"),
            vec![receipts.clone()]
        );
        cache.query_all(profile.clone()).await.expect("profile");
    }
    assert_eq!(relay.reads.load(Ordering::SeqCst), 2);
    cache
        .query_all(json!({"kinds":[44224],"authors":["p"],"#h":["other"]}))
        .await
        .expect("another channel");
    assert_eq!(relay.reads.load(Ordering::SeqCst), 3);
    let event = nostr::EventBuilder::new(nostr::Kind::TextNote, "x")
        .sign_with_keys(&relay.keys)
        .expect("event");
    assert!(cache.submit_event(event).await.is_err());
    assert!(cache.scoped(Keys::generate(), String::new()).is_err());
}
