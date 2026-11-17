-- Derived from Autopush 1.86.0 autopush-common/src/db/postgres/schema.psql.
-- MPL-2.0. Fresh, fixture-owned databases ONLY; never a migration or reset.
\set ON_ERROR_STOP on
BEGIN;
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM pg_tables
        WHERE schemaname = current_schema()
    ) THEN
        RAISE EXCEPTION 'Refusing to initialize a nonempty schema';
    END IF;
END $$;

CREATE TABLE router (
    uaid varchar(40) NOT NULL PRIMARY KEY UNIQUE,
    connected_at bigint,
    router_type varchar(100),
    router_data text,
    last_connect bigint,
    node_id varchar(100),
    record_version bigint,
    version text,
    last_update bigint,
    expiry bigint,
    priv_channels varchar(40)[]
);

CREATE TABLE meta (
    channel_id varchar(40) NOT NULL,
    uaid varchar(40) NOT NULL REFERENCES router ON DELETE cascade,
    FOREIGN KEY(uaid) REFERENCES router (uaid),
    PRIMARY KEY (uaid, channel_id)
);

CREATE TABLE reliability (
    id varchar(40) NOT NULL PRIMARY KEY,
    states jsonb,
    last_update_timestamp bigint
);

CREATE TABLE message (
    uaid varchar(40) NOT NULL REFERENCES router ON DELETE cascade,
    channel_id varchar(40),
    chid_message_id varchar(100),
    version text,
    ttl bigint,
    expiry bigint,
    topic varchar,
    timestamp bigint,
    data text,
    sortkey_timestamp bigint,
    headers text,
    reliability_id varchar(40) REFERENCES reliability(id),
    FOREIGN KEY(uaid) REFERENCES router(uaid),
    PRIMARY KEY(uaid, chid_message_id)
);

CREATE UNIQUE INDEX chid_message_idx ON message (chid_message_id);
CREATE INDEX message_reliability_id_idx ON message (reliability_id);
CREATE INDEX meta_uaid_idx ON meta (uaid);
CREATE INDEX update_idx ON reliability(last_update_timestamp);
COMMIT;
