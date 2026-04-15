/// Row from the `libraries` table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LibraryEntry {
    pub id: i64,
    pub root_path: String,
    pub name: String,
    pub added_at: i64,
    pub last_scan: Option<i64>,
}

/// Row from the `chart_cache` table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CachedChart {
    pub id: i64,
    pub library_id: i64,
    pub relative_path: String,
    pub song_group_id: String,
    pub title: String,
    pub artist: String,
    pub noter: String,
    pub genre: i32,
    pub level_easy: i32,
    pub level_normal: i32,
    pub level_hard: i32,
    pub note_count_easy: i32,
    pub note_count_normal: i32,
    pub note_count_hard: i32,
    pub duration_easy: i32,
    pub duration_normal: i32,
    pub duration_hard: i32,
    pub bpm: f64,
    pub duration_sec: f64,
    pub keys: i32,
    pub cover_offset: i32,
    pub cover_size: i32,
    pub file_size: i64,
    pub file_modified: i64,
    pub cached_at: i64,
}

/// Intermediate representation during scanning (before DB insert).
/// Uses the OjnHeader from the shared parser crate — no duplication.
pub struct ChartScanEntry {
    pub relative_path: String,
    pub song_group_id: String,
    pub header: open2jam_rs_parsers::ojn::OjnHeader,
    pub file_size: u64,
    pub file_modified: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_entry_fields_are_accessible() {
        let entry = LibraryEntry {
            id: 1,
            root_path: "/music".to_string(),
            name: "My Library".to_string(),
            added_at: 1_700_000_000,
            last_scan: None,
        };
        assert_eq!(entry.id, 1);
        assert_eq!(entry.root_path, "/music");
        assert!(entry.last_scan.is_none());
    }

    #[test]
    fn library_entry_with_scan_time() {
        let entry = LibraryEntry {
            id: 1,
            root_path: "/music".to_string(),
            name: "My Library".to_string(),
            added_at: 1_700_000_000,
            last_scan: Some(1_700_100_000),
        };
        assert_eq!(entry.last_scan, Some(1_700_100_000));
    }

    #[test]
    fn cached_chart_fields_are_accessible() {
        let chart = CachedChart {
            id: 1,
            library_id: 1,
            relative_path: "song/file.ojn".to_string(),
            song_group_id: "abc123".to_string(),
            title: "Test Song".to_string(),
            artist: "Test Artist".to_string(),
            noter: "Test Noter".to_string(),
            genre: 1,
            level_easy: 1,
            level_normal: 5,
            level_hard: 10,
            note_count_easy: 100,
            note_count_normal: 300,
            note_count_hard: 600,
            duration_easy: 60_000,
            duration_normal: 120_000,
            duration_hard: 180_000,
            bpm: 140.0,
            duration_sec: 180.0,
            keys: 7,
            cover_offset: 1000,
            cover_size: 5000,
            file_size: 50000,
            file_modified: 1_700_000_000,
            cached_at: 1_700_000_000,
        };
        assert_eq!(chart.title, "Test Song");
        assert_eq!(chart.level_hard, 10);
        assert_eq!(chart.duration_hard, 180_000);
        assert!((chart.bpm - 140.0).abs() < f64::EPSILON);
    }

    #[test]
    fn chart_scan_entry_from_ojn_header() {
        use open2jam_rs_parsers::ojn::OjnHeader;

        let header = OjnHeader {
            song_id: 42,
            encode_version: 1.0,
            genre: 1,
            bpm: 120.0,
            level_easy: 2,
            level_normal: 6,
            level_hard: 12,
            event_count_easy: 100,
            event_count_normal: 200,
            event_count_hard: 300,
            note_count_easy: 50,
            note_count_normal: 150,
            note_count_hard: 250,
            measure_count_easy: 10,
            measure_count_normal: 20,
            measure_count_hard: 30,
            title: "My Song".to_string(),
            artist: "My Artist".to_string(),
            noter: "My Noter".to_string(),
            ojm_filename: "song.ojm".to_string(),
            bmp_size: 256,
            cover_size: 4096,
            duration_easy: 60000,
            duration_normal: 120000,
            duration_hard: 180000,
            note_offset_easy: 300,
            note_offset_normal: 600,
            note_offset_hard: 900,
            cover_offset: 1000,
        };

        let entry = ChartScanEntry {
            relative_path: "test/song.ojn".to_string(),
            song_group_id: "deadbeef".to_string(),
            header,
            file_size: 50000,
            file_modified: 1_700_000_000,
        };

        assert_eq!(entry.relative_path, "test/song.ojn");
        assert_eq!(entry.header.title, "My Song");
        assert_eq!(entry.file_size, 50000);
    }
}
