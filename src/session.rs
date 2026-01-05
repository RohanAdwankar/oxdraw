use sqlx::SqlitePool;
use anyhow::{Context, Result};
use uuid::Uuid;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub last_activity_at: DateTime<Utc>,
}

impl Session {
    pub async fn create(pool: &SqlitePool) -> Result<Self> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();

        sqlx::query(
            "INSERT INTO sessions (id, created_at, last_activity_at) VALUES (?, ?, ?)",
        )
        .bind(&id)
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(pool)
        .await
        .context("Failed to create session")?;

        Ok(Self {
            id,
            created_at: now,
            last_activity_at: now,
        })
    }

    pub async fn get_by_id(pool: &SqlitePool, id: &str) -> Result<Option<Self>> {
        let row: Option<SessionRow> = sqlx::query_as(
            "SELECT id, created_at, last_activity_at FROM sessions WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("Failed to get session")?;

        Ok(row.map(|r| Self {
            id: r.id,
            created_at: r.created_at.parse().unwrap_or_else(|_| Utc::now()),
            last_activity_at: r.last_activity_at.parse().unwrap_or_else(|_| Utc::now()),
        }))
    }

    pub async fn touch(&self, pool: &SqlitePool) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "UPDATE sessions SET last_activity_at = ? WHERE id = ?",
        )
        .bind(now.to_rfc3339())
        .bind(&self.id)
        .execute(pool)
        .await
        .context("Failed to update session activity")?;
        Ok(())
    }

    pub async fn delete(&self, pool: &SqlitePool) -> Result<()> {
        sqlx::query(
            "DELETE FROM sessions WHERE id = ?",
        )
        .bind(&self.id)
        .execute(pool)
        .await
        .context("Failed to delete session")?;
        Ok(())
    }
}

#[derive(sqlx::FromRow)]
struct SessionRow {
    id: String,
    created_at: String,
    last_activity_at: String,
}

pub fn create_session_cookie(session_id: &str) -> String {
    format!("oxdraw_session={}; Path=/; HttpOnly; SameSite=Lax; Max-Age=2592000", session_id)
}

pub fn clear_session_cookie() -> String {
    "oxdraw_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use sqlx::SqlitePool;

    #[tokio::test]
    async fn test_session_lifecycle() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let pool = SqlitePool::connect(&format!("sqlite://{}", db_path.display()))
            .await
            .unwrap();

        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                last_activity_at TEXT NOT NULL DEFAULT (datetime('now'))
            )
        "#).execute(&pool).await.unwrap();

        let session = Session::create(&pool).await.unwrap();
        assert!(!session.id.is_empty());

        let retrieved = Session::get_by_id(&pool, &session.id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, session.id);

        session.touch(&pool).await.unwrap();
        session.delete(&pool).await.unwrap();

        let after_delete = Session::get_by_id(&pool, &session.id).await.unwrap();
        assert!(after_delete.is_none());
    }
}
