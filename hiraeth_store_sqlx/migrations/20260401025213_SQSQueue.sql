CREATE TABLE sqs_queues (
  id INTEGER PRIMARY KEY NOT NULL,
  name TEXT NOT NULL,
  region TEXT NOT NULL,
  account_id TEXT NOT NULL,
  queue_type TEXT NOT NULL,
  visibility_timeout_seconds INTEGER NOT NULL,
  delay_seconds INTEGER NOT NULL,
  message_retention_period_seconds INTEGER NOT NULL,
  receive_message_wait_time_seconds INTEGER NOT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(name, region, account_id)
);
