CREATE TABLE sns_topics (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    region TEXT NOT NULL,
    account_id TEXT NOT NULL,
    display_name TEXT,
    policy TEXT NOT NULL DEFAULT '{}',
    delivery_policy TEXT,
    fifo_topic TEXT,
    signature_version TEXT,
    tracing_config TEXT,
    kms_master_key_id TEXT,
    data_protection_policy TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(name, region, account_id)
);

CREATE INDEX sns_topics_account_id_region_idx ON sns_topics(account_id, region);
