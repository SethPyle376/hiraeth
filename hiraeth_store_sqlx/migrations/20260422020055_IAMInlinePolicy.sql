CREATE TABLE iam_principal_inline_policies (
  id INTEGER PRIMARY KEY,
  principal_id INTEGER NOT NULL REFERENCES iam_principals(id) ON DELETE CASCADE,
  policy_name TEXT NOT NULL,
  policy_document TEXT NOT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(principal_id, policy_name)
);

CREATE INDEX iam_principal_inline_policies_principal_id_idx
  ON iam_principal_inline_policies(principal_id);

INSERT INTO iam_principal_inline_policies (principal_id, policy_name, policy_document)
  VALUES (
    (SELECT id FROM iam_principals WHERE account_id = '000000000000' AND kind = 'user' AND name = 'test'),
    'default-account-admin',
    '{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Action":"*","Resource":"arn:aws:*:*:000000000000:*"}]}'
  );
