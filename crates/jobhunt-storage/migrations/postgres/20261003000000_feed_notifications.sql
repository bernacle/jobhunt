-- JobHunt web and notifications (BRU-295).
--
-- * The Today feed: when a recommendation was resurfaced because it
--   changed materially, and when the person put it aside ("not now"), on
--   the per-person search state BRU-294 introduced.
-- * Email notifications: the person's settings (the address encrypted
--   like every other private value), an outbox of deliveries, and one row
--   per opportunity ever put in a notification, so an opportunity is
--   emailed to a person at most once, whatever retries or concurrent
--   workers do.
-- * The notification worker's runs, next to discovery and verification.

ALTER TABLE user_opportunities
    ADD COLUMN resurfaced_at timestamptz,
    ADD COLUMN dismissed_at  timestamptz;
-- A row may now record only that the person put an opportunity aside
-- (times_shown 0, no tier).
ALTER TABLE user_opportunities ALTER COLUMN last_tier DROP NOT NULL;

ALTER TABLE worker_runs DROP CONSTRAINT worker_runs_kind_check;
ALTER TABLE worker_runs ADD CONSTRAINT worker_runs_kind_check
    CHECK (kind IN ('discovery', 'verification', 'notification'));

-- One row per account that has touched its notification settings.
CREATE TABLE notification_settings (
    user_id            text COLLATE "C" PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    email_enabled      boolean NOT NULL DEFAULT false,
    -- 'immediate' (soon after a strong match appears, at most one email
    -- every few hours) or 'daily' (at most one a day).
    cadence            text NOT NULL DEFAULT 'immediate'
                       CHECK (cadence IN ('immediate', 'daily')),
    -- The address, sealed (AES-256-GCM, see postgres::crypto).
    email              bytea,
    -- When the address was confirmed through the link sent to it.
    email_confirmed_at timestamptz,
    -- SHA-256 of the pending confirmation token, and when it was sent.
    confirm_token_hash bytea,
    confirm_sent_at    timestamptz,
    -- A notification worker composing for this account (a lease).
    lease_owner        text,
    lease_expires_at   timestamptz,
    created_at         timestamptz NOT NULL,
    updated_at         timestamptz NOT NULL
);

CREATE INDEX notification_settings_enabled
    ON notification_settings (user_id) WHERE email_enabled AND email_confirmed_at IS NOT NULL;

-- The outbox. A delivery is written (with its rendered message, sealed)
-- before the provider is asked to send it, and sent with its id as the
-- provider's idempotency key, so a retry after a crash does not send it
-- twice. It is 'sent' only once the provider accepted it.
CREATE TABLE notification_deliveries (
    id                   text COLLATE "C" PRIMARY KEY,
    -- Order of creation; the account's notification cursor
    -- (user_state.notification_cursor) is the seq of its last sent one.
    seq                  bigint GENERATED ALWAYS AS IDENTITY UNIQUE,
    user_id              text COLLATE "C" NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    kind                 text NOT NULL CHECK (kind IN ('recommendations', 'confirmation')),
    -- pending: not accepted by the provider yet (retried);
    -- sent: accepted; failed: gave up after repeated failures;
    -- abandoned: outcome unknown and too old to retry safely.
    status               text NOT NULL CHECK (status IN ('pending', 'sent', 'failed', 'abandoned')),
    attempts             integer NOT NULL DEFAULT 0,
    next_attempt_at      timestamptz NOT NULL,
    lease_owner          text,
    lease_expires_at     timestamptz,
    created_at           timestamptz NOT NULL,
    sent_at              timestamptz,
    provider             text,
    provider_message_id  text,
    last_error           text,
    items                integer NOT NULL DEFAULT 0,
    message              bytea NOT NULL
);

CREATE INDEX notification_deliveries_due
    ON notification_deliveries (next_attempt_at) WHERE status = 'pending';
CREATE INDEX notification_deliveries_by_user
    ON notification_deliveries (user_id, created_at DESC);

-- Every opportunity ever included in a person's notification, with the
-- ranking that justified it. The primary key is the idempotency rule.
CREATE TABLE notification_items (
    user_id          text COLLATE "C" NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    opportunity_id   text COLLATE "C" NOT NULL,
    delivery_id      text COLLATE "C" NOT NULL
                     REFERENCES notification_deliveries (id) ON DELETE CASCADE,
    tier             text NOT NULL
                     CHECK (tier IN ('strong_fit', 'worth_reviewing', 'maybe', 'low_priority')),
    -- The job record and content version the recommendation rested on.
    job_id           text COLLATE "C" NOT NULL,
    content_version  text NOT NULL,
    ranking_version  text NOT NULL,
    notified_at      timestamptz NOT NULL,
    PRIMARY KEY (user_id, opportunity_id)
);

CREATE INDEX notification_items_by_delivery ON notification_items (delivery_id);
