CREATE TABLE principals (
  id INTEGER PRIMARY KEY,
  account_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  name TEXT NOT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE access_keys (
  id INTEGER PRIMARY KEY,
  key_id TEXT NOT NULL UNIQUE,
  secret_key TEXT NOT NULL,
  principal_id INTEGER NOT NULL REFERENCES principals(id) ON DELETE CASCADE,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO principals (account_id, kind, name)
  VALUES (
    '000000000000',
    'user',
    'test'
  );

INSERT INTO access_keys (key_id, secret_key, principal_id)
  VALUES (
    'test',
    'test',
    (SELECT id FROM principals WHERE account_id = '000000000000' AND kind = 'user' AND name = 'test')
  );
