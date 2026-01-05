use sqlx::SqlitePool;
use anyhow::{Result, Context};
use std::io::{Cursor, Write};
use zip::write::{FileOptions, ZipWriter};

#[derive(sqlx::FromRow)]
struct FileRow {
    name: String,
    filename: String,
    content: String,
}

pub async fn export_all_files(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<Vec<u8>> {
    let files: Vec<FileRow> = sqlx::query_as(
        "SELECT name, filename, content FROM diagrams
         WHERE session_id = ? AND is_deleted = 0
         ORDER BY updated_at DESC",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await
    .context("Failed to fetch files for export")?;

    if files.is_empty() {
        return Ok(Vec::new());
    }

    let mut cursor = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(&mut cursor);
    let options: FileOptions<()> = FileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .unix_permissions(0o644);

    for file in &files {
        let filename = if file.filename.is_empty() {
            format!("{}.mmd", file.name)
        } else {
            file.filename.clone()
        };

        zip.start_file(filename, options)
            .with_context(|| "Failed to start zip file entry".to_string())?;
        zip.write_all(file.content.as_bytes())
            .with_context(|| "Failed to write file content".to_string())?;
    }

    zip.finish()
        .with_context(|| "Failed to finalize ZIP file".to_string())?;

    Ok(cursor.into_inner())
}

pub fn export_single_file(_filename: &str, content: &str) -> Result<Vec<u8>> {
    Ok(content.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use sqlx::SqlitePool;
    use crate::session::Session;
    use crate::files::DiagramFile;

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
    async fn test_single_export() {
        let content = "graph TD\nA --> B";
        let result = export_single_file("test.mmd", content).unwrap();
        assert_eq!(result, content.as_bytes());
    }

    #[tokio::test]
    async fn test_bulk_export() {
        let (pool, session_id) = setup_test_db().await;

        DiagramFile::create(&pool, &session_id, "file1.mmd", Some("flowchart")).await.unwrap();
        DiagramFile::create(&pool, &session_id, "file2.mmd", Some("sequence")).await.unwrap();

        let zip_data = export_all_files(&pool, &session_id).await.unwrap();
        assert!(!zip_data.is_empty());

        let mut zip = zip::ZipArchive::new(Cursor::new(zip_data)).unwrap();
        assert_eq!(zip.len(), 2);
    }
}
