use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::events::AppEvent;

pub struct EventStore {
    base_dir: PathBuf,
}

impl EventStore {
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        EventStore {
            base_dir: base_dir.as_ref().to_path_buf(),
        }
    }

    pub fn events_path(&self) -> PathBuf {
        self.base_dir.join("events.jsonl")
    }

    pub fn ensure_store_dir(&self) -> io::Result<()> {
        std::fs::create_dir_all(&self.base_dir)
    }

    pub fn load_events(&self) -> Vec<AppEvent> {
        let path = self.events_path();
        let file = match File::open(&path) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };
        let reader = BufReader::new(file);
        let mut events = Vec::new();
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("storage: error reading event log: {e}");
                    break;
                }
            };
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<AppEvent>(&line) {
                Ok(event) => events.push(event),
                Err(e) => eprintln!("storage: skipping malformed event line: {e}"),
            }
        }
        events
    }

    pub fn append_event(&self, event: &AppEvent) -> io::Result<()> {
        self.ensure_store_dir()?;
        let path = self.events_path();

        // Guard torn final line: if the file already exists, is non-empty, and
        // its final byte is not '\n' (a partial line from a prior crash), write
        // a '\n' first so the new event cannot be glued onto the fragment.
        let needs_newline_prefix = if path.exists() {
            let mut file = File::open(&path)?;
            let len = file.metadata()?.len();
            if len > 0 {
                file.seek(SeekFrom::End(-1))?;
                let mut buf = [0u8; 1];
                file.read_exact(&mut buf)?;
                buf[0] != b'\n'
            } else {
                false
            }
        } else {
            false
        };

        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;

        if needs_newline_prefix {
            file.write_all(b"\n")?;
        }

        let json = serde_json::to_string(event).map_err(io::Error::other)?;
        file.write_all(json.as_bytes())?;
        file.write_all(b"\n")?;
        Ok(())
    }

    pub fn load_next_seq(&self) -> u64 {
        self.load_events()
            .iter()
            .map(|e| e.seq.0)
            .max()
            .map(|m| m + 1)
            .unwrap_or(1)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::events::{EventKind, EventSeq};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    /// RAII guard that creates a unique temp directory and removes it on drop,
    /// even when a test panics. Uses process ID + atomic counter to stay
    /// parallel-safe.
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            let count = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
            let name = format!("caravan_test_{}_{}", std::process::id(), count);
            let path = std::env::temp_dir().join(name);
            std::fs::create_dir_all(&path).expect("failed to create temp dir");
            TempDir { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn make_event(seq: u64, kind: EventKind, detail: &str) -> AppEvent {
        AppEvent {
            seq: EventSeq(seq),
            kind,
            detail: detail.to_string(),
        }
    }

    #[test]
    fn append_then_load_round_trips_content_and_order() {
        let dir = TempDir::new();
        let store = EventStore::new(dir.path());
        let e1 = make_event(1, EventKind::AppStart, "started");
        let e2 = make_event(2, EventKind::ExitRequest, "exiting");
        store
            .append_event(&e1)
            .expect("first append should succeed");
        store
            .append_event(&e2)
            .expect("second append should succeed");
        let events = store.load_events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], e1);
        assert_eq!(events[1], e2);
    }

    #[test]
    fn several_appends_preserve_seq_order() {
        let dir = TempDir::new();
        let store = EventStore::new(dir.path());
        for i in 1..=5 {
            let e = make_event(i, EventKind::UserMessage, &format!("text {i}"));
            store.append_event(&e).expect("append should succeed");
        }
        let events = store.load_events();
        assert_eq!(events.len(), 5);
        for (i, event) in events.iter().enumerate() {
            assert_eq!(event.seq.0, (i + 1) as u64);
        }
    }

    #[test]
    fn load_next_seq_equals_max_seq_plus_one_after_appends() {
        let dir = TempDir::new();
        let store = EventStore::new(dir.path());
        store
            .append_event(&make_event(1, EventKind::AppStart, ""))
            .expect("append 1");
        store
            .append_event(&make_event(2, EventKind::SlashCommand, ""))
            .expect("append 2");
        store
            .append_event(&make_event(3, EventKind::ExitRequest, ""))
            .expect("append 3");
        assert_eq!(store.load_next_seq(), 4);
    }

    #[test]
    fn load_next_seq_returns_one_on_missing_store() {
        let dir = TempDir::new();
        let store = EventStore::new(dir.path().join("nonexistent_subdir"));
        assert_eq!(store.load_next_seq(), 1);
    }

    #[test]
    fn malformed_line_is_skipped_and_valid_line_is_loaded() {
        let dir = TempDir::new();
        let store = EventStore::new(dir.path());
        store.ensure_store_dir().expect("ensure dir");
        let valid = make_event(1, EventKind::AppStart, "ok");
        let valid_json = serde_json::to_string(&valid).expect("serialize");
        let content = format!("{valid_json}\nnot valid json\n");
        std::fs::write(store.events_path(), content).expect("write file");
        let events = store.load_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], valid);
    }

    #[test]
    fn load_events_on_directory_path_returns_empty() {
        let dir = TempDir::new();
        let store = EventStore::new(dir.path());
        store.ensure_store_dir().expect("ensure dir");
        // Create a directory where events.jsonl would normally be.
        // On Unix, File::open on a directory may succeed but reads yield EISDIR;
        // on other systems open may fail outright. Either way we expect an empty Vec.
        std::fs::create_dir_all(store.events_path()).expect("create dir at events path");
        let events = store.load_events();
        assert_eq!(events, Vec::new());
    }

    #[test]
    fn events_path_equals_base_dir_join_events_jsonl() {
        let dir = TempDir::new();
        let store = EventStore::new(dir.path());
        assert_eq!(store.events_path(), dir.path().join("events.jsonl"));
    }

    #[test]
    fn append_after_partial_line_preserves_new_event() {
        let dir = TempDir::new();
        let store = EventStore::new(dir.path());
        store.ensure_store_dir().expect("ensure dir");

        // Hand-write a file whose final line is a truncated JSON object with
        // NO trailing newline — simulating a torn write from a prior crash.
        let truncated = r#"{"seq":1,"kind":"AppStart","detail":"torn"#;
        std::fs::write(store.events_path(), truncated).expect("write partial file");

        // Append a valid event after the partial line.
        let valid = make_event(2, EventKind::SlashCommand, "after torn");
        store
            .append_event(&valid)
            .expect("append after partial line should succeed");

        // The torn fragment must be skipped; only the valid event is returned.
        let events = store.load_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], valid);
    }
}
