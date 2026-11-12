//! The asset library: imported folders, background scanning and analysis,
//! tags and search.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant, UNIX_EPOCH};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use walkdir::WalkDir;

use crate::analyze::analyze;
use crate::db::{Db, FoundFile};
use crate::model::{
    Asset, CHANNEL_TAG_CATEGORY, CHANNEL_TAGS, Folder, SUPPORTED_EXTENSIONS, Tag,
    guess_channel_tag,
};
use crate::search::{Index, Query, SearchResult};
use crate::{Error, Result};

/// Wait this long after the last file-system event before rescanning, so a
/// folder being copied in is scanned once, not once per file.
const WATCH_DEBOUNCE: Duration = Duration::from_millis(1500);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);
const CHANGED_INTERVAL: Duration = Duration::from_millis(1000);

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct ScanStatus {
    /// Folder currently being walked, if any.
    pub scanning: Option<String>,
    /// Files seen so far in the folder being walked.
    pub files_found: usize,
    /// Files analysed since the queue last ran empty.
    pub analyzed: usize,
    /// Files still waiting for analysis (including ones in progress).
    pub remaining: usize,
    /// Files in this run that could not be read.
    pub failed: usize,
}

impl ScanStatus {
    pub fn busy(&self) -> bool {
        self.scanning.is_some() || self.remaining > 0
    }
}

#[derive(Debug, Clone)]
pub enum LibraryEvent {
    /// Scan/analysis progress changed.
    Progress(ScanStatus),
    /// Folders or assets changed: anything showing library data should refresh.
    Changed,
}

type EventSink = Box<dyn Fn(LibraryEvent) + Send + Sync>;

#[derive(Default)]
struct Queue {
    items: VecDeque<(i64, String)>,
    /// Ids that are queued or being analysed right now.
    active: HashSet<i64>,
}

struct Inner {
    db: Mutex<Db>,
    index: RwLock<Index>,
    queue: Mutex<Queue>,
    queue_cv: Condvar,
    analyzed: AtomicUsize,
    failed: AtomicUsize,
    scans_pending: AtomicUsize,
    scanning: Mutex<Option<String>>,
    files_found: AtomicUsize,
    shutdown: AtomicBool,
    on_event: EventSink,
    last_progress: Mutex<Instant>,
    last_changed: Mutex<Instant>,
    scan_tx: Mutex<Sender<i64>>,
    watcher: Mutex<Option<RecommendedWatcher>>,
    watch_tx: Mutex<Sender<PathBuf>>,
}

pub struct Library {
    inner: Arc<Inner>,
}

fn normalize(path: &str) -> String {
    let mut p = path.replace('/', "\\").to_lowercase();
    while p.len() > 3 && p.ends_with('\\') {
        p.pop();
    }
    p
}

/// True if `a` and `b` are the same folder or one contains the other.
fn overlaps(a: &str, b: &str) -> bool {
    let (a, b) = (normalize(a), normalize(b));
    let inside = |outer: &str, inner: &str| {
        inner.len() > outer.len()
            && inner.starts_with(outer)
            && (outer.ends_with('\\') || inner.as_bytes()[outer.len()] == b'\\')
    };
    a == b || inside(&a, &b) || inside(&b, &a)
}

fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| SUPPORTED_EXTENSIONS.iter().any(|s| s.eq_ignore_ascii_case(e)))
}

impl Library {
    /// Opens (or creates) the library database. Call [`Library::start`]
    /// afterwards to begin background work.
    pub fn open(db_path: &Path, on_event: impl Fn(LibraryEvent) + Send + Sync + 'static) -> Result<Self> {
        Self::with_db(Db::open(db_path)?, on_event)
    }

    pub fn in_memory(on_event: impl Fn(LibraryEvent) + Send + Sync + 'static) -> Result<Self> {
        Self::with_db(Db::open_in_memory()?, on_event)
    }

