# Application events and webhook plugins

Beekeeper reserves persistent, nonreplaceable Nostr kind `50000` for
application-defined data. It uses ordinary signed NIP-01 `EVENT`, `REQ`, and
`COUNT` frames. New application types and payload versions require no relay
kind registration.

## Event envelope

```json
{
  "kind": 50000,
  "tags": [
    ["h", "c42dcf94-1a36-4ce5-b663-5903aa10fbdb"],
    ["L", "org.agiterra.application-event"],
    ["l", "org.example.build.completed", "org.agiterra.application-event"]
  ],
  "content": "{\"schema\":1,\"data\":{\"buildId\":\"b-123\"}}"
}
```

Exactly one `h`, `L`, and `l` tag is required. The `h` value is a channel UUID.
The `L` value and the third `l` value must equal
`org.agiterra.application-event`. The application type in `l` is a lowercase
reverse-domain name with at least three dot-separated components, at most 128
bytes total, and at most 63 bytes per component. Components contain lowercase
ASCII letters, digits, and internal hyphens. The content is a JSON object with
a positive integer `schema` and object `data`.

Ingest requires `MessagesWrite` and uses the existing event signature,
timestamp, size, host/community, token, channel membership, and archived
channel checks. Unknown event kinds remain denied. The relay stores and fans
out application events without executing their content. Type prefixes are a
naming convention, not proof of ownership: consumers must check the signed
author key before acting on data.

For historical reads, use `{"kinds":[50000],"#h":["<channel UUID>"],
"#l":["org.example.build.completed"]}`. The relay pushes the type predicate
into the SQL JSONB tag query before pagination for `REQ` and exact `COUNT`.
Live subscriptions use standard NIP-01 filter matching.

## Webhook plugin surface

Compiled-in Rust plugins register an Axum router under
`/webhooks/<plugin_name>/`. Names are lowercase ASCII letters, digits, or
internal hyphens, at most 63 bytes. Registration rejects invalid and duplicate
names. The relay resolves the request Host to a community, limits the raw body
to 1 MiB, and calls the plugin's `WebhookVerifier` with the original headers
and body before running its handler. The handler receives the resolved
`TenantContext` as an Axum extension. An unknown Host fails closed.

The plugin owns vendor routes and signature algorithms. This namespace is
separate from the secret-authenticated workflow route `/hooks/{id}`. A plugin
that emits a Beekeeper event must separately provide a signing identity,
destination channel, and authorization rule. A vendor signature authenticates
an HTTP request; it does not authorize a Beekeeper user event. No vendor plugin
or webhook-to-event bridge is installed by this protocol addition.
