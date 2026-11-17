//! The store lock (spec §3.3): `<data dir>/.lock`, an empty regular file of mode 0600 that is
//! never deleted. Writers hold it exclusively from before they load until they are dropped;
//! readers hold it shared while they load. A lock not got within 5 s is `Error::Locked`.

use super::store_file::{self, STORE_FILES};
use super::{Error, File, LockReason, Store, to_turtle};
use std::cell::OnceCell;
use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The lock file's name in the data dir.
const LOCK_FILE: &str = ".lock";

/// How long a process waits for the lock.
const WAIT: Duration = Duration::from_secs(5);

/// The first and the longest pause between two tries.
const FIRST_PAUSE: Duration = Duration::from_millis(10);
const LONGEST_PAUSE: Duration = Duration::from_millis(100);

/// The one way to publish a store file: a real run of a mutating command holds a `Writer`,
/// which holds the exclusive lock until it is dropped.
pub struct Writer {
    dir: PathBuf,
    /// The locked `.lock`; empty while the data dir did not exist yet (nothing was locked).
    lock: OnceCell<fs::File>,
    stall: Option<PathBuf>,
}

impl Writer {
    /// Takes the exclusive lock on `<dir>/.lock` (creating it), waiting up to 5 s. When `dir`
    /// does not exist, it locks and creates nothing: the first `save` does.
    pub fn open(dir: &Path) -> Result<Writer, Error> {
        let writer = Writer {
            dir: dir.to_path_buf(),
            lock: OnceCell::new(),
            stall: None,
        };
        match fs::metadata(dir) {
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(writer),
            Err(_) => return Err(io(dir)),
            Ok(meta) => store_file::check_dir(dir, &meta)?,
        }
        let file = acquire(dir, Mode::Exclusive)?.expect("a writer creates the lock file");
        let _ = writer.lock.set(file);
        Ok(writer)
    }

    /// A test-hooks build only: before each rename, create `marker` and sleep (spec §14.2).
    #[cfg(feature = "test-hooks")]
    pub fn stall_publish(&mut self, marker: PathBuf) {
        self.stall = Some(marker);
    }

    /// Loads the store under the lock this writer holds.
    pub fn load(&self) -> Result<Store, Error> {
        store_file::load_files(&self.dir)
    }

    /// Publishes one file's graph atomically. With no data dir at `open`, it first creates the
    /// dir (0700) and `.lock`, takes the exclusive lock and, if a store file now exists (another
    /// process wrote first), fails with `Error::Locked` of reason `changed`, writing nothing.
    pub fn save(&self, store: &Store, file: File) -> Result<(), Error> {
        if self.lock.get().is_none() {
            let lock = self.first_lock()?;
            let _ = self.lock.set(lock);
        }
        store_file::publish(
            &self.dir,
            file.name(),
            to_turtle(store, file).as_bytes(),
            self.stall.as_deref(),
        )
    }

    fn first_lock(&self) -> Result<fs::File, Error> {
        let dir = &self.dir;
        if let Some(parent) = dir.parent() {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)
                .map_err(|_| io(dir))?;
        }
        match fs::DirBuilder::new().mode(0o700).create(dir) {
            // The mode given is masked by the umask; set it so the dir is 0700 whatever it is.
            Ok(()) => {
                fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).map_err(|_| io(dir))?
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(_) => return Err(io(dir)),
        }
        let meta = fs::metadata(dir).map_err(|_| io(dir))?;
        store_file::check_dir(dir, &meta)?;
        let lock = acquire(dir, Mode::Exclusive)?.expect("a writer creates the lock file");
        for name in STORE_FILES {
            match fs::symlink_metadata(dir.join(format!("{name}.ttl"))) {
                Err(e) if e.kind() == ErrorKind::NotFound => {}
                _ => {
                    return Err(Error::Locked {
                        path: dir.join(LOCK_FILE),
                        reason: LockReason::Changed,
                    });
                }
            }
        }
        Ok(lock)
    }
}

/// The shared lock a reader holds while it loads; `None` when there is no data dir or no
/// `.lock` in it (then nothing is locked, and nothing is created).
pub(super) fn shared(dir: &Path) -> Result<Option<fs::File>, Error> {
    match fs::metadata(dir) {
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(io(dir)),
        Ok(meta) => store_file::check_dir(dir, &meta)?,
    }
    acquire(dir, Mode::Shared)
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Shared,
    Exclusive,
}

/// Opens `<dir>/.lock` (an exclusive locker creates it, 0600) and locks it, trying every 10 to
/// 100 ms for up to 5 s. A `.lock` that is not a regular file is `Error::Io`; it is never
/// followed or opened blocking. `None` when a shared locker finds no `.lock`.
fn acquire(dir: &Path, mode: Mode) -> Result<Option<fs::File>, Error> {
    let path = dir.join(LOCK_FILE);
    let io = || Error::Io { path: path.clone() };
    let mut options = fs::OpenOptions::new();
    options.read(true);
    if mode == Mode::Exclusive {
        options.write(true).create(true).truncate(false).mode(0o600);
    }
    // O_NOFOLLOW and O_NONBLOCK: a symlink is never followed and a FIFO never blocks the open.
    // O_CLOEXEC: the lock is not inherited across exec.
    let file = match options
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(&path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == ErrorKind::NotFound && mode == Mode::Shared => return Ok(None),
        Err(_) => return Err(io()),
    };
    let meta = file.metadata().map_err(|_| io())?;
    if !meta.is_file() {
        return Err(io());
    }
    if mode == Mode::Exclusive && meta.permissions().mode() & 0o777 != 0o600 {
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| io())?;
    }

    let started = Instant::now();
    let mut pause = FIRST_PAUSE;
    loop {
        let tried = match mode {
            Mode::Shared => file.try_lock_shared(),
            Mode::Exclusive => file.try_lock(),
        };
        match tried {
            Ok(()) => return Ok(Some(file)),
            Err(fs::TryLockError::WouldBlock) => {}
            Err(fs::TryLockError::Error(_)) => return Err(io()),
        }
        let waited = started.elapsed();
        if waited >= WAIT {
            return Err(Error::Locked {
                path,
                reason: LockReason::Timeout,
            });
        }
        std::thread::sleep(pause.min(WAIT - waited));
        pause = (pause * 2).min(LONGEST_PAUSE);
    }
}

fn io(path: &Path) -> Error {
    Error::Io {
        path: path.to_path_buf(),
    }
}
