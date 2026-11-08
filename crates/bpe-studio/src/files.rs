//! Writing scene files so that a cut-off save never leaves a half-written file: the text goes
//! to a temporary file beside it, which is then swapped in, keeping the version before as a
//! backup.

use std::io::Write;
use std::path::{Path, PathBuf};

/// Writes `text` to `path` safely: to `<name>.tmp` first (flushed to the disk), then swapped
/// in. With `backup`, the file that was there is copied to [`backup_path`] first. On failure
/// the file at `path` is untouched and no temporary file is left.
pub fn write_safely(path: &Path, text: &str, backup: bool) -> std::io::Result<()> {
    write_via(path, text, backup, ".tmp")
}

/// [`write_safely`] through a temporary file named with `tmp_suffix`, so two writers of the same
/// file (the autosave and the crash hook) never share one.
pub fn write_via(path: &Path, text: &str, backup: bool, tmp_suffix: &str) -> std::io::Result<()> {
    let tmp = sibling(path, tmp_suffix);
    let written = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        if backup && path.is_file() {
            // A copy, not a rename, so the real file is never missing, even for a moment.
            std::fs::copy(path, backup_path(path))?;
        }
        swap_in(&tmp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// Renames `tmp` over `path`, trying again for a moment: on Windows a backup or sync program
/// (OneDrive, an antivirus, the indexer) can hold the file briefly and the rename is refused.
fn swap_in(tmp: &Path, path: &Path) -> std::io::Result<()> {
    let mut tries = 0;
    loop {
        match std::fs::rename(tmp, path) {
            Err(error) if tries < 5 && error.kind() == std::io::ErrorKind::PermissionDenied => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            done => return done,
        }
    }
}

/// Where the version before the last save is kept: `My scene.ron` keeps `My scene (backup).ron`,
/// which opens straight from the Open dialog.
pub fn backup_path(path: &Path) -> PathBuf {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let name = match path.extension() {
        Some(ext) => format!("{stem} (backup).{}", ext.to_string_lossy()),
        None => format!("{stem} (backup)"),
    };
    path.with_file_name(name)
}

/// `path` with `suffix` added to its file name.
pub fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ball-studio-files-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn backups_are_named_so_they_open_as_scenes() {
        assert_eq!(backup_path(Path::new("/a/My scene.ron")), PathBuf::from("/a/My scene (backup).ron"));
        assert_eq!(backup_path(Path::new("/a/plain")), PathBuf::from("/a/plain (backup)"));
    }

    #[test]
    fn saving_over_a_file_keeps_the_old_one_as_a_backup_and_leaves_no_temporary_file() {
        let dir = temp_dir("over");
        let path = dir.join("Scene.ron");
        write_safely(&path, "first", true).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "first");
        assert!(!backup_path(&path).exists(), "nothing to back up the first time");
        write_safely(&path, "second", true).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
        assert_eq!(std::fs::read_to_string(backup_path(&path)).unwrap(), "first");
        write_safely(&path, "third", true).unwrap();
        assert_eq!(std::fs::read_to_string(backup_path(&path)).unwrap(), "second", "one backup: the version before");
        assert!(!sibling(&path, ".tmp").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failed_write_leaves_the_file_as_it_was() {
        let dir = temp_dir("fail");
        let path = dir.join("Scene.ron");
        write_safely(&path, "kept", false).unwrap();
        // A folder where the temporary file should go makes the write fail.
        std::fs::create_dir_all(sibling(&path, ".tmp")).unwrap();
        assert!(write_safely(&path, "lost", true).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "kept");
        assert!(write_safely(&dir.join("missing").join("x.ron"), "x", true).is_err(), "a folder that isn't there");
        let _ = std::fs::remove_dir_all(dir);
    }
}
