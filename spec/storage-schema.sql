-- Canonical storage schema for Harness.
--
-- This file is the spec; the first SQLx migration at
-- backend/crates/harness-storage/migrations/0001_init.sql must match it
-- byte-for-byte (modulo SQL comments). See PLAN.md §5.3 and §5.4.
--
-- All timestamps are seconds since the Unix epoch, stored as INTEGER.
-- All "*_json" columns are BLOB so we can transparently switch to encrypted
-- payloads for sensitive entries (notably providers_config.config_json) without
-- a schema change.

PRAGMA foreign_keys = ON;

-- Sandbox templates must exist before conversations, since conversations.
-- sandbox_template_id has a FK into this table.
CREATE TABLE IF NOT EXISTS sandbox_templates (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  description TEXT,
  profile TEXT NOT NULL,             -- SBPL profile text
  is_builtin INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS providers_config (
  provider_id TEXT PRIMARY KEY,
  config_json BLOB NOT NULL,         -- encrypted at rest (see security spec)
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
  role TEXT NOT NULL,                -- 'user' | 'assistant' | 'tool'
  content_json BLOB NOT NULL,        -- structured: text blocks, tool_use, tool_result
  created_at INTEGER NOT NULL,
  ordinal INTEGER NOT NULL           -- monotonically increasing within conversation
);

CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value_json BLOB NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_messages_conv_ord
  ON messages(conversation_id, ordinal);