    fn with_db(db: Db, on_event: impl Fn(LibraryEvent) + Send + Sync + 'static) -> Result<Self> {
        let index = Index::new(db.all_assets()?);
        let (scan_tx, scan_rx) = mpsc::channel();
        let (watch_tx, watch_rx) = mpsc::channel();
        let long_ago = Instant::now() - Duration::from_secs(60);
        let inner = Arc::new(Inner {
            db: Mutex::new(db),
            index: RwLock::new(index),
            queue: Mutex::default(),
            queue_cv: Condvar::new(),
            analyzed: AtomicUsize::new(0),
            failed: AtomicUsize::new(0),
            scans_pending: AtomicUsize::new(0),
            scanning: Mutex::new(None),
            files_found: AtomicUsize::new(0),
            shutdown: AtomicBool::new(false),
            on_event: Box::new(on_event),
            last_progress: Mutex::new(long_ago),
            last_changed: Mutex::new(long_ago),
            scan_tx: Mutex::new(scan_tx),
            watcher: Mutex::new(None),
            watch_tx: Mutex::new(watch_tx),
        });

        let workers = thread::available_parallelism()
            .map(|n| n.get().saturating_sub(1))
            .unwrap_or(1)
            .clamp(1, 6);
        for i in 0..workers {
            let inner = inner.clone();
            thread::Builder::new()
                .name(format!("gmaudio-analyze-{i}"))
                .spawn(move || inner.analysis_worker())?;
        }
        {
            let inner = inner.clone();
            thread::Builder::new()
                .name("gmaudio-scan".into())
                .spawn(move || inner.scan_worker(scan_rx))?;
        }
        {
            let inner = inner.clone();
            thread::Builder::new()
                .name("gmaudio-watch".into())
                .spawn(move || inner.watch_worker(watch_rx))?;
        }
        Ok(Self { inner })
    }

    /// Resumes unfinished analysis, rescans every folder for changes made
    /// while the app was closed, and starts watching the folders.
    pub fn start(&self) -> Result<()> {
        // Libraries imported before channel tags existed get them once.
        const DONE: &str = "channel_tags_guessed";
        if self.get_setting(DONE)?.is_none() {
            self.inner.guess_channel_tags(None)?;
            self.inner.reload_index()?;
            self.set_setting(DONE, "1")?;
        }
        // Likewise tempo: measure music that was analysed before it existed.
        const TEMPO: &str = "tempo_measured";
        if self.get_setting(TEMPO)?.is_none() {
            self.inner.db.lock().unwrap().requeue_for_tempo()?;
            self.inner.reload_index()?;
            self.set_setting(TEMPO, "1")?;
        }
        self.inner.enqueue_pending()?;
        let folders = self.folders()?;
        self.inner.start_watcher(&folders);
        for folder in folders {
            self.inner.request_scan(folder.id);
        }
        Ok(())
    }

    pub fn folders(&self) -> Result<Vec<Folder>> {
        self.inner.db.lock().unwrap().folders()
    }

    /// Adds a folder and scans it in the background.
    pub fn add_folder(&self, path: &Path) -> Result<Folder> {
        if !path.is_dir() {
            return Err(Error::NotAFolder(path.to_path_buf()));
        }
        let path_str = path.to_string_lossy().to_string();
        let id = {
            let db = self.inner.db.lock().unwrap();
            if db.folders()?.iter().any(|f| overlaps(&f.path, &path_str)) {
                return Err(Error::FolderAlreadyAdded);
            }
            db.add_folder(&path_str)?
        };
        self.inner.watch(path);
        self.inner.request_scan(id);
        self.inner.emit_changed(true);
        Ok(Folder {
            id,
            path: path_str,
            asset_count: 0,
            missing: false,
        })
    }

    /// Removes a folder and everything imported from it. Files on disk are
    /// not touched.
    pub fn remove_folder(&self, id: i64) -> Result<()> {
        let path = {
            let db = self.inner.db.lock().unwrap();
            let path = db.folder_path(id)?;
            db.remove_folder(id)?;
            path
        };
        if let Some(w) = self.inner.watcher.lock().unwrap().as_mut() {
            let _ = w.unwatch(Path::new(&path));
        }
        self.inner.reload_index()?;
        self.inner.emit_changed(true);
        Ok(())
    }

    /// Rescans one folder, or all of them.
    pub fn rescan(&self, folder_id: Option<i64>) -> Result<()> {
        match folder_id {
            Some(id) => self.inner.request_scan(id),
            None => {
                for f in self.folders()? {
                    self.inner.request_scan(f.id);
                }
            }
        }
        Ok(())
    }

    pub fn search(&self, query: &Query) -> SearchResult {
        self.inner.index.read().unwrap().search(query)
    }

    pub fn asset(&self, id: i64) -> Option<Asset> {
        self.inner.index.read().unwrap().get(id).cloned()
    }

    pub fn asset_count(&self) -> usize {
        self.inner.index.read().unwrap().len()
    }

