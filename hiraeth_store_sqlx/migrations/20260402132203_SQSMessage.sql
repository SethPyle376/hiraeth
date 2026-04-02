CREATE TABLE sqs_messages (
  id INTEGER PRIMARY KEY,
  queue_id INTEGER NOT NULL REFERENCES sqs_queues(id) ON DELETE CASCADE,
  message_id TEXT NOT NULL UNIQUE,
  body TEXT NOT NULL,
  message_attributes TEXT NOT NULL DEFAULT '{}',
  sent_at DATETIME NOT NULL,
  visible_at DATETIME NOT NULL,
  expires_at DATETIME NOT NULL,
  receive_count INTEGER NOT NULL DEFAULT 0,
  receipt_handle TEXT,
  first_received_at DATETIME,
  message_group_id TEXT,
  message_deduplication_id TEXT
);
