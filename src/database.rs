use sqlx::{sqlite::SqlitePool, Pool, Sqlite};
use anyhow::{Context, Result};
use std::path::PathBuf;

const MAX_FILES_DEFAULT: usize = 10;
const EXPIRATION_DAYS_DEFAULT: i64 = 7;

#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub path: PathBuf,
    pub max_files_per_session: usize,
    pub expiration_days: i64,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::from(
                std::env::var("OXDRAW_DB_PATH")
                    .unwrap_or_else(|_| "oxdraw.db".to_string())
            ),
            max_files_per_session: std::env::var("OXDRAW_MAX_FILES")
                .unwrap_or_else(|_| "10".to_string())
                .parse()
                .unwrap_or(MAX_FILES_DEFAULT),
            expiration_days: std::env::var("OXDRAW_EXPIRATION_DAYS")
                .unwrap_or_else(|_| "7".to_string())
                .parse()
                .unwrap_or(EXPIRATION_DAYS_DEFAULT),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Database {
    pool: Pool<Sqlite>,
    config: DatabaseConfig,
}

impl Database {
    pub async fn new(config: DatabaseConfig) -> Result<Self> {
        let db_url = if config.path.is_absolute() {
            format!("sqlite:///{}", config.path.display())
        } else {
            format!("sqlite:{}", config.path.display())
        };
        let pool = SqlitePool::connect(&db_url)
            .await
            .context("Failed to connect to SQLite database")?;

        let db = Self {
            pool,
            config: config.clone(),
        };

        db.run_migrations().await?;
        Ok(db)
    }

    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }

    pub fn config(&self) -> &DatabaseConfig {
        &self.config
    }

    async fn run_migrations(&self) -> Result<()> {
        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                last_activity_at TEXT NOT NULL DEFAULT (datetime('now'))
            )
        "#)
        .execute(&self.pool)
        .await
        .context("Failed to create sessions table")?;

        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS diagrams (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                name TEXT NOT NULL,
                filename TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                is_deleted INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
            )
        "#)
        .execute(&self.pool)
        .await
        .context("Failed to create diagrams table")?;

        sqlx::query(r#"CREATE INDEX IF NOT EXISTS idx_diagrams_session ON diagrams(session_id, updated_at DESC)"#)
        .execute(&self.pool)
        .await
        .context("Failed to create diagrams_session index")?;

        sqlx::query(r#"CREATE INDEX IF NOT EXISTS idx_diagrams_expire ON diagrams(updated_at)"#)
        .execute(&self.pool)
        .await
        .context("Failed to create diagrams_expire index")?;

        Ok(())
    }

    pub async fn cleanup_expired(&self) -> Result<(u64, u64)> {
        let expiration_days = self.config.expiration_days;

        let diagrams_deleted = sqlx::query(
            r#"DELETE FROM diagrams WHERE updated_at < datetime('now', ?)"#,
        )
        .bind(format!("-{} days", expiration_days))
        .execute(&self.pool)
        .await
        .context("Failed to cleanup expired diagrams")?
        .rows_affected();

        let sessions_deleted = sqlx::query(
            r#"DELETE FROM sessions WHERE id NOT IN (SELECT DISTINCT session_id FROM diagrams)
               AND last_activity_at < datetime('now', ?)"#,
        )
        .bind(format!("-{} days", expiration_days))
        .execute(&self.pool)
        .await
        .context("Failed to cleanup orphaned sessions")?
        .rows_affected();

        Ok((diagrams_deleted, sessions_deleted))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_database_creation() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let config = DatabaseConfig {
            path: db_path,
            max_files_per_session: 10,
            expiration_days: 7,
        };

        let db = Database::new(config).await.unwrap();
        let _: i32 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
            .fetch_one(db.pool())
            .await
            .unwrap();
        let _: i32 = sqlx::query_scalar("SELECT COUNT(*) FROM diagrams")
            .fetch_one(db.pool())
            .await
            .unwrap();
    }
}
