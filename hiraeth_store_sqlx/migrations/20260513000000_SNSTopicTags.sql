CREATE TABLE sns_topic_tags (
    topic_id INTEGER NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (topic_id, key),
    FOREIGN KEY (topic_id) REFERENCES sns_topics(id) ON DELETE CASCADE
);

CREATE INDEX sns_topic_tags_topic_id_idx ON sns_topic_tags(topic_id);
