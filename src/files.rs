use sqlx::SqlitePool;
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagramFile {
    pub id: i64,
    pub session_id: String,
    pub name: String,
    pub filename: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileListItem {
    pub id: i64,
    pub name: String,
    pub filename: String,
    pub updated_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub last_activity_at: DateTime<Utc>,
    pub file_count: i64,
    pub max_files: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileResponse {
    pub file: DiagramFile,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileListResponse {
    pub files: Vec<FileListItem>,
    pub max_files: usize,
    pub current_file_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFileRequest {
    pub name: String,
    pub template: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateFileRequest {
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateFileRequest {
    pub name: Option<String>,
}

#[derive(sqlx::FromRow)]
struct FileListRow {
    id: i64,
    name: String,
    filename: String,
    updated_at: String,
}

pub fn get_template(template: &str) -> &'static str {
    match template {
        "flowchart" | "graph" => "graph TD\n    Start([Start]) --> Process[Process]\n    Process --> End([End])",
        "sequence" => "sequenceDiagram\n    participant A as Alice\n    participant B as Bob\n    A->>B: Hello\n    B->>A: Hi!",
        "class" => "classDiagram\n    Animal <|-- Duck\n    Animal <|-- Fish\n    Animal : +int age\n    Animal : +String gender",
        "state" => "stateDiagram-v2\n    [*] --> Active\n    Active --> Inactive\n    Inactive --> Active",
        "er" => "erDiagram\n    CUSTOMER ||--o{ ORDER : places\n    CUSTOMER ||--|{ DELIVERY-ADDRESS : has",
        "journey" => "journey\n    My day\n    I wake up, 5, \"Happy\"\n    I eat breakfast, 5, \"Hungry\"\n    I go to work, 2, \"Sad\"",
        _ => "graph TD\n    A[Start] --> B[Process]\n    B --> C[End]",
    }
}

impl DiagramFile {
    pub async fn create(
        pool: &SqlitePool,
        session_id: &str,
        name: &str,
        template: Option<&str>,
    ) -> Result<Self> {
        let filename = if name.ends_with(".mmd") {
            name.to_string()
        } else {
            format!("{}.mmd", name)
        };

        let content = match template {
            Some(t) => get_template(t).to_string(),
            None => get_template("flowchart").to_string(),
        };

        let now = Utc::now();
        let id = sqlx::query(
            r#"INSERT INTO diagrams (session_id, name, filename, content, created_at, updated_at)
               VALUES (?, ?, ?, ?, ?, ?)"#,
        )
        .bind(session_id)
        .bind(name)
        .bind(&filename)
        .bind(&content)
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(pool)
        .await
        .context("Failed to create diagram")?
        .last_insert_rowid();

        Ok(Self {
            id,
            session_id: session_id.to_string(),
            name: name.to_string(),
            filename,
            content,
            created_at: now,
            updated_at: now,
        })
    }

    pub async fn get_by_id(pool: &SqlitePool, id: i64, session_id: &str) -> Result<Option<Self>> {
        let row: Option<DiagramFileRow> = sqlx::query_as(
            "SELECT id, session_id, name, filename, content, created_at, updated_at
             FROM diagrams WHERE id = ? AND session_id = ? AND is_deleted = 0",
        )
        .bind(id)
        .bind(session_id)
        .fetch_optional(pool)
        .await
        .context("Failed to get diagram")?;

        Ok(row.map(|r| Self {
            id: r.id,
            session_id: r.session_id,
            name: r.name,
            filename: r.filename,
            content: r.content,
            created_at: r.created_at.parse().unwrap_or_else(|_| Utc::now()),
            updated_at: r.updated_at.parse().unwrap_or_else(|_| Utc::now()),
        }))
    }

    pub async fn list_by_session(
        pool: &SqlitePool,
        session_id: &str,
    ) -> Result<Vec<FileListItem>> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: i64,
            name: String,
            filename: String,
            updated_at: String,
        }

        let rows: Vec<Row> = sqlx::query_as(
            "SELECT id, name, filename, updated_at FROM diagrams
             WHERE session_id = ? AND is_deleted = 0
             ORDER BY updated_at DESC",
        )
        .bind(session_id)
        .fetch_all(pool)
        .await
        .context("Failed to list diagrams")?;

        let expiration_days = 7;
        Ok(rows
            .into_iter()
            .map(|r| {
                let updated_at: DateTime<Utc> = r.updated_at.parse().unwrap_or_else(|_| Utc::now());
                let expires_at = updated_at + chrono::Duration::days(expiration_days);
                FileListItem {
                    id: r.id,
                    name: r.name,
                    filename: r.filename,
                    updated_at,
                    expires_at,
                }
            })
            .collect())
    }

    pub async fn update_content(&self, pool: &SqlitePool, content: &str) -> Result<Self> {
        let now = Utc::now();
        sqlx::query(
            "UPDATE diagrams SET content = ?, updated_at = ? WHERE id = ?",
        )
        .bind(content)
        .bind(now.to_rfc3339())
        .bind(self.id)
        .execute(pool)
        .await
        .context("Failed to update diagram")?;

        Ok(Self {
            content: content.to_string(),
            updated_at: now,
            ..self.clone()
        })
    }

    pub async fn delete(&self, pool: &SqlitePool) -> Result<()> {
        sqlx::query(
            "UPDATE diagrams SET is_deleted = 1 WHERE id = ?",
        )
        .bind(self.id)
        .execute(pool)
        .await
        .context("Failed to delete diagram")?;
        Ok(())
    }

    pub async fn duplicate(&self, pool: &SqlitePool, new_name: Option<&str>) -> Result<Self> {
        let name = match new_name {
            Some(n) => n.to_string(),
            None => format!("{} (copy)", self.name),
        };
        let filename = if name.ends_with(".mmd") {
            name.clone()
        } else {
            format!("{}.mmd", name)
        };

        let now = Utc::now();
        let id = sqlx::query(
            r#"INSERT INTO diagrams (session_id, name, filename, content, created_at, updated_at)
               VALUES (?, ?, ?, ?, ?, ?)"#,
        )
        .bind(&self.session_id)
        .bind(&name)
        .bind(&filename)
        .bind(&self.content)
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(pool)
        .await
        .context("Failed to duplicate diagram")?
        .last_insert_rowid();

        Ok(Self {
            id,
            session_id: self.session_id.clone(),
            name: name.clone(),
            filename,
            content: self.content.clone(),
            created_at: now,
            updated_at: now,
        })
    }

    pub async fn count_by_session(pool: &SqlitePool, session_id: &str) -> Result<i64> {
        let count = sqlx::query_scalar(
            "SELECT COUNT(*) FROM diagrams WHERE session_id = ? AND is_deleted = 0",
        )
        .bind(session_id)
        .fetch_one(pool)
        .await
        .context("Failed to count diagrams")?;
        Ok(count)
    }
}

#[derive(sqlx::FromRow)]
struct DiagramFileRow {
    id: i64,
    session_id: String,
    name: String,
    filename: String,
    content: String,
    created_at: String,
    updated_at: String,
}

pub async fn get_session_info(
    pool: &SqlitePool,
    session_id: &str,
    max_files: usize,
) -> Result<SessionInfo> {
    let session: Option<SessionRow> = sqlx::query_as(
        "SELECT id, created_at, last_activity_at FROM sessions WHERE id = ?",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await
    .context("Failed to get session")?;

    let file_count = DiagramFile::count_by_session(pool, session_id).await?;

    if let Some(s) = session {
        Ok(SessionInfo {
            id: s.id,
            created_at: s.created_at.parse().unwrap_or_else(|_| Utc::now()),
            last_activity_at: s.last_activity_at.parse().unwrap_or_else(|_| Utc::now()),
            file_count,
            max_files,
        })
    } else {
        bail!("Session not found")
    }
}

#[derive(sqlx::FromRow)]
struct SessionRow {
    id: String,
    created_at: String,
    last_activity_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use sqlx::SqlitePool;

    async fn setup_test_db() -> (SqlitePool, String) {
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
        "#).execute(&pool).await.unwrap();

        let session = Session::create(&pool).await.unwrap();
        (pool, session.id)
    }

    #[tokio::test]
    async fn test_file_crud() {
        let (pool, session_id) = setup_test_db().await;

        let file = DiagramFile::create(&pool, &session_id, "test.mmd", Some("flowchart")).await.unwrap();
        assert_eq!(file.name, "test.mmd");

        let retrieved = DiagramFile::get_by_id(&pool, file.id, &session_id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().content, file.content);

        let updated = retrieved.unwrap().update_content(&pool, "graph TD\nA --> B").await.unwrap();
        assert_ne!(updated.content, file.content);

        let list = DiagramFile::list_by_session(&pool, &session_id).await.unwrap();
        assert_eq!(list.len(), 1);

        let duplicated = updated.duplicate(&pool, Some("copy.mmd")).await.unwrap();
        assert_eq!(duplicated.name, "copy.mmd");

        updated.delete(&pool).await.unwrap();

        let after_delete = DiagramFile::get_by_id(&pool, file.id, &session_id).await.unwrap();
        assert!(after_delete.is_none());
    }
}
