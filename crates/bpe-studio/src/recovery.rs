//! Autosave and crash recovery: each running editor keeps a copy of its unsaved scene in the
//! settings folder's `recovery` folder. A copy whose editor is gone (it crashed, or the power
//! went) is offered back on the next start.
//!
//! Each editor owns one session there: `session-<pid>-<millis>.ron` (the scene), `.path` (the
//! scene's own file, or nothing) and `.lock`, which the editor holds locked for its whole life.
//! The operating system lets go of the lock when the process ends, however it ends, so a
//! session whose lock can be taken is an orphan.

use crate::files::{sibling, write_via};
use bpe_core::Scene;
use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

/// How often unsaved work is copied to the recovery folder.
pub const AUTOSAVE_EVERY: Duration = Duration::from_secs(60);
/// How often the copy a crash would save is refreshed while the scene changes.
pub const CRASH_COPY_EVERY: Duration = Duration::from_secs(1);

/// The recovery folder in the user's settings folder.
pub fn recovery_dir() -> Option<PathBuf> {
    crate::settings::settings_dir().map(|d| d.join("recovery"))
}

/// Where one session keeps its scene and its scene's own file name.
#[derive(Clone, Debug)]
pub struct SessionFiles {
    pub scene: PathBuf,
    pub path: PathBuf,
}

impl SessionFiles {
    fn of(scene: PathBuf) -> Self {
        SessionFiles { path: scene.with_extension("path"), scene }
    }

    /// Writes `scene` and the name of its own file, through temporary files named with
    /// `tmp_suffix`.
    fn write(&self, scene: &Scene, original: Option<&Path>, tmp_suffix: &str) -> std::io::Result<()> {
        let original = original.map(|p| p.display().to_string()).unwrap_or_default();
        write_via(&self.path, &original, false, tmp_suffix)?;
        write_via(&self.scene, &bpe_core::save(scene), false, tmp_suffix)
    }

    fn remove(&self) {
        let _ = std::fs::remove_file(&self.scene);
        let _ = std::fs::remove_file(&self.path);
    }
}

enum Job {
    Write(Box<Scene>, Option<PathBuf>),
    Clear,
    Flush(Sender<()>),
}

