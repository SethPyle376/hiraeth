CREATE TABLE sns_subscriptions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    topic_arn TEXT NOT NULL,
    protocol TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    owner_account_id TEXT NOT NULL,
    subscription_arn TEXT NOT NULL UNIQUE,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX sns_subscriptions_topic_arn_idx ON sns_subscriptions(topic_arn);
