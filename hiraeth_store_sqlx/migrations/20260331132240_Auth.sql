CREATE TABLE iam_principals (
  id INTEGER PRIMARY KEY,
  account_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  name TEXT NOT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(account_id, kind, name)
);

CREATE TABLE iam_access_keys (
  id INTEGER PRIMARY KEY,
  key_id TEXT NOT NULL UNIQUE,
  secret_key TEXT NOT NULL,
  principal_id INTEGER NOT NULL REFERENCES iam_principals(id) ON DELETE CASCADE,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX iam_access_keys_principal_id_idx
  ON iam_access_keys(principal_id);

INSERT INTO iam_principals (account_id, kind, name)
  VALUES (
    '000000000000',
    'user',
    'test'
  );

INSERT INTO iam_access_keys (key_id, secret_key, principal_id)
  VALUES (
    'test',
    'test',
    (SELECT id FROM iam_principals WHERE account_id = '000000000000' AND kind = 'user' AND name = 'test')
  );
