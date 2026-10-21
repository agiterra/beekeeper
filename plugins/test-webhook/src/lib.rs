//! Test only: a `webhook-plugin@1` component. It behaves well by default and
//! misbehaves when the delivery asks it to, so the host's webhook-world
//! checks can be proven against a real component.
//!
//! - `authenticate` needs `x-signature` (hex), `x-delivery` and `x-event`; an
//!   `x-timestamp` header is passed through as the vendor timestamp, and
//!   `x-fault: spin` loops until the host stops it.
//! - `project` acts on the vendor event: `push` accepts `com.example.push`,
//!   `ping` ignores, `bad` rejects, and the rest each try one thing the
//!   contract forbids (`forge-*`, `wrong-prefix`, `not-object`, `spin`).
//! - `validate-settings` refuses settings with `"bad": true`.

wit_bindgen::generate!({
    path: "../../wit/plugin",
    world: "webhook-plugin",
});

use beekeeper::plugin::{log, webhook_types as types};
use exports::beekeeper::plugin::webhook::Guest;
use serde_json::json;

struct Webhook;

fn header<'a>(headers: &'a [types::Header], name: &str) -> Option<&'a [u8]> {
    headers
        .iter()
        .find(|h| h.name == name)
        .map(|h| h.value.as_slice())
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn unhex(text: &[u8]) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    text.chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
        .collect()
}

fn spin() -> ! {
    let mut n: u64 = 0;
    loop {
        n = std::hint::black_box(n.wrapping_add(1));
    }
}

fn event(event_type: &str, tags: Vec<Vec<String>>, data: String) -> types::Disposition {
    types::Disposition::Accept(types::AppEvent {
        event_type: event_type.to_owned(),
        schema: 1,
        data_json: data,
        tags,
    })
}

fn tag(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| (*v).to_owned()).collect()
}

impl Guest for Webhook {
    fn describe() -> types::WebhookManifest {
        types::WebhookManifest {
            name: "test-webhook".to_owned(),
            version: "0.1.0".to_owned(),
            type_prefix: "com.example".to_owned(),
            max_body_bytes: 65_536,
        }
    }

    fn validate_settings(settings_json: String) -> Result<(), String> {
        let settings: serde_json::Value =
            serde_json::from_str(&settings_json).map_err(|e| e.to_string())?;
        if settings["bad"] == json!(true) {
            return Err("settings say bad".to_owned());
        }
        Ok(())
    }

    fn authenticate(headers: Vec<types::Header>) -> Result<types::AuthPlan, types::AuthFailure> {
        if header(&headers, "x-fault") == Some(b"spin") {
            spin();
        }
        let signature = header(&headers, "x-signature")
            .ok_or_else(|| types::AuthFailure::MissingHeader("x-signature".to_owned()))?;
        let signature = unhex(signature)
            .ok_or_else(|| types::AuthFailure::Malformed("x-signature is not hex".to_owned()))?;
        let delivery_id = header(&headers, "x-delivery")
            .ok_or_else(|| types::AuthFailure::MissingHeader("x-delivery".to_owned()))?;
        let vendor_event = header(&headers, "x-event")
            .ok_or_else(|| types::AuthFailure::MissingHeader("x-event".to_owned()))?;
        let timestamp = header(&headers, "x-timestamp").and_then(|t| text(t).parse().ok());
        Ok(types::AuthPlan {
            algorithm: types::MacAlgorithm::HmacSha256,
            signature,
            signed_prefix: Vec::new(),
            timestamp,
            delivery_id: text(delivery_id),
            vendor_event: text(vendor_event),
        })
    }

    fn project(
        settings_json: String,
        vendor_event: String,
        _headers: Vec<types::Header>,
        body: Vec<u8>,
    ) -> types::Disposition {
        log::log(log::Level::Info, &format!("projecting {vendor_event}"));
        let settings: serde_json::Value = serde_json::from_str(&settings_json).unwrap_or_default();
        let data = json!({ "bytes": body.len(), "body": text(&body) }).to_string();
        match vendor_event.as_str() {
            "push" => event("com.example.push", Vec::new(), data),
            "echo-channel" => {
                let channel = settings["channel"].as_str().unwrap_or("");
                event("com.example.push", vec![tag(&["h", channel])], data)
            }
            "ping" => types::Disposition::Ignore("ping".to_owned()),
            "bad" => types::Disposition::Reject("cannot use this".to_owned()),
            "forge-h" => event(
                "com.example.push",
                vec![tag(&["h", "00000000-0000-4000-8000-000000000000"])],
                data,
            ),
            "forge-p" => event(
                "com.example.push",
                vec![tag(&[
                    "p",
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                ])],
                data,
            ),
            "forge-l" => event(
                "com.example.push",
                vec![tag(&[
                    "l",
                    "com.example.admin",
                    "org.agiterra.application-event",
                ])],
                data,
            ),
            "wrong-prefix" => event("com.github.push", Vec::new(), data),
            "not-object" => event("com.example.push", Vec::new(), "[1]".to_owned()),
            "spin" => spin(),
            other => types::Disposition::Reject(format!("unknown event {other}")),
        }
    }
}

export!(Webhook);
