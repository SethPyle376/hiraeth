CREATE TABLE iam_managed_policies (
  id INTEGER PRIMARY KEY NOT NULL,
  policy_id TEXT NOT NULL UNIQUE,
  account_id TEXT NOT NULL,
  policy_name TEXT NOT NULL,
  policy_path TEXT NOT NULL DEFAULT '/',
  policy_document TEXT NOT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(account_id, policy_name)
);

CREATE INDEX iam_managed_policies_id_idx
  ON iam_managed_policies(id);