/// This editor's own autosave session. The scene is written on a thread of its own, so the
/// editor only clones it.
pub struct Session {
    files: SessionFiles,
    lock_path: PathBuf,
    /// Held locked while the editor runs.
    lock: Option<File>,
    jobs: Option<Sender<Job>>,
    /// How each write went: `Ok` or why it failed.
    outcomes: Receiver<Result<(), String>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Session {
    /// Starts this editor's session in `dir`, making the folder if need be.
    pub fn start(dir: &Path) -> std::io::Result<Session> {
        std::fs::create_dir_all(dir)?;
        let millis = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_millis();
        let stem = format!("session-{}-{millis}", std::process::id());
        let lock_path = dir.join(format!("{stem}.lock"));
        let lock = File::create(&lock_path)?;
        // Another editor starting at the same moment may be testing this new lock: try again.
        let mut tries = 0;
        while let Err(error) = lock.try_lock() {
            match error {
                TryLockError::WouldBlock if tries < 10 => {
                    tries += 1;
                    std::thread::sleep(Duration::from_millis(20));
                }
                TryLockError::WouldBlock => return Err(std::io::Error::other("the recovery file is in use")),
                TryLockError::Error(e) => return Err(e),
            }
        }
        let files = SessionFiles::of(dir.join(format!("{stem}.ron")));
        let (jobs, received) = channel::<Job>();
        let (told, outcomes) = channel();
        let worker_files = files.clone();
        let worker = std::thread::Builder::new().name("autosave".into()).spawn(move || {
            for job in received {
                match job {
                    Job::Write(scene, original) => {
                        let written = worker_files.write(&scene, original.as_deref(), ".tmp");
                        let _ = told.send(written.map_err(|e| e.to_string()));
                    }
                    Job::Clear => worker_files.remove(),
                    Job::Flush(done) => _ = done.send(()),
                }
            }
        })?;
        Ok(Session { files, lock_path, lock: Some(lock), jobs: Some(jobs), outcomes, worker: Some(worker) })
    }

    /// Copies `scene` (whose own file is `original`) to the recovery folder, on the session's
    /// thread.
    pub fn write(&self, scene: Scene, original: Option<PathBuf>) {
        self.send(Job::Write(Box::new(scene), original));
    }

    /// Deletes the copy: the work is saved, or was thrown away on purpose.
    pub fn clear(&self) {
        self.send(Job::Clear);
    }

    fn send(&self, job: Job) {
        if let Some(jobs) = &self.jobs {
            let _ = jobs.send(job);
        }
    }

    /// How the last write since the last ask went, if one finished.
    pub fn outcome(&self) -> Option<Result<(), String>> {
        self.outcomes.try_iter().last()
    }

    pub fn files(&self) -> &SessionFiles {
        &self.files
    }

    /// Waits until everything sent so far is written.
    pub fn flush(&self) {
        let (done, wait) = channel();
        self.send(Job::Flush(done));
        let _ = wait.recv();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Finish what was sent, then let go of the lock and tidy its file away.
        self.jobs = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        drop(self.lock.take());
        let _ = std::fs::remove_file(&self.lock_path);
    }
}

/// A copy left by an editor that is gone.
#[derive(Clone, Debug, PartialEq)]
pub struct Orphan {
    pub scene_file: PathBuf,
    /// The scene's own file, if it had one.
    pub original: Option<PathBuf>,
    /// When it was last written.
    pub saved_at: SystemTime,
}

impl Orphan {
    pub fn read(&self) -> std::io::Result<String> {
        std::fs::read_to_string(&self.scene_file)
    }

    /// Deletes the copy (it was restored, or discarded).
    pub fn discard(&self) {
        SessionFiles::of(self.scene_file.clone()).remove();
        let _ = std::fs::remove_file(self.scene_file.with_extension("lock"));
    }

    /// Keeps a copy that couldn't be read, renamed so it is never offered again.
    pub fn set_aside(&self) {
        let _ = std::fs::rename(&self.scene_file, sibling(&self.scene_file, ".broken"));
        self.discard();
    }
}

/// The copies in `dir` whose editors are gone, newest first.
pub fn orphans(dir: &Path) -> Vec<Orphan> {
    let mut found: Vec<Orphan> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|p| {
            p.extension().is_some_and(|e| e == "ron")
                && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("session-"))
        })
        .filter(|p| !held(&p.with_extension("lock")))
        .filter_map(|scene_file| {
            let saved_at = std::fs::metadata(&scene_file).and_then(|m| m.modified()).ok()?;
            let original = std::fs::read_to_string(scene_file.with_extension("path")).unwrap_or_default();
            let original = Some(original.trim()).filter(|p| !p.is_empty()).map(PathBuf::from);
            Some(Orphan { scene_file, original, saved_at })
        })
        .collect();
    found.sort_by_key(|o| std::cmp::Reverse(o.saved_at));
    found
}

/// How old a lock file must be before [`tidy`] may delete it: a younger one may belong to an
/// editor that is starting.
const TIDY_AFTER: Duration = Duration::from_secs(30);

/// Deletes the lock files of editors that are gone and left no work behind.
pub fn tidy(dir: &Path) {
    for lock in std::fs::read_dir(dir).into_iter().flatten().flatten().map(|entry| entry.path()) {
        let session = lock.file_name().is_some_and(|n| n.to_string_lossy().starts_with("session-"));
        let old = std::fs::metadata(&lock)
            .and_then(|m| m.modified())
            .is_ok_and(|t| t.elapsed().unwrap_or_default() >= TIDY_AFTER);
        if session
            && old
            && lock.extension().is_some_and(|e| e == "lock")
            && !lock.with_extension("ron").exists()
            && !held(&lock)
        {
            let _ = std::fs::remove_file(lock.with_extension("path"));
            let _ = std::fs::remove_file(&lock);
        }
    }
}

