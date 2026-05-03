CREATE TABLE sns_topics (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    region TEXT NOT NULL,
    account_id TEXT NOT NULL,
    display_name TEXT NOT NULL DEFAULT '',
    policy TEXT NOT NULL DEFAULT '{}',
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(name, region, account_id)
);

CREATE INDEX sns_topics_account_id_region_idx ON sns_topics(account_id, region);
