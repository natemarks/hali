//! hali's state (spec §4, §7): the remembered Scan roots in `<state dir>/roots.json`, outside
//! the store. Reads are bounded and never block or follow a symlink; writes are atomic.

use super::Error;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// The file of the remembered Scan roots, in the state dir.
pub const ROOTS_FILE: &str = "roots.json";

/// The most bytes of `roots.json` read (1 MiB).
const MAX_ROOTS_FILE: u64 = 1 << 20;

/// The text of `<dir>/roots.json`; `None` when `dir` or the file does not exist. A file that is
/// not a regular file (a symlink, a FIFO, a directory), holds more than 1 MiB or is not UTF-8 is
/// `Error::Io` with the file's path.
pub fn read_roots(dir: &Path) -> Result<Option<String>, Error> {
    let path = dir.join(ROOTS_FILE);
    let io = || Error::Io { path: path.clone() };
    // A first look without opening, so a FIFO or device is never opened.
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(io()),
        Ok(meta) if !meta.is_file() => return Err(io()),
        Ok(_) => {}
    }
    // O_NONBLOCK and O_NOFOLLOW: a FIFO or a symlink swapped in since cannot block or be
    // followed. O_NOCTTY: a terminal never becomes the controlling one.
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_NOCTTY)
        .open(&path)
        .map_err(|_| io())?;
    let meta = file.metadata().map_err(|_| io())?;
    if !meta.is_file() || meta.len() > MAX_ROOTS_FILE {
        return Err(io());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_ROOTS_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| io())?;
    if bytes.len() as u64 > MAX_ROOTS_FILE {
        return Err(io());
    }
    String::from_utf8(bytes).map(Some).map_err(|_| io())
}

/// The temp file `write_roots` writes before its rename.
fn temp_file(dir: &Path) -> PathBuf {
    dir.join(format!(".{ROOTS_FILE}.{}.tmp", std::process::id()))
}

/// Makes `dir` ready for `write_roots`: creates it with mode 0700 if needed, then proves a file
/// can be created in it (and removes that file). `Error::Io` with `dir` when it cannot.
pub fn prepare(dir: &Path) -> Result<(), Error> {
    let io = || Error::Io {
        path: dir.to_path_buf(),
    };
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(|_| io())?;
    let temp = temp_file(dir);
    let _ = fs::remove_file(&temp);
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NOCTTY)
        .open(&temp)
        .map_err(|_| io())?;
    fs::remove_file(&temp).map_err(|_| io())
}

/// Writes `<dir>/roots.json` atomically: a temp file in `dir` (mode 0600), then a rename.
/// Creates `dir` with mode 0700. On a failure the temp file is removed and what was there stays.
pub fn write_roots(dir: &Path, text: &str) -> Result<(), Error> {
    let io = |path: &Path| Error::Io {
        path: path.to_path_buf(),
    };
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(|_| io(dir))?;
    let path = dir.join(ROOTS_FILE);
    let temp = temp_file(dir);
    // A temp file left by a crashed run of this pid would block create_new.
    let _ = fs::remove_file(&temp);
    let written = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NOCTTY)
            .open(&temp)?;
        // The mode given to open is masked by the umask; 0600 needs nothing it could remove,
        // but set it anyway so the file is private whatever the umask.
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temp, &path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temp);
        return Err(io(&path));
    }
    Ok(())
}