/// Whether a running editor may hold the lock file at `lock`. Only a missing file or a lock we
/// could take says no: a file that can't be opened (an antivirus scanning it) counts as held.
fn held(lock: &Path) -> bool {
    match File::options().write(true).open(lock) {
        Ok(file) => !matches!(file.try_lock(), Ok(())),
        Err(error) => error.kind() != std::io::ErrorKind::NotFound,
    }
}

/// "less than a minute ago", "3 minutes ago", "2 hours ago", "4 days ago".
pub fn how_long_ago(then: SystemTime, now: SystemTime) -> String {
    let minutes = now.duration_since(then).unwrap_or_default().as_secs() / 60;
    let plural = |n: u64, unit: &str| if n == 1 { format!("1 {unit} ago") } else { format!("{n} {unit}s ago") };
    match minutes {
        0 => "less than a minute ago".into(),
        1..60 => plural(minutes, "minute"),
        60..1440 => plural(minutes / 60, "hour"),
        _ => plural(minutes / 1440, "day"),
    }
}

/// The unsaved scene a crash would save, kept fresh by the editor.
pub struct CrashCopy {
    pub scene: Scene,
    pub original: Option<PathBuf>,
}

pub type CrashSlot = Arc<Mutex<Option<CrashCopy>>>;

/// Makes a panic on the editor's own thread (the one that would close it) save the unsaved
/// scene to its session, and a short crash report, before the usual panic message. Panics on
/// other threads (an export, the sound) don't close the editor and are left alone.
pub fn install_crash_hook(slot: CrashSlot, files: SessionFiles, report: Option<PathBuf>) {
    let editor = std::thread::current().id();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if std::thread::current().id() == editor
            && crash_save(&slot, &files)
            && let Some(report) = &report
        {
            let when = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs();
            let _ = std::fs::write(report, format!("Ball Studio stopped (unix time {when}):\n{info}\n"));
        }
        previous(info);
    }));
}

