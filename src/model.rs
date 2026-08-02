use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Per-file transfer status.
#[derive(Clone, PartialEq)]
pub enum JobStatus {
    Queued,
    Sending,
    Done,
    Error(String),
}

/// A single file queued for upload. `sent`/`status` are behind `Arc` so the
/// background worker thread can update them while the UI reads them each frame.
pub struct FileJob {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub sent: Arc<AtomicU64>,
    pub status: Arc<Mutex<JobStatus>>,
}

impl FileJob {
    /// Build a job from a path, skipping anything that isn't a readable file.
    pub fn new(path: PathBuf) -> Option<Self> {
        let meta = std::fs::metadata(&path).ok()?;
        if !meta.is_file() {
            return None;
        }
        let name = path.file_name()?.to_string_lossy().into_owned();
        Some(Self {
            path,
            name,
            size: meta.len(),
            sent: Arc::new(AtomicU64::new(0)),
            status: Arc::new(Mutex::new(JobStatus::Queued)),
        })
    }

    /// Build a fabricated job (no real file) for `--demo` screenshots/testing.
    pub fn demo(name: &str, size: u64, sent: u64, status: JobStatus) -> Self {
        Self {
            path: PathBuf::from(name),
            name: name.to_string(),
            size,
            sent: Arc::new(AtomicU64::new(sent)),
            status: Arc::new(Mutex::new(status)),
        }
    }

    pub fn status(&self) -> JobStatus {
        self.status.lock().unwrap().clone()
    }

    pub fn set_status(&self, s: JobStatus) {
        *self.status.lock().unwrap() = s;
    }

    pub fn sent(&self) -> u64 {
        self.sent.load(Ordering::Relaxed)
    }

    /// Fraction transferred in [0, 1].
    pub fn progress(&self) -> f32 {
        if self.size == 0 {
            return matches!(self.status(), JobStatus::Done) as u8 as f32;
        }
        (self.sent() as f64 / self.size as f64) as f32
    }
}

/// State of the USB-network link + SSH reachability to the Kindle.
#[derive(Clone, PartialEq)]
pub enum ConnState {
    Disconnected,
    Connecting,
    Connected,
    Failed(String),
}

// ---- remote library (files already on the Kindle) ------------------------

/// What's currently happening to a remote file.
#[derive(Clone, PartialEq)]
pub enum FileOp {
    Idle,
    Downloading,
    Deleting,
    Saved(String),
    Error(String),
}

/// A file that lives on the Kindle.
pub struct RemoteFile {
    pub name: String,
    pub size: u64,
    pub got: Arc<AtomicU64>,
    pub op: Arc<Mutex<FileOp>>,
}

impl RemoteFile {
    pub fn new(name: String, size: u64) -> Self {
        Self {
            name,
            size,
            got: Arc::new(AtomicU64::new(0)),
            op: Arc::new(Mutex::new(FileOp::Idle)),
        }
    }

    pub fn op(&self) -> FileOp {
        self.op.lock().unwrap().clone()
    }

    pub fn set_op(&self, o: FileOp) {
        *self.op.lock().unwrap() = o;
    }

    pub fn got(&self) -> u64 {
        self.got.load(Ordering::Relaxed)
    }

    pub fn progress(&self) -> f32 {
        if self.size == 0 {
            return 0.0;
        }
        (self.got() as f64 / self.size as f64) as f32
    }
}

/// Loading state of the remote listing.
#[derive(Clone, PartialEq)]
pub enum LibraryStatus {
    Idle,
    Loading,
    Loaded,
    Error(String),
}

/// The Kindle's file listing plus its load state.
pub struct Library {
    pub status: LibraryStatus,
    pub files: Vec<Arc<RemoteFile>>,
}

impl Library {
    pub fn new() -> Self {
        Self {
            status: LibraryStatus::Idle,
            files: Vec::new(),
        }
    }
}
