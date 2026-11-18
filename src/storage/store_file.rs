//! The store files on disk (spec §3.3): the data dir check, bounded reads that never follow a
//! symlink or block on a FIFO, and the atomic publish of one tool-managed file.

use super::{CORE_TTL, Error, SHAPES_TTL, Store, graph, parse_into};
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

/// The most bytes of one store file read (256 MiB).
const MAX_STORE_FILE: u64 = 256 << 20;

/// The files read from the data dir, by graph name (`<name>.ttl`).
pub(super) const STORE_FILES: [&str; 3] = ["vocabulary", "resources", "extensions"];

/// Why a data dir owned by `uid` with permission bits `mode` is not used by the effective user
/// `euid`: it is someone else's, or group- or world-writable. Read and search bits are fine.
pub fn dir_problem(uid: u32, mode: u32, euid: u32) -> Option<&'static str> {
    if uid != euid {
        Some("not owned by you")
    } else if mode & 0o022 != 0 {
        Some("writable by group or others")
    } else {
        None
    }
}

/// The effective uid of this process.
fn euid() -> u32 {
    // SAFETY: geteuid takes no arguments, touches no memory and always succeeds.
    unsafe { libc::geteuid() }
}

/// Checks the data dir's metadata (from `stat`, so a symlinked data dir is followed): a
/// directory that `dir_problem` accepts.
pub(super) fn check_dir(dir: &Path, meta: &fs::Metadata) -> Result<(), Error> {
    if !meta.is_dir() {
        return Err(Error::Io {
            path: dir.to_path_buf(),
        });
    }
    match dir_problem(meta.uid(), meta.mode(), euid()) {
        Some(reason) => Err(Error::UnsafeDir {
            path: dir.to_path_buf(),
            reason,
        }),
        None => Ok(()),
    }
}

/// The embedded core ontology and shapes, and the store files in `dir`, read without a lock. A
/// missing dir or file is an empty graph.
pub(super) fn load_files(dir: &Path) -> Result<Store, Error> {
    let store = Store::new().map_err(|_| Error::Io {
        path: dir.to_path_buf(),
    })?;
    for core in [CORE_TTL, SHAPES_TTL] {
        parse_into(&store, core.as_bytes(), graph("core").as_ref().into())
            .expect("the embedded core ontology and shapes are valid Turtle");
    }
    for name in STORE_FILES {
        let file = format!("{name}.ttl");
        let Some(bytes) = read_store_file(&dir.join(&file))? else {
            continue;
        };
        parse_into(&store, &bytes, graph(name).as_ref().into())
            .map_err(|(line, column)| Error::Unparseable { file, line, column })?;
    }
    Ok(store)
}

/// The bytes of one store file; `None` when nothing is there (or its dir is missing). A file
/// that is not a regular file (a symlink, a FIFO, a directory) or holds more than 256 MiB is
/// `Error::Io` with its path, and is never followed, opened blocking or read in full.
fn read_store_file(path: &Path) -> Result<Option<Vec<u8>>, Error> {
    let io = || Error::Io {
        path: path.to_path_buf(),
    };
    // A first look without opening, so a FIFO or device is never opened.
    match fs::symlink_metadata(path) {
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
        .open(path)
        .map_err(|_| io())?;
    let meta = file.metadata().map_err(|_| io())?;
    if !meta.is_file() || meta.len() > MAX_STORE_FILE {
        return Err(io());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_STORE_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| io())?;
    if bytes.len() as u64 > MAX_STORE_FILE {
        return Err(io());
    }
    Ok(Some(bytes))
}

/// Publishes `bytes` as `<dir>/<name>` atomically: the fixed temp `<dir>/.<name>.tmp` (whatever
/// is planted there is removed, never followed; a directory there is an error) is created
/// exclusively with mode 0600, written and synced, then renamed over the target, and the dir is
/// synced. On any error before the rename the temp is removed and the old bytes stay; a failed
/// sync of the dir after it is ignored. The caller holds the
/// exclusive lock. `stall`, in a test-hooks build only, is a marker file created just before the
/// rename, after which the process sleeps.
pub(super) fn publish(
    dir: &Path,
    name: &str,
    bytes: &[u8],
    stall: Option<&Path>,
) -> Result<(), Error> {
    let target = dir.join(name);
    let io = || Error::Io {
        path: target.clone(),
    };
    match fs::symlink_metadata(&target) {
        Ok(meta) if !meta.is_file() => return Err(io()),
        Ok(_) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(_) => return Err(io()),
    }
    let temp = dir.join(format!(".{name}.tmp"));
    match fs::symlink_metadata(&temp) {
        Ok(meta) if meta.is_dir() => {
            return Err(Error::Io {
                path: temp.to_path_buf(),
            });
        }
        // A planted file or symlink is removed itself; its target is never touched.
        Ok(_) => fs::remove_file(&temp).map_err(|_| Error::Io {
            path: temp.to_path_buf(),
        })?,
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(_) => {
            return Err(Error::Io {
                path: temp.to_path_buf(),
            });
        }
    }
    let written = write_and_rename(&temp, &target, bytes, stall);
    if written.is_err() {
        let _ = fs::remove_file(&temp);
        return Err(io());
    }
    // The rename is durable once the dir is synced. O_DIRECTORY and O_NONBLOCK: only a directory
    // is opened, and the open never blocks. The bytes are published by now, so a failed sync does
    // not fail the run.
    let _ = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NONBLOCK)
        .open(dir)
        .and_then(|d| d.sync_all());
    Ok(())
}

fn write_and_rename(
    temp: &Path,
    target: &Path,
    bytes: &[u8],
    stall: Option<&Path>,
) -> std::io::Result<()> {
    // O_EXCL (create_new) and O_NOFOLLOW: a file or link planted since the removal fails the
    // open instead of being written through. O_NOCTTY: a terminal never becomes the
    // controlling one.
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NOCTTY)
        .open(temp)?;
    // The open's mode is masked by the umask; set it again so the file is 0600 whatever it is.
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    #[cfg(feature = "test-hooks")]
    if let Some(marker) = stall {
        fs::write(marker, b"")?;
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
    #[cfg(not(feature = "test-hooks"))]
    let _ = stall;
    fs::rename(temp, target)
}