    pub fn set_tags(&self, asset_id: i64, tags: &[Tag]) -> Result<Asset> {
        let asset = {
            let mut db = self.inner.db.lock().unwrap();
            db.set_tags(asset_id, tags)?;
            db.asset(asset_id)?
        };
        self.inner.index.write().unwrap().upsert(asset.clone());
        Ok(asset)
    }

    /// Adds or removes one tag on many assets at once.
    pub fn set_tag_on(&self, asset_ids: &[i64], tag: &Tag, on: bool) -> Result<()> {
        self.inner.db.lock().unwrap().set_tag_on(asset_ids, tag, on)?;
        self.inner.reload_index()?;
        self.inner.emit_changed(true);
        Ok(())
    }

    /// Documents of one kind as (id, json).
    pub fn docs(&self, kind: &str) -> Result<Vec<(i64, String)>> {
        self.inner.db.lock().unwrap().docs(kind)
    }

    pub fn doc(&self, id: i64) -> Result<Option<(String, String)>> {
        self.inner.db.lock().unwrap().doc(id)
    }

    pub fn put_doc(&self, kind: &str, id: Option<i64>, data: &str) -> Result<i64> {
        self.inner.db.lock().unwrap().put_doc(kind, id, data)
    }

    pub fn delete_doc(&self, id: i64) -> Result<()> {
        self.inner.db.lock().unwrap().delete_doc(id)
    }

    /// All tags with their asset counts.
    pub fn tags(&self) -> Result<Vec<(Tag, i64)>> {
        self.inner.db.lock().unwrap().tags()
    }

    pub fn status(&self) -> ScanStatus {
        self.inner.status()
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        self.inner.db.lock().unwrap().get_setting(key)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.inner.db.lock().unwrap().set_setting(key, value)
    }

    /// Blocks until scanning and analysis have finished, or the timeout
    /// passes. Returns whether the library went idle.
    pub fn wait_idle(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if !self.inner.status().busy() && self.inner.scans_pending.load(Ordering::SeqCst) == 0 {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        self.inner.shutdown.store(true, Ordering::SeqCst);
        self.inner.queue_cv.notify_all();
        *self.inner.watcher.lock().unwrap() = None;
    }
}

impl Inner {
    fn stopping(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    fn status(&self) -> ScanStatus {
        ScanStatus {
            scanning: self.scanning.lock().unwrap().clone(),
            files_found: self.files_found.load(Ordering::Relaxed),
            analyzed: self.analyzed.load(Ordering::Relaxed),
            remaining: self.queue.lock().unwrap().active.len(),
            failed: self.failed.load(Ordering::Relaxed),
        }
    }

    fn emit_progress(&self, force: bool) {
        {
            let mut last = self.last_progress.lock().unwrap();
            if !force && last.elapsed() < PROGRESS_INTERVAL {
                return;
            }
            *last = Instant::now();
        }
        (self.on_event)(LibraryEvent::Progress(self.status()));
    }

    fn emit_changed(&self, force: bool) {
        {
            let mut last = self.last_changed.lock().unwrap();
            if !force && last.elapsed() < CHANGED_INTERVAL {
                return;
            }
            *last = Instant::now();
        }
        (self.on_event)(LibraryEvent::Changed);
    }

    fn reload_index(&self) -> Result<()> {
        let assets = self.db.lock().unwrap().all_assets()?;
        self.index.write().unwrap().rebuild(assets);
        Ok(())
    }

    /// Gives assets that have no channel tag one guessed from their folder
    /// names. `since` limits this to a folder's newly added files.
    fn guess_channel_tags(&self, since: Option<(i64, i64)>) -> Result<()> {
        let mut db = self.db.lock().unwrap();
        let candidates = db.assets_without_tags(&CHANNEL_TAGS, since)?;
        let mut by_tag: [Vec<i64>; 4] = Default::default();
        for (id, path) in candidates {
            if let Some(tag) = guess_channel_tag(&path) {
                let slot = CHANNEL_TAGS.iter().position(|t| *t == tag).unwrap_or(0);
                by_tag[slot].push(id);
            }
        }
        for (name, ids) in CHANNEL_TAGS.iter().zip(&by_tag) {
            let tag = Tag {
                name: (*name).into(),
                category: CHANNEL_TAG_CATEGORY.into(),
            };
            db.set_tag_on(ids, &tag, true)?;
        }
        Ok(())
    }

    // ---- scanning ------------------------------------------------------

    fn request_scan(&self, folder_id: i64) {
        self.scans_pending.fetch_add(1, Ordering::SeqCst);
        if self.scan_tx.lock().unwrap().send(folder_id).is_err() {
            self.scans_pending.fetch_sub(1, Ordering::SeqCst);
        }
    }

    fn scan_worker(&self, rx: Receiver<i64>) {
        loop {
            let folder_id = match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(id) => id,
                Err(RecvTimeoutError::Timeout) if !self.stopping() => continue,
                Err(_) => return,
            };
            if !self.stopping() {
                // A folder removed while its scan was queued is simply skipped.
                let _ = self.scan_folder(folder_id);
            }
            self.scans_pending.fetch_sub(1, Ordering::SeqCst);
            self.emit_progress(true);
        }
    }

