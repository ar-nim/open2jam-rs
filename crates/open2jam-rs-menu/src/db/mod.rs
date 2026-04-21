mod models;

use std::path::Path;

pub use models::{CachedChart, ChartScanEntry, LibraryEntry};

/// Open a SQLite connection pool and initialise schema.
///
/// Creates the database file and parent directories if they don't exist.
///
/// # Errors
/// Returns an error if the database file cannot be created or accessed.
pub async fn open_pool(db_path: &Path) -> anyhow::Result<sqlx::SqlitePool> {
    let db_dir = db_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid DB path: no parent directory"))?;
    std::fs::create_dir_all(db_dir)?;

    // Ensure the DB file exists (sqlx may not auto-create it depending on flags).
    if !db_path.exists() {
        std::fs::File::create(db_path)?;
    }

    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(4)
        .connect(
            db_path
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("invalid DB path: non-UTF8"))?,
        )
        .await?;

    sqlx::query("PRAGMA journal_mode = WAL")
        .execute(&pool)
        .await?;
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await?;

    init_schema(&pool).await?;

    Ok(pool)
}

/// Create tables and indexes if they don't exist.
async fn init_schema(pool: &sqlx::SqlitePool) -> anyhow::Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS libraries (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            root_path TEXT NOT NULL UNIQUE,
            name TEXT NOT NULL,
            added_at INTEGER NOT NULL,
            last_scan INTEGER
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS chart_cache (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            library_id INTEGER NOT NULL,
            relative_path TEXT NOT NULL,
            song_group_id TEXT NOT NULL,
            title TEXT NOT NULL,
            artist TEXT NOT NULL,
            noter TEXT,
            genre INTEGER,
            level_easy INTEGER DEFAULT 0,
            level_normal INTEGER DEFAULT 0,
            level_hard INTEGER DEFAULT 0,
            note_count_easy INTEGER DEFAULT 0,
            note_count_normal INTEGER DEFAULT 0,
            note_count_hard INTEGER DEFAULT 0,
            duration_easy INTEGER DEFAULT 0,
            duration_normal INTEGER DEFAULT 0,
            duration_hard INTEGER DEFAULT 0,
            bpm REAL,
            duration_sec REAL,
            keys INTEGER DEFAULT 7,
            cover_offset INTEGER DEFAULT 0,
            cover_size INTEGER DEFAULT 0,
            file_size INTEGER NOT NULL,
            file_modified INTEGER NOT NULL,
            cached_at INTEGER NOT NULL,
            FOREIGN KEY (library_id) REFERENCES libraries(id) ON DELETE CASCADE
        )",
    )
    .execute(pool)
    .await?;

    // Migrate: add per-difficulty duration columns to existing databases.
    for col in &["duration_easy", "duration_normal", "duration_hard"] {
        let sql = format!(
            "ALTER TABLE chart_cache ADD COLUMN {} INTEGER DEFAULT 0",
            col
        );
        // Ignore errors — column may already exist
        let _ = sqlx::query(&sql).execute(pool).await;
    }

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_chart_cache_library ON chart_cache(library_id)")
        .execute(pool)
        .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_chart_cache_song_group ON chart_cache(song_group_id)",
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_chart_cache_title ON chart_cache(title)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_chart_cache_artist ON chart_cache(artist)")
        .execute(pool)
        .await?;

    Ok(())
}

