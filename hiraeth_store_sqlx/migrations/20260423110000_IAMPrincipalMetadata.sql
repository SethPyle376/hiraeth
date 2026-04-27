PRAGMA foreign_keys = OFF;

CREATE TEMP TABLE iam_access_keys_backup AS
SELECT id, key_id, secret_key, principal_id, created_at
FROM iam_access_keys;

CREATE TEMP TABLE iam_principal_inline_policies_backup AS
SELECT id, principal_id, policy_name, policy_document, created_at, updated_at
FROM iam_principal_inline_policies;

CREATE TABLE iam_principals_new (
  id INTEGER PRIMARY KEY,
  account_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  name TEXT NOT NULL,
  path TEXT NOT NULL,
  user_id TEXT NOT NULL UNIQUE,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(account_id, kind, name)
);

INSERT INTO iam_principals_new (id, account_id, kind, name, path, user_id, created_at)
SELECT
  id,
  account_id,
  kind,
  name,
  '/',
  'AIDA' || upper(hex(randomblob(8))),
  created_at
FROM iam_principals;

DROP TABLE iam_access_keys;
DROP TABLE iam_principal_inline_policies;
DROP TABLE iam_principals;
ALTER TABLE iam_principals_new RENAME TO iam_principals;

CREATE TABLE iam_access_keys (
  id INTEGER PRIMARY KEY,
  key_id TEXT NOT NULL UNIQUE,
  secret_key TEXT NOT NULL,
  principal_id INTEGER NOT NULL REFERENCES iam_principals(id) ON DELETE CASCADE,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO iam_access_keys (id, key_id, secret_key, principal_id, created_at)
SELECT id, key_id, secret_key, principal_id, created_at
FROM iam_access_keys_backup;

CREATE INDEX iam_access_keys_principal_id_idx
  ON iam_access_keys(principal_id);

CREATE TABLE iam_principal_inline_policies (
  id INTEGER PRIMARY KEY NOT NULL,
  principal_id INTEGER NOT NULL REFERENCES iam_principals(id) ON DELETE CASCADE,
  policy_name TEXT NOT NULL,
  policy_document TEXT NOT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(principal_id, policy_name)
);

INSERT INTO iam_principal_inline_policies (
  id,
  principal_id,
  policy_name,
  policy_document,
  created_at,
  updated_at
)
SELECT id, principal_id, policy_name, policy_document, created_at, updated_at
FROM iam_principal_inline_policies_backup;

CREATE INDEX iam_principal_inline_policies_principal_id_idx
  ON iam_principal_inline_policies(principal_id);

PRAGMA foreign_keys = ON;
