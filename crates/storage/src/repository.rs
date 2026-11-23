//! Repository core: connection pool, migrations, generic kv.

use super::*;

/// Storage errors.
#[derive(Debug, Error)]
pub enum StorageError {
    /// SQLx error.
    #[error("sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),
    /// Migration error.
    #[error("migration: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    /// I/O error.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Repository wrapping a SQLite connection pool.
#[derive(Debug, Clone)]
pub struct Repository {
    pub(crate) pool: SqlitePool,
}

impl Repository {
    /// Open (or create) a SQLite database at the given path and run
    /// the embedded migrations.
    pub async fn open(path: &Path) -> Result<Self, StorageError> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
            .busy_timeout(std::time::Duration::from_secs(5))
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .connect_with(opts)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    /// Open an in-memory database (for tests).
    pub async fn open_in_memory() -> Result<Self, StorageError> {
        let opts = SqliteConnectOptions::new()
            .filename(":memory:")
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    /// Underlying pool. Use sparingly; prefer typed methods.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

impl Repository {
    /// Default location of the SQLite file.
    ///
    /// Honors the `aco` → `flowntier` rename (2026-06-24). On first
    /// call after upgrading, if a legacy `aco/` directory exists at
    /// the same parent and the new `flowntier/` directory does not
    /// yet, the legacy directory is renamed in place. The migration
    /// is best-effort and logged once; if the rename fails (perms,
    /// locked file, ...) we silently fall back to the new path.
    #[must_use]
    pub fn default_data_dir() -> Option<PathBuf> {
        let new = dirs::data_dir().map(|p| p.join("flowntier"));
        if let Some(dir) = &new {
            if !dir.exists() {
                if let Some(legacy) = dirs::data_dir().map(|p| p.join("aco")) {
                    if legacy.exists() {
                        match std::fs::rename(&legacy, dir) {
                            Ok(()) => eprintln!(
                                "[flowntier] migrated data dir: {} -> {}",
                                legacy.display(),
                                dir.display()
                            ),
                            Err(e) => eprintln!(
                                "[flowntier] could not migrate legacy data dir {} ({}); using fresh {}",
                                legacy.display(),
                                e,
                                dir.display()
                            ),
                        }
                    }
                }
            }
        }
        new
    }
}

impl Repository {
    // ── Generic kv ──────────────────────────────────────────────

    pub async fn kv_get(&self, k: &str) -> Result<Option<String>, StorageError> {
        let r: Option<(String,)> = sqlx::query_as("SELECT v FROM kv WHERE k = ?")
            .bind(k)
            .fetch_optional(&self.pool)
            .await?;
        Ok(r.map(|(v,)| v))
    }

    pub async fn kv_set(&self, k: &str, v: &str) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO kv (k, v, mtime) VALUES (?, ?, strftime('%s','now'))
             ON CONFLICT(k) DO UPDATE SET v=excluded.v, mtime=strftime('%s','now')",
        )
        .bind(k)
        .bind(v)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
