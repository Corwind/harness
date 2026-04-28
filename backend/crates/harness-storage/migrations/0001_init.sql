-- 0001_init.sql — initial Harness schema.
-- Mirrors spec/storage-schema.sql. Keep the two in sync.

CREATE TABLE IF NOT EXISTS sandbox_templates (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  description TEXT,
  profile TEXT NOT NULL,
  is_builtin INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS providers_config (
  provider_id TEXT PRIMARY KEY,
  config_json BLOB NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS conversations (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  provider_id TEXT NOT NULL,
  model TEXT NOT NULL,
  sandbox_template_id TEXT REFERENCES sandbox_templates(id) ON DELETE SET NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS messages (
  id TEXT PRIMARY KEY,
  conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  role TEXT NOT NULL,
  content_json BLOB NOT NULL,
  created_at INTEGER NOT NULL,
  ordinal INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value_json BLOB NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_messages_conv_ord
  ON messages(conversation_id, ordinal);
