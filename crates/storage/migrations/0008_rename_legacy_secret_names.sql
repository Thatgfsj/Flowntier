-- v0.4.30: rename secret_name for the 9 built-in providers from
-- `*_API_KEY` (which collided with shell env var naming conventions)
-- to `flowntier/<id>` (internal namespace, unambiguous).
--
-- Chairman's complaint (audit 000130): the old names like
-- `MINIMAX_API_KEY` looked like shell env vars — even though
-- Flowntier ONLY reads from the keychain SQLite table, the
-- matching name made the chairman worry that an old env var
-- was being picked up. The new names put every Flowntier
-- secret in a clearly internal namespace so there's zero
-- possibility of confusion.
--
-- Idempotent: each UPDATE is wrapped in a WHERE clause that
-- only matches the OLD name. Re-running this migration on a DB
-- where the rename already happened is a no-op.
--
-- Old name → new name map (must match the preset table in
-- crates/pipe-server/src/providers.rs::PRESETS):
--
--   OPENAI_API_KEY      → flowntier/openai
--   ANTHROPIC_API_KEY   → flowntier/anthropic
--   GOOGLE_API_KEY      → flowntier/google
--   DEEPSEEK_API_KEY    → flowntier/deepseek
--   MINIMAX_API_KEY     → flowntier/minimax
--   MOONSHOT_API_KEY    → flowntier/kimi
--   GLM_API_KEY         → flowntier/glm
--   MIMO_API_KEY        → flowntier/mimo
--   SILICONFLOW_API_KEY → flowntier/siliconflow

UPDATE secret SET name = 'flowntier/openai'       WHERE name = 'OPENAI_API_KEY';
UPDATE secret SET name = 'flowntier/anthropic'    WHERE name = 'ANTHROPIC_API_KEY';
UPDATE secret SET name = 'flowntier/google'       WHERE name = 'GOOGLE_API_KEY';
UPDATE secret SET name = 'flowntier/deepseek'     WHERE name = 'DEEPSEEK_API_KEY';
UPDATE secret SET name = 'flowntier/minimax'      WHERE name = 'MINIMAX_API_KEY';
UPDATE secret SET name = 'flowntier/kimi'         WHERE name = 'MOONSHOT_API_KEY';
UPDATE secret SET name = 'flowntier/glm'          WHERE name = 'GLM_API_KEY';
UPDATE secret SET name = 'flowntier/mimo'         WHERE name = 'MIMO_API_KEY';
UPDATE secret SET name = 'flowntier/siliconflow'  WHERE name = 'SILICONFLOW_API_KEY';

-- Custom providers (Settings → 中转站) stored their API key
-- under `CUSTOM_PROVIDER_KEY_<id>`. We can't enumerate every
-- possible id, so use a SQL LIKE pattern. Migration is
-- idempotent because once renamed, the row no longer matches
-- the old prefix.
UPDATE secret
   SET name = 'flowntier/custom/' || SUBSTR(name, LENGTH('CUSTOM_PROVIDER_KEY_') + 1)
 WHERE name LIKE 'CUSTOM_PROVIDER_KEY_%';