/// The crash hook's save: the slot's copy into the session files, through their own temporary
/// files. Never waits for the slot (the panic may have happened while it was held). Gives
/// whether there was unsaved work to save.
pub fn crash_save(slot: &CrashSlot, files: &SessionFiles) -> bool {
    let copy = match slot.try_lock() {
        Ok(guard) => guard,
        Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => return false,
    };
    match copy.as_ref() {
        Some(copy) => {
            let _ = files.write(&copy.scene, copy.original.as_deref(), ".crash.tmp");
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ball-studio-recovery-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn scene() -> Scene {
        bpe_kit::new_studio_scene(&bpe_kit::kit().registry)
    }

    #[test]
    fn a_running_session_is_not_an_orphan_and_becomes_one_when_its_editor_is_gone() {
        let dir = temp_dir("orphan");
        let session = Session::start(&dir).unwrap();
        session.write(scene(), Some(PathBuf::from("/scenes/Mine.ron")));
        session.flush();
        assert_eq!(session.outcome(), Some(Ok(())));
        assert!(session.files().scene.is_file());
        assert!(orphans(&dir).is_empty(), "its editor is still running");
        let scene_file = session.files().scene.clone();
        drop(session);
        let found = orphans(&dir);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].scene_file, scene_file);
        assert_eq!(found[0].original, Some(PathBuf::from("/scenes/Mine.ron")));
        assert!(bpe_core::load(&found[0].read().unwrap(), &bpe_kit::kit().registry).is_ok());
        found[0].discard();
        assert!(orphans(&dir).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn clearing_removes_the_copy_and_a_scene_without_a_file_has_no_original() {
        let dir = temp_dir("clear");
        let session = Session::start(&dir).unwrap();
        session.write(scene(), None);
        session.clear();
        session.flush();
        assert!(!session.files().scene.exists());
        session.write(scene(), None);
        session.flush();
        drop(session);
        let found = orphans(&dir);
        assert_eq!(found[0].original, None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn orphans_come_newest_first_and_a_broken_one_is_set_aside() {
        let dir = temp_dir("order");
        std::fs::create_dir_all(&dir).unwrap();
        for (name, age) in [("session-1-1.ron", 300), ("session-2-2.ron", 10), ("session-3-3.ron", 100)] {
            let path = dir.join(name);
            std::fs::write(&path, "x").unwrap();
            let when = SystemTime::now() - Duration::from_secs(age);
            File::options().write(true).open(&path).unwrap().set_modified(when).unwrap();
        }
        std::fs::write(dir.join("other.ron"), "not a session").unwrap();
        let found = orphans(&dir);
        let names: Vec<_> = found.iter().map(|o| o.scene_file.file_name().unwrap().to_owned()).collect();
        assert_eq!(names, ["session-2-2.ron", "session-3-3.ron", "session-1-1.ron"]);
        found[0].set_aside();
        assert!(dir.join("session-2-2.ron.broken").is_file(), "kept, but never offered again");
        assert_eq!(orphans(&dir).len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn tidying_removes_only_the_locks_of_gone_editors_without_work() {
        let dir = temp_dir("tidy");
        std::fs::create_dir_all(&dir).unwrap();
        let old = SystemTime::now() - Duration::from_secs(60);
        for name in ["session-1-1.lock", "session-2-2.lock", "session-3-3.lock"] {
            std::fs::write(dir.join(name), "").unwrap();
            if name != "session-3-3.lock" {
                File::options().write(true).open(dir.join(name)).unwrap().set_modified(old).unwrap();
            }
        }
        std::fs::write(dir.join("session-2-2.ron"), "work").unwrap();
        let running = Session::start(&dir).unwrap();
        tidy(&dir);
        assert!(!dir.join("session-1-1.lock").exists(), "gone and nothing left");
        assert!(dir.join("session-3-3.lock").exists(), "too new: its editor may be starting");
        assert!(dir.join("session-2-2.lock").exists(), "its work is still to be offered");
        assert!(running.lock_path.exists(), "still running");
        drop(running);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_crash_saves_the_unsaved_copy_unless_there_is_none_or_it_is_busy() {
        let dir = temp_dir("crash");
        std::fs::create_dir_all(&dir).unwrap();
        let files = SessionFiles::of(dir.join("session-9-9.ron"));
        let slot: CrashSlot = Arc::default();
        assert!(!crash_save(&slot, &files), "nothing unsaved");
        assert!(!files.scene.exists());
        *slot.lock().unwrap() = Some(CrashCopy { scene: scene(), original: Some(PathBuf::from("/s/A.ron")) });
        {
            let _held = slot.lock().unwrap();
            assert!(!crash_save(&slot, &files), "never waits for a held slot");
        }
        assert!(crash_save(&slot, &files));
        assert!(bpe_core::load(&std::fs::read_to_string(&files.scene).unwrap(), &bpe_kit::kit().registry).is_ok());
        assert_eq!(std::fs::read_to_string(&files.path).unwrap(), "/s/A.ron");
        assert!(!sibling(&files.scene, ".crash.tmp").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_unreadable_lock_counts_as_held() {
        let dir = temp_dir("held");
        std::fs::create_dir_all(dir.join("session-1-1.lock")).unwrap();
        assert!(held(&dir.join("session-1-1.lock")), "a folder can't be opened: assume an editor has it");
        assert!(!held(&dir.join("missing.lock")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn times_read_naturally() {
        let now = SystemTime::now();
        let ago = |s: u64| how_long_ago(now - Duration::from_secs(s), now);
        assert_eq!(ago(20), "less than a minute ago");
        assert_eq!(ago(60), "1 minute ago");
        assert_eq!(ago(600), "10 minutes ago");
        assert_eq!(ago(7300), "2 hours ago");
        assert_eq!(ago(200_000), "2 days ago");
    }
}