    fn scan_folder(&self, folder_id: i64) -> Result<()> {
        let root = self.db.lock().unwrap().folder_path(folder_id)?;
        // An unplugged drive must not mark the whole folder as missing.
        if !Path::new(&root).is_dir() {
            return Ok(());
        }

        *self.scanning.lock().unwrap() = Some(root.clone());
        self.files_found.store(0, Ordering::Relaxed);
        self.emit_progress(true);

        let mut found = Vec::new();
        for entry in WalkDir::new(&root).follow_links(false).into_iter().flatten() {
            if self.stopping() {
                break;
            }
            if !entry.file_type().is_file() || !is_supported(entry.path()) {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let path = entry.path();
            found.push(FoundFile {
                path: path.to_string_lossy().to_string(),
                name: path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default(),
                ext: path
                    .extension()
                    .map(|s| s.to_string_lossy().to_lowercase())
                    .unwrap_or_default(),
                size: meta.len() as i64,
                mtime: meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
            });
            self.files_found.store(found.len(), Ordering::Relaxed);
            self.emit_progress(false);
        }

        let result = (|| {
            if self.stopping() {
                return Ok(());
            }
            let before = UNIX_EPOCH.elapsed().map(|d| d.as_secs() as i64).unwrap_or(0);
            let diff = self.db.lock().unwrap().apply_scan(folder_id, &found)?;
            if diff.added > 0 {
                self.guess_channel_tags(Some((folder_id, before)))?;
            }
            if !diff.is_empty() {
                self.reload_index()?;
                self.enqueue_pending()?;
                self.emit_changed(true);
            }
            Ok(())
        })();
        *self.scanning.lock().unwrap() = None;
        self.files_found.store(0, Ordering::Relaxed);
        result
    }

    // ---- analysis ------------------------------------------------------

    fn enqueue_pending(&self) -> Result<()> {
        let pending = self.db.lock().unwrap().pending()?;
        let mut queue = self.queue.lock().unwrap();
        for (id, path) in pending {
            if queue.active.insert(id) {
                queue.items.push_back((id, path));
            }
        }
        drop(queue);
        self.queue_cv.notify_all();
        self.emit_progress(true);
        Ok(())
    }

    fn analysis_worker(&self) {
        loop {
            let (id, path) = {
                let mut queue = self.queue.lock().unwrap();
                loop {
                    if self.stopping() {
                        return;
                    }
                    if let Some(job) = queue.items.pop_front() {
                        break job;
                    }
                    queue = self
                        .queue_cv
                        .wait_timeout(queue, Duration::from_millis(500))
                        .unwrap()
                        .0;
                }
            };

            let outcome = analyze(Path::new(&path));
            if self.stopping() {
                return;
            }
            let stored = self.store_analysis(id, outcome);
            if stored.is_ok() {
                self.analyzed.fetch_add(1, Ordering::Relaxed);
            }

            let drained = {
                let mut queue = self.queue.lock().unwrap();
                queue.active.remove(&id);
                let drained = queue.active.is_empty();
                if drained {
                    // Reset while still holding the lock, so nobody can see
                    // an idle queue with stale counters.
                    self.analyzed.store(0, Ordering::Relaxed);
                    self.failed.store(0, Ordering::Relaxed);
                }
                drained
            };
            self.emit_progress(drained);
            self.emit_changed(drained);
        }
    }

    fn store_analysis(&self, id: i64, outcome: Result<crate::model::Analysis>) -> Result<()> {
        let mut db = self.db.lock().unwrap();
        let final_id = match outcome {
            Ok(analysis) => db.finish_analysis(id, &analysis)?,
            Err(e) => {
                self.failed.fetch_add(1, Ordering::Relaxed);
                db.fail_analysis(id, &e.to_string())?;
                id
            }
        };
        if final_id == id {
            // The row can be gone if its folder was removed meanwhile.
            if let Ok(asset) = db.asset(id) {
                drop(db);
                self.index.write().unwrap().upsert(asset);
            }
        } else {
            // A moved file was re-linked to its old row: ids changed.
            let assets = db.all_assets()?;
            drop(db);
            self.index.write().unwrap().rebuild(assets);
        }
        Ok(())
    }

    // ---- watching ------------------------------------------------------

    fn start_watcher(&self, folders: &[Folder]) {
        let tx = self.watch_tx.lock().unwrap().clone();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                for path in event.paths {
                    let _ = tx.send(path);
                }
            }
        });
        // No watcher (rare) only costs automatic pickup: rescan still works.
        if let Ok(w) = watcher {
            *self.watcher.lock().unwrap() = Some(w);
            for f in folders {
                self.watch(Path::new(&f.path));
            }
        }
    }

    fn watch(&self, path: &Path) {
        if let Some(w) = self.watcher.lock().unwrap().as_mut() {
            let _ = w.watch(path, RecursiveMode::Recursive);
        }
    }

    /// Collects file-system events and rescans the affected folders once
    /// things have been quiet for [`WATCH_DEBOUNCE`].
    fn watch_worker(&self, rx: Receiver<PathBuf>) {
        let mut dirty: HashSet<i64> = HashSet::new();
        let mut last_event = Instant::now();
        // Copying a sound pack in fires thousands of events: look the folder
        // list up at most once a second instead of once per event.
        let mut folders: Vec<Folder> = Vec::new();
        let mut folders_at: Option<Instant> = None;
        loop {
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(path) => {
                    // Audio files, or anything without an extension (folders).
                    if !(is_supported(&path) || path.extension().is_none()) {
                        continue;
                    }
                    if folders_at.is_none_or(|t| t.elapsed() >= Duration::from_secs(1)) {
                        if let Ok(f) = self.db.lock().unwrap().folders() {
                            folders = f;
                        }
                        folders_at = Some(Instant::now());
                    }
                    let path = path.to_string_lossy().to_string();
                    for f in &folders {
                        if overlaps(&f.path, &path) {
                            dirty.insert(f.id);
                            last_event = Instant::now();
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            if self.stopping() {
                return;
            }
            if !dirty.is_empty() && last_event.elapsed() >= WATCH_DEBOUNCE {
                for id in dirty.drain() {
                    self.request_scan(id);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyze::testutil::write_sine_wav;
    use crate::model::AnalysisStatus;
    use std::sync::atomic::AtomicUsize;

    const WAIT: Duration = Duration::from_secs(60);

    fn q(text: &str) -> Query {
        Query {
            text: text.into(),
            ..Default::default()
        }
    }

    #[test]
    fn overlap_rules() {
        assert!(overlaps("D:\\Audio", "d:/audio/"));
        assert!(overlaps("D:\\Audio", "D:\\Audio\\Music"));
        assert!(overlaps("D:\\Audio\\Music", "D:\\Audio"));
        assert!(!overlaps("D:\\Audio", "D:\\Audio2"));
        assert!(!overlaps("D:\\Audio\\Music", "D:\\Audio\\SFX"));
        assert!(overlaps("D:\\", "D:\\Audio"));
    }

    #[test]
    fn imports_analyzes_and_searches_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        write_sine_wav(&dir.path().join("Tavern Night.wav"), 1.0, 440.0, -20.0, 2);
        write_sine_wav(&dir.path().join("sfx/Door Creak.WAV"), 0.5, 880.0, -12.0, 1);
        std::fs::write(dir.path().join("notes.txt"), "not audio").unwrap();
        std::fs::write(dir.path().join("broken.mp3"), "definitely not an mp3").unwrap();

        let changed = Arc::new(AtomicUsize::new(0));
        let c = changed.clone();
        let lib = Library::in_memory(move |e| {
            if matches!(e, LibraryEvent::Changed) {
                c.fetch_add(1, Ordering::SeqCst);
            }
        })
        .unwrap();
        let folder = lib.add_folder(dir.path()).unwrap();
        assert!(lib.wait_idle(WAIT));

        let all = lib.search(&q(""));
        assert_eq!(all.total, 3, "txt is ignored, broken mp3 is listed as failed");
        let tavern = &lib.search(&q("tavern")).assets[0];
        assert_eq!(tavern.status, AnalysisStatus::Ok);
        assert_eq!(tavern.duration_ms, Some(1000));
        assert!(tavern.loudness_lufs.is_some());
        let door = &lib.search(&q("door")).assets[0];
        assert_eq!((door.ext.as_str(), door.channels), ("wav", Some(1)));
        let broken = &lib.search(&q("broken")).assets[0];
        assert_eq!(broken.status, AnalysisStatus::Failed);
        assert!(broken.error.is_some());

        assert_eq!(lib.folders().unwrap()[0].asset_count, 3);
        assert!(changed.load(Ordering::SeqCst) >= 2);
        assert_eq!(lib.status(), ScanStatus::default());

        // Adding the same folder, a parent or a child again is refused.
        assert!(matches!(lib.add_folder(dir.path()), Err(Error::FolderAlreadyAdded)));
        assert!(matches!(
            lib.add_folder(&dir.path().join("sfx")),
            Err(Error::FolderAlreadyAdded)
        ));

        lib.remove_folder(folder.id).unwrap();
        assert_eq!(lib.search(&q("")).total, 0);
        assert!(lib.folders().unwrap().is_empty());
    }

    #[test]
    fn rescan_picks_up_new_removed_and_moved_files() {
        let dir = tempfile::tempdir().unwrap();
        write_sine_wav(&dir.path().join("one.wav"), 0.5, 440.0, -20.0, 2);
        write_sine_wav(&dir.path().join("two.wav"), 0.5, 550.0, -20.0, 2);
        let lib = Library::in_memory(|_| {}).unwrap();
        let folder = lib.add_folder(dir.path()).unwrap();
        assert!(lib.wait_idle(WAIT));

        let one = lib.search(&q("one")).assets[0].clone();
        lib.set_tags(
            one.id,
            &[Tag {
                name: "keep me".into(),
                category: "custom".into(),
            }],
        )
        .unwrap();

        // Delete two.wav, add three.wav, move one.wav into a subfolder.
        std::fs::remove_file(dir.path().join("two.wav")).unwrap();
        write_sine_wav(&dir.path().join("three.wav"), 0.5, 660.0, -20.0, 2);
        std::fs::create_dir(dir.path().join("moved")).unwrap();
        std::fs::rename(dir.path().join("one.wav"), dir.path().join("moved/uno.wav")).unwrap();

        lib.rescan(Some(folder.id)).unwrap();
        assert!(lib.wait_idle(WAIT));

        assert_eq!(lib.search(&q("two")).total, 0);
        assert_eq!(lib.search(&q("three")).total, 1);
        let moved = lib.search(&q("uno")).assets[0].clone();
        assert_eq!(moved.id, one.id, "a moved file keeps its identity");
        assert_eq!(moved.tags[0].name, "keep me");
        assert_eq!(lib.search(&q("")).total, 2);
    }

    #[test]
    fn library_survives_reopen_and_resumes() {
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path().join("audio");
        write_sine_wav(&audio.join("a.wav"), 0.5, 440.0, -20.0, 2);
        let db_path = dir.path().join("lib.db");
        {
            let lib = Library::open(&db_path, |_| {}).unwrap();
            lib.add_folder(&audio).unwrap();
            assert!(lib.wait_idle(WAIT));
        }
        // A file appears while the app is closed.
        write_sine_wav(&audio.join("b.wav"), 0.5, 440.0, -20.0, 2);
        let lib = Library::open(&db_path, |_| {}).unwrap();
        assert_eq!(lib.search(&q("")).total, 1);
        lib.start().unwrap();
        assert!(lib.wait_idle(WAIT));
        let all = lib.search(&q(""));
        assert_eq!(all.total, 2);
        assert!(all.assets.iter().all(|a| a.status == AnalysisStatus::Ok));
    }

    #[test]
    fn watcher_imports_files_dropped_into_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        write_sine_wav(&dir.path().join("first.wav"), 0.3, 440.0, -20.0, 2);
        let lib = Library::in_memory(|_| {}).unwrap();
        lib.start().unwrap();
        lib.add_folder(dir.path()).unwrap();
        assert!(lib.wait_idle(WAIT));
        assert_eq!(lib.search(&q("")).total, 1);

        write_sine_wav(&dir.path().join("dropped.wav"), 0.3, 440.0, -20.0, 2);
        let deadline = Instant::now() + Duration::from_secs(20);
        while lib.search(&q("dropped")).total == 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(lib.search(&q("dropped")).total, 1, "watcher did not pick up the new file");
    }

    /// Milestone 1 acceptance: a 10,000-file library imports.
    #[test]
    fn ten_thousand_file_library_imports() {
        let dir = tempfile::tempdir().unwrap();
        // One real file, hard-linked/copied 10,000 times across 100 folders.
        let seed = dir.path().join("seed.wav");
        write_sine_wav(&seed, 0.25, 440.0, -20.0, 1);
        let root = dir.path().join("library");
        for i in 0..10_000 {
            let sub = root.join(format!("pack {:03}", i % 100));
            std::fs::create_dir_all(&sub).unwrap();
            let target = sub.join(format!("sound {i:05}.wav"));
            if std::fs::hard_link(&seed, &target).is_err() {
                std::fs::copy(&seed, &target).unwrap();
            }
        }

        let lib = Library::in_memory(|_| {}).unwrap();
        let started = Instant::now();
        lib.add_folder(&root).unwrap();
        // Files are listed (and searchable) as soon as the walk finishes.
        while lib.asset_count() < 10_000 && started.elapsed() < Duration::from_secs(120) {
            thread::sleep(Duration::from_millis(20));
        }
        let listed = started.elapsed();
        assert_eq!(lib.asset_count(), 10_000);

        assert!(lib.wait_idle(Duration::from_secs(600)));
        let analyzed = started.elapsed();
        println!("10,000 files: listed in {listed:.2?}, fully analysed in {analyzed:.2?}");

        let r = lib.search(&Query {
            text: "sound 00420".into(),
            limit: 50,
            ..Default::default()
        });
        assert_eq!(r.total, 1);
        let all = lib.search(&q(""));
        assert_eq!(all.total, 10_000);
        assert!(all.assets.iter().all(|a| a.status == AnalysisStatus::Ok));
    }
}

#[cfg(test)]
mod channel_tag_tests {
    use super::*;
    use crate::analyze::testutil::write_sine_wav;

    #[test]
    fn new_files_get_a_channel_tag_from_their_folder() {
        let dir = tempfile::tempdir().unwrap();
        for sub in ["Music/Taverns", "Ambience", "SFX", "Misc"] {
            std::fs::create_dir_all(dir.path().join(sub)).unwrap();
        }
        write_sine_wav(&dir.path().join("Music/Taverns/reel.wav"), 0.5, 220.0, -12.0, 1);
        write_sine_wav(&dir.path().join("Ambience/rain.wav"), 0.5, 220.0, -12.0, 1);
        write_sine_wav(&dir.path().join("SFX/door.wav"), 0.5, 220.0, -12.0, 1);
        write_sine_wav(&dir.path().join("Misc/thing.wav"), 0.5, 220.0, -12.0, 1);

        let lib = Library::in_memory(|_| {}).unwrap();
        lib.start().unwrap();
        lib.add_folder(dir.path()).unwrap();
        assert!(lib.wait_idle(Duration::from_secs(20)));

        let tags_of = |name: &str| -> Vec<String> {
            let found = lib.search(&Query { text: name.into(), ..Default::default() });
            found.assets[0].tags.iter().map(|t| t.name.clone()).collect()
        };
        assert_eq!(tags_of("reel"), ["Music"]);
        assert_eq!(tags_of("rain"), ["Ambience"]);
        assert_eq!(tags_of("door"), ["Sound effects"]);
        assert!(tags_of("thing").is_empty());

        // A tag the GM removes is not put back by a rescan.
        let reel = lib.search(&Query { text: "reel".into(), ..Default::default() }).assets[0].id;
        lib.set_tags(reel, &[]).unwrap();
        lib.rescan(None).unwrap();
        assert!(lib.wait_idle(Duration::from_secs(20)));
        assert!(tags_of("reel").is_empty());

        // Tag filter and bulk tagging.
        let all: Vec<i64> = lib.search(&Query::default()).assets.iter().map(|a| a.id).collect();
        let tag = Tag { name: "session 3".into(), category: "custom".into() };
        lib.set_tag_on(&all, &tag, true).unwrap();
        let found = lib.search(&Query { tags: vec!["session 3".into()], ..Default::default() });
        assert_eq!(found.total, 4);
    }
}
