CREATE TABLE iam_user_policy_attachments (
  id INTEGER PRIMARY KEY NOT NULL,
  user_id INTEGER NOT NULL REFERENCES iam_principals(id) ON DELETE CASCADE,
  policy_id INTEGER NOT NULL REFERENCES iam_managed_policies(id) ON DELETE CASCADE,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(user_id, policy_id)
);

CREATE INDEX iam_user_policy_attachments_user_id_idx
  ON iam_user_policy_attachments(user_id, policy_id);