/// Add a library entry, returning existing if path already registered.
///
/// # Errors
/// Returns an error if the database write fails.
pub async fn add_library(pool: &sqlx::SqlitePool, root_path: &str) -> anyhow::Result<LibraryEntry> {
    let name = Path::new(root_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(root_path)
        .to_string();

    let now = unix_epoch_secs();

    sqlx::query("INSERT OR IGNORE INTO libraries (root_path, name, added_at) VALUES (?, ?, ?)")
        .bind(root_path)
        .bind(&name)
        .bind(now)
        .execute(pool)
        .await?;

    let entry = sqlx::query_as::<_, LibraryEntry>(
        "SELECT id, root_path, name, added_at, last_scan FROM libraries WHERE root_path = ?",
    )
    .bind(root_path)
    .fetch_one(pool)
    .await?;

    Ok(entry)
}

/// Delete a library and all its cached charts (CASCADE).
///
/// # Errors
/// Returns an error if the database write fails.
pub async fn delete_library(pool: &sqlx::SqlitePool, library_id: i64) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM libraries WHERE id = ?")
        .bind(library_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Get all registered libraries.
///
/// # Errors
/// Returns an error if the database read fails.
pub async fn get_all_libraries(pool: &sqlx::SqlitePool) -> anyhow::Result<Vec<LibraryEntry>> {
    let entries = sqlx::query_as::<_, LibraryEntry>(
        "SELECT id, root_path, name, added_at, last_scan FROM libraries ORDER BY id",
    )
    .fetch_all(pool)
    .await?;
    Ok(entries)
}

/// Get all cached charts for a library.
///
/// # Errors
/// Returns an error if the database read fails.
pub async fn get_charts_for_library(
    pool: &sqlx::SqlitePool,
    library_id: i64,
) -> anyhow::Result<Vec<CachedChart>> {
    let charts = sqlx::query_as::<_, CachedChart>(
        r#"SELECT
            id, library_id, relative_path, song_group_id,
            title, artist, noter, genre,
            level_easy, level_normal, level_hard,
            note_count_easy, note_count_normal, note_count_hard,
            duration_easy, duration_normal, duration_hard,
            bpm, duration_sec, keys,
            cover_offset, cover_size,
            file_size, file_modified, cached_at
        FROM chart_cache
        WHERE library_id = ?
        ORDER BY title, artist"#,
    )
    .bind(library_id)
    .fetch_all(pool)
    .await?;
    Ok(charts)
}

/// Delete all cached charts for a library.
///
/// # Errors
/// Returns an error if the database write fails.
pub async fn clear_cache(pool: &sqlx::SqlitePool, library_id: i64) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM chart_cache WHERE library_id = ?")
        .bind(library_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Bulk insert chart entries within a single transaction.
///
/// # Errors
/// Returns an error if any insert or the transaction fails.
pub async fn bulk_insert_charts(
    pool: &sqlx::SqlitePool,
    library_id: i64,
    entries: &[ChartScanEntry],
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;

    for entry in entries {
        let now = unix_epoch_secs();
        let h = &entry.header;

        sqlx::query(
            r#"INSERT INTO chart_cache (
                library_id, relative_path, song_group_id,
                title, artist, noter, genre,
                level_easy, level_normal, level_hard,
                note_count_easy, note_count_normal, note_count_hard,
                duration_easy, duration_normal, duration_hard,
                bpm, duration_sec, keys,
                cover_offset, cover_size,
                file_size, file_modified, cached_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(library_id)
        .bind(&entry.relative_path)
        .bind(&entry.song_group_id)
        .bind(&h.title)
        .bind(&h.artist)
        .bind(&h.noter)
        .bind(h.genre as i32)
        .bind(h.level_easy as i32)
        .bind(h.level_normal as i32)
        .bind(h.level_hard as i32)
        .bind(h.note_count_easy as i32)
        .bind(h.note_count_normal as i32)
        .bind(h.note_count_hard as i32)
        .bind(h.duration_easy as i32)
        .bind(h.duration_normal as i32)
        .bind(h.duration_hard as i32)
        .bind(h.bpm as f64)
        .bind(h.duration_hard as f64)
        .bind(7i32)
        .bind(h.cover_offset as i32)
        .bind(h.cover_size as i32)
        .bind(entry.file_size as i64)
        .bind(entry.file_modified as i64)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

/// Update the last_scan timestamp for a library.
///
/// # Errors
/// Returns an error if the database write fails.
pub async fn update_scan_time(pool: &sqlx::SqlitePool, library_id: i64) -> anyhow::Result<()> {
    let now = unix_epoch_secs();
    sqlx::query("UPDATE libraries SET last_scan = ? WHERE id = ?")
        .bind(now)
        .bind(library_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Get current time as unix epoch seconds (i64).
fn unix_epoch_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use open2jam_rs_parsers::ojn::OjnHeader;

    /// Helper: create an in-memory pool with schema initialised.
    async fn test_pool() -> sqlx::SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("PRAGMA journal_mode = WAL")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .unwrap();
        init_schema(&pool).await.unwrap();
        pool
    }

    fn test_header(title: &str, artist: &str) -> OjnHeader {
        OjnHeader {
            song_id: 1,
            encode_version: 1.0,
            genre: 1,
            bpm: 120.0,
            level_easy: 2,
            level_normal: 5,
            level_hard: 10,
            event_count_easy: 100,
            event_count_normal: 200,
            event_count_hard: 300,
            note_count_easy: 50,
            note_count_normal: 150,
            note_count_hard: 250,
            measure_count_easy: 10,
            measure_count_normal: 20,
            measure_count_hard: 30,
            title: title.to_string(),
            artist: artist.to_string(),
            noter: "Test Noter".to_string(),
            ojm_filename: "test.ojm".to_string(),
            bmp_size: 256,
            cover_size: 4096,
            duration_easy: 60000,
            duration_normal: 120000,
            duration_hard: 180000,
            note_offset_easy: 300,
            note_offset_normal: 600,
            note_offset_hard: 900,
            cover_offset: 1000,
        }
    }

    #[tokio::test]
    async fn open_pool_creates_schema() {
        let pool = test_pool().await;
        // Verify tables exist by querying them
        let libs = get_all_libraries(&pool).await.unwrap();
        assert!(libs.is_empty());
    }

    #[tokio::test]
    async fn open_pool_creates_db_file_from_scratch() {
        // Use a temp path that definitely doesn't exist
        let db_path = std::env::temp_dir()
            .join("open2jam-test-db-create")
            .join("songcache.db");

        // Clean up any leftover state
        let db_dir = db_path.parent().unwrap();
        let _ = std::fs::remove_dir_all(db_dir);
        assert!(!db_path.exists(), "DB should not exist before test");
        assert!(!db_dir.exists(), "DB dir should not exist before test");

        // open_pool must create both the directory and the DB file
        let pool = open_pool(&db_path)
            .await
            .expect("open_pool should create the DB file and directory");

        assert!(db_path.exists(), "DB file should exist after open_pool");

        // Verify schema is initialised
        let libs = get_all_libraries(&pool).await.unwrap();
        assert!(libs.is_empty());

        // Clean up
        drop(pool);
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
        let _ = std::fs::remove_dir_all(db_dir);
    }

    #[tokio::test]
    async fn add_library_inserts_and_returns_existing() {
        let pool = test_pool().await;

        let lib = add_library(&pool, "/music/test").await.unwrap();
        assert_eq!(lib.root_path, "/music/test");
        assert_eq!(lib.name, "test");
        assert!(lib.last_scan.is_none());

        // Calling again should return the same row (INSERT OR IGNORE)
        let lib2 = add_library(&pool, "/music/test").await.unwrap();
        assert_eq!(lib.id, lib2.id);
    }

    #[tokio::test]
    async fn delete_library_removes_entry() {
        let pool = test_pool().await;

        let lib = add_library(&pool, "/music/del").await.unwrap();
        delete_library(&pool, lib.id).await.unwrap();

        let libs = get_all_libraries(&pool).await.unwrap();
        assert!(libs.is_empty());
    }

    #[tokio::test]
    async fn bulk_insert_and_read_charts() {
        let pool = test_pool().await;

        let lib = add_library(&pool, "/music/bulk").await.unwrap();

        let header1 = test_header("Song A", "Artist A");
        let header2 = test_header("Song B", "Artist B");

        let entries = vec![
            ChartScanEntry {
                relative_path: "song_a/file.ojn".to_string(),
                song_group_id: "group_a".to_string(),
                header: header1,
                file_size: 50000,
                file_modified: 1_700_000_000,
            },
            ChartScanEntry {
                relative_path: "song_b/file.ojn".to_string(),
                song_group_id: "group_b".to_string(),
                header: header2,
                file_size: 60000,
                file_modified: 1_700_100_000,
            },
        ];

        bulk_insert_charts(&pool, lib.id, &entries).await.unwrap();

        let charts = get_charts_for_library(&pool, lib.id).await.unwrap();
        assert_eq!(charts.len(), 2);
        assert_eq!(charts[0].title, "Song A");
        assert_eq!(charts[1].title, "Song B");
    }

    #[tokio::test]
    async fn clear_cache_removes_all_charts_for_library() {
        let pool = test_pool().await;

        let lib = add_library(&pool, "/music/clear").await.unwrap();
        let header = test_header("Song X", "Artist X");
        let entries = vec![ChartScanEntry {
            relative_path: "x/file.ojn".to_string(),
            song_group_id: "g_x".to_string(),
            header,
            file_size: 10000,
            file_modified: 1_700_000_000,
        }];

        bulk_insert_charts(&pool, lib.id, &entries).await.unwrap();
        clear_cache(&pool, lib.id).await.unwrap();

        let charts = get_charts_for_library(&pool, lib.id).await.unwrap();
        assert!(charts.is_empty());
    }

    #[tokio::test]
    async fn update_scan_time_sets_timestamp() {
        let pool = test_pool().await;

        let lib = add_library(&pool, "/music/scan").await.unwrap();
        assert!(lib.last_scan.is_none());

        update_scan_time(&pool, lib.id).await.unwrap();

        let libs = get_all_libraries(&pool).await.unwrap();
        let updated = libs.iter().find(|l| l.id == lib.id).unwrap();
        assert!(updated.last_scan.is_some());
    }

    #[tokio::test]
    async fn delete_library_cascades_to_charts() {
        let pool = test_pool().await;

        let lib = add_library(&pool, "/music/cascade").await.unwrap();
        let header = test_header("Cascade", "Artist");
        let entries = vec![ChartScanEntry {
            relative_path: "cascade/file.ojn".to_string(),
            song_group_id: "g_c".to_string(),
            header,
            file_size: 20000,
            file_modified: 1_700_000_000,
        }];

        bulk_insert_charts(&pool, lib.id, &entries).await.unwrap();

        // Verify charts exist
        let charts_before = get_charts_for_library(&pool, lib.id).await.unwrap();
        assert_eq!(charts_before.len(), 1);

        // Delete library
        delete_library(&pool, lib.id).await.unwrap();

        // Charts should be gone (CASCADE)
        let charts_after = get_charts_for_library(&pool, lib.id).await.unwrap();
        assert!(charts_after.is_empty());
    }

    #[test]
    fn unix_epoch_secs_returns_positive_value() {
        let now = unix_epoch_secs();
        assert!(now > 1_700_000_000);
    }
}
