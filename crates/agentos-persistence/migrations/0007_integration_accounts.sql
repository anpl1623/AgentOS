-- An account of an integration, bound by the operator.
--
-- The token is NOT here. It is a network credential like any other, stored in
-- the secret store under the origin of `host` with `label` as its name, and
-- this table holds only what points at it: the same split the settings table
-- observes for provider keys. Deleting a row does not delete the credential;
-- the runtime does that, after the row is gone, so that a crash between the
-- two leaves an orphan secret rather than a row pointing at nothing.
--
-- There is no update path. `host` decides where every request for the account
-- goes and `private_network` decides which addresses it may reach, and both
-- are the operator's to set when binding. Changing either is an unbind and a
-- bind, each recorded in the audit chain.
CREATE TABLE integration_accounts (
    id              TEXT PRIMARY KEY NOT NULL,
    -- `github`, ...
    integration     TEXT NOT NULL
                    CHECK (length(integration) BETWEEN 1 AND 32
                           AND integration NOT GLOB '*[^a-z0-9-]*'),
    -- The operator's name for the account, and its credential's name.
    label           TEXT NOT NULL
                    CHECK (length(label) BETWEEN 1 AND 32 AND label NOT GLOB '*[^a-z0-9-]*'),
    -- The API base URL, e.g. https://api.github.com or https://ghe.example/api/v3.
    -- It comes from here and never from a tool argument.
    host            TEXT NOT NULL CHECK (host GLOB 'https://*' OR host GLOB 'http://*'),
    -- 1 when the operator allowed the account to reach private-network
    -- addresses, for a self-hosted server. Never loopback or link-local.
    private_network INTEGER NOT NULL DEFAULT 0 CHECK (private_network IN (0, 1)),
    -- What the operator says the token can do. A note, not a control: what an
    -- agent may do with the account is the policy's to say.
    scopes          TEXT,
    created_at      TEXT NOT NULL,
    last_used_at    TEXT,
    UNIQUE (integration, label)
);

CREATE INDEX idx_integration_accounts_kind ON integration_accounts(integration, created_at DESC);
