-- Vendor webhook ingress (GitHub-plugin slice, plan § 4, finding F3).
--
-- `vendor_installations`: one row per (community, plugin, channel) webhook
-- installation. The webhook secret is stored in plaintext, following the
-- workflow webhook-secret precedent (operator ruling ASK-201: no encryption
-- at rest in slice 1). Unauthenticated rejections are bounded counters on
-- this row, never one insert per request (a write-amplification vector).
--
-- `vendor_deliveries`: one row per AUTHENTICATED delivery, keyed by the
-- vendor's delivery id. Only a request whose signature verified may occupy
-- the key (F3): otherwise anyone who guessed a delivery GUID could
-- pre-poison it and the genuine delivery would be dropped as a duplicate.
-- An accepted row is written in the same transaction as its kind:50000
-- event (and therefore the event's workflow_dispatch_queue row); event_id is
-- set iff the delivery was accepted. A `rejected` row may be re-projected
-- (overwritten) by a later redelivery; `accepted` and `ignored` rows are
-- final and make a redelivery a duplicate.
--
-- Additive migration: previously applied files must not change checksum.

CREATE TABLE vendor_installations (
    community_id         UUID NOT NULL REFERENCES communities(id),
    id                   UUID NOT NULL DEFAULT gen_random_uuid(),
    plugin               TEXT NOT NULL CHECK (length(plugin) BETWEEN 1 AND 64),
    channel_id           UUID NOT NULL,
    created_by           BYTEA NOT NULL,
    secret               BYTEA NOT NULL CHECK (length(secret) >= 16),
    settings             JSONB NOT NULL DEFAULT '{}'::jsonb
                             CHECK (jsonb_typeof(settings) = 'object'),
    enabled              BOOLEAN NOT NULL DEFAULT true,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    disabled_at          TIMESTAMPTZ,
    rejected_count       BIGINT NOT NULL DEFAULT 0 CHECK (rejected_count >= 0),
    last_rejected_at     TIMESTAMPTZ,
    last_rejected_reason TEXT,
    error_count          BIGINT NOT NULL DEFAULT 0 CHECK (error_count >= 0),
    last_error_at        TIMESTAMPTZ,
    last_error           TEXT,
    last_accepted_at     TIMESTAMPTZ,
    PRIMARY KEY (community_id, id),
    FOREIGN KEY (community_id, channel_id) REFERENCES channels (community_id, id),
    FOREIGN KEY (community_id, created_by) REFERENCES users (community_id, pubkey)
);
CREATE INDEX vendor_installations_channel
    ON vendor_installations (community_id, channel_id);

CREATE TABLE vendor_deliveries (
    community_id    UUID NOT NULL REFERENCES communities(id),
    installation_id UUID NOT NULL,
    delivery_id     TEXT NOT NULL CHECK (length(delivery_id) BETWEEN 1 AND 200),
    disposition     TEXT NOT NULL CHECK (disposition IN ('accepted', 'ignored', 'rejected')),
    vendor_event    TEXT NOT NULL CHECK (length(vendor_event) <= 200),
    reason          TEXT,
    event_id        BYTEA CHECK (event_id IS NULL OR length(event_id) = 32),
    plugin_digest   BYTEA NOT NULL CHECK (length(plugin_digest) = 32),
    received_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, installation_id, delivery_id),
    FOREIGN KEY (community_id, installation_id)
        REFERENCES vendor_installations (community_id, id),
    CHECK ((disposition = 'accepted') = (event_id IS NOT NULL))
);
CREATE INDEX vendor_deliveries_received
    ON vendor_deliveries (community_id, installation_id, received_at);
CREATE INDEX vendor_deliveries_retention
    ON vendor_deliveries (received_at);

SELECT attach_community_write_fence('vendor_installations');
SELECT attach_community_write_fence('vendor_deliveries');
