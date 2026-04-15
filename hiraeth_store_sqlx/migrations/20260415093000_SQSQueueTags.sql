CREATE TABLE sqs_queue_tags (
  queue_id INTEGER NOT NULL REFERENCES sqs_queues(id) ON DELETE CASCADE,
  tag_key TEXT NOT NULL,
  tag_value TEXT NOT NULL,
  PRIMARY KEY (queue_id, tag_key)
);
