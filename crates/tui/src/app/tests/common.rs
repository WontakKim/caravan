use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

pub static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct TempDir {
    path: std::path::PathBuf,
}

impl TempDir {
    pub fn new() -> Self {
        let count = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let name = format!("caravan_apptest_{}_{}", std::process::id(), count);
        let path = std::env::temp_dir().join(name);
        std::fs::create_dir_all(&path).expect("failed to create temp dir");
        TempDir { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
