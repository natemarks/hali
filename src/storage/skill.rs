//! The skill directories (spec §11.3): the check and the atomic writes of `hali skill install`,
//! and the bounded read of an installed `SKILL.md` for `describe`.
//!
//! A skill dir is `<base>/<a>/<b>/<own>` (`$HOME/.claude/skills/hali`): `<base>` must be a
//! directory and is never created; `<a>` and `<b>` are followed when they are symlinks (the
//! user's choice) and created when missing; `<own>` and every directory below it are hali's own:
//! never followed and created when missing. `<base>` (HOME) must be owned by the effective user
//! (its mode is not checked); every other directory there must be owned by the effective user
//! and not group- or world-writable. Everything is checked through directory descriptors before
//! anything is written, and every write goes through them too: a file is never opened for writing
//! by name, only a fresh temp file created with `O_EXCL|O_NOFOLLOW` and renamed over it.

use super::store_file::{dir_problem, euid};
use std::ffi::{CStr, CString};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// The most bytes of an installed file read to compare it (1 MiB).
const MAX_INSTALLED: u64 = 1 << 20;

/// The most bytes of an installed `SKILL.md` read for `describe` (64 KiB).
const MAX_DESCRIBED: u64 = 64 << 10;

/// The directories between `<base>` and `<own>` that are followed and may be created.
const FOLLOWED: usize = 2;

/// Why a skill dir cannot be used: `reason` is `not_a_directory`, `symlink`, `not_a_file`,
/// `unsafe` or `cannot_create`, and `path` the offending path.
#[derive(Debug)]
pub struct Unusable {
    pub path: PathBuf,
    pub reason: &'static str,
    /// `path` is `<base>` (HOME) itself.
    pub home: bool,
    /// For `unsafe`: `dir_problem`'s reason (`not owned by you`, `writable by group or others`).
    pub problem: Option<&'static str>,
}

fn unusable(path: &Path, reason: &'static str) -> Unusable {
    Unusable {
        path: path.to_path_buf(),
        reason,
        home: false,
        problem: None,
    }
}

fn not_safe(path: &Path, problem: &'static str) -> Unusable {
    Unusable {
        problem: Some(problem),
        ..unusable(path, "unsafe")
    }
}

/// One file of a checked skill dir.
pub struct Planned {
    /// Its path below the skill dir.
    pub path: PathBuf,
    /// The bytes an install writes.
    pub new: Vec<u8>,
    /// The bytes there now (at most 1 MiB and one byte of them); `None` when there is no file;
    /// empty when the file there is replaced whatever its bytes (it cannot be read, is not the
    /// effective user's, or is group- or world-writable). In a location that is the same
    /// directory as an earlier one, the new bytes: the earlier one writes them.
    pub current: Option<Vec<u8>>,
    dir: usize,
    name: CString,
}

struct Dir {
    path: PathBuf,
    name: CString,
    parent: usize,
    own: bool,
    /// The open directory; `None` while it is still to be created.
    fd: Option<File>,
}

/// A checked skill dir: its directories (the base first, every parent before its children) and
/// its files, in the order given.
pub struct Checked {
    dirs: Vec<Dir>,
    pub files: Vec<Planned>,
    /// The same directory as an earlier location (through a followed symlink): not written.
    alias: bool,
}

/// Checks the skill dirs `dirs` and the files `files` in each (paths relative to it,
/// `/`-separated, with their new bytes), and reads the files there now. Writes nothing. A dir
/// whose `<a>` or `<b>` is the same directory as an earlier dir's (same device and inode) is that
/// dir again: its files read as already holding their new bytes, and `install` skips it.
pub fn check(dirs: &[PathBuf], files: &[(&str, &[u8])]) -> Result<Vec<Checked>, Unusable> {
    let mut checked: Vec<Checked> = Vec::new();
    for dir in dirs {
        let mut c = check_one(dir, files)?;
        if checked.iter().any(|earlier| same_dir(earlier, &c)) {
            c.alias = true;
            for file in &mut c.files {
                file.current = Some(file.new.clone());
            }
        }
        checked.push(c);
    }
    Ok(checked)
}

/// Whether `a` and `b` have the same open directory at the same followed depth.
fn same_dir(a: &Checked, b: &Checked) -> bool {
    (1..=FOLLOWED).any(|i| match (&a.dirs[i].fd, &b.dirs[i].fd) {
        (Some(x), Some(y)) => match (x.metadata(), y.metadata()) {
            (Ok(x), Ok(y)) => (x.dev(), x.ino()) == (y.dev(), y.ino()),
            _ => false,
        },
        _ => false,
    })
}

fn check_one(dir: &Path, files: &[(&str, &[u8])]) -> Result<Checked, Unusable> {
    let base = dir
        .ancestors()
        .nth(FOLLOWED + 1)
        .expect("a skill dir is <base>/<a>/<b>/<own>");
    let at_home = |e: Unusable| Unusable { home: true, ..e };
    let base_fd = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NONBLOCK)
        .open(base)
        .map_err(|e| match e.raw_os_error() {
            Some(libc::ENOENT | libc::ENOTDIR | libc::ELOOP) => {
                at_home(unusable(base, "not_a_directory"))
            }
            _ => at_home(unusable(base, "cannot_create")),
        })?;
    // HOME must be the effective user's; its mode is not checked (private-group homes are 0775).
    let meta = base_fd
        .metadata()
        .map_err(|_| at_home(unusable(base, "cannot_create")))?;
    if meta.uid() != euid() {
        return Err(at_home(not_safe(base, "not owned by you")));
    }
    let mut dirs = vec![Dir {
        path: base.to_path_buf(),
        name: CString::default(),
        parent: 0,
        own: false,
        fd: Some(base_fd),
    }];
    for depth in (0..=FOLLOWED).rev() {
        let path = dir.ancestors().nth(depth).expect("checked above");
        add_dir(&mut dirs, path, depth == 0);
    }
    let mut planned = Vec::new();
    for (rel, bytes) in files {
        let (sub, name) = match rel.rsplit_once('/') {
            Some((sub, name)) => (Some(sub), name),
            None => (None, *rel),
        };
        let mut at = FOLLOWED + 1;
        let mut path = dir.to_path_buf();
        for part in sub.into_iter().flat_map(|s| s.split('/')) {
            path.push(part);
            at = match dirs.iter().position(|d| d.path == path) {
                Some(i) => i,
                None => add_dir(&mut dirs, &path, true),
            };
        }
        planned.push(Planned {
            path: path.join(name),
            new: bytes.to_vec(),
            current: None,
            dir: at,
            name: c_name(name.as_bytes()),
        });
    }
    // The directories, each through its parent's descriptor; below a missing one all are missing.
    for i in 1..dirs.len() {
        let opened = match &dirs[dirs[i].parent].fd {
            Some(parent) if dirs[i].own => probe_own(parent, &dirs[i].name, &dirs[i].path)?,
            Some(parent) => probe_followed(parent, &dirs[i].name, &dirs[i].path)?,
            None => None,
        };
        dirs[i].fd = opened;
    }
    for file in &mut planned {
        if let Some(fd) = &dirs[file.dir].fd {
            file.current = probe_file(fd, &file.name, &file.path)?;
        }
    }
    Ok(Checked {
        dirs,
        files: planned,
        alias: false,
    })
}

/// Creates what `check` found missing (mode 0700) and writes every file whose bytes differ,
/// atomically (mode 0600). A file that is already right is not touched. `wrote` is set once a
/// directory or file has been created or replaced, also when a later step then fails.
pub fn install(checked: &mut Checked, wrote: &mut bool) -> Result<(), Unusable> {
    if checked.alias {
        return Ok(());
    }
    for i in 1..checked.dirs.len() {
        if checked.dirs[i].fd.is_some() {
            continue;
        }
        let made = {
            let d = &checked.dirs[i];
            let parent = checked.dirs[d.parent]
                .fd
                .as_ref()
                .expect("a parent comes first, so it is open by now");
            create_dir(parent, &d.name, &d.path, wrote)?
        };
        checked.dirs[i].fd = Some(made);
    }
    for file in &checked.files {
        if file.current.as_deref() == Some(file.new.as_slice()) {
            continue;
        }
        let dir = checked.dirs[file.dir]
            .fd
            .as_ref()
            .expect("every directory is open by now");
        write_file(dir, &file.name, &file.new, &file.path)?;
        *wrote = true;
    }
    Ok(())
}

/// The text of the installed file `path` (`<base>/<a>/<b>/<own>/<file>`), for `describe`: `None`
/// when nothing is there; empty when something is there that is not read (a symlinked own dir
/// or file, a FIFO, a device, a directory, or a file that cannot be read); else at most its first
/// 64 KiB, with invalid UTF-8 replaced. Never blocks, never follows the own dir or the file.
pub fn read_installed(path: &Path) -> Option<String> {
    let own = path.parent()?;
    let skills = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NONBLOCK)
        .open(own.parent()?)
        .ok()?;
    let own_fd = match open_at(
        &skills,
        &c_name(own.file_name()?.as_bytes()),
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        0,
    ) {
        Ok(fd) => fd,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return None,
        Err(_) => return Some(String::new()),
    };
    let name = c_name(path.file_name()?.as_bytes());
    match kind_at(&own_fd, &name) {
        Ok(Kind::Missing) => return None,
        Ok(Kind::Regular) => {}
        _ => return Some(String::new()),
    }
    let Ok(file) = open_at(
        &own_fd,
        &name,
        libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_NOCTTY,
        0,
    ) else {
        return Some(String::new());
    };
    if !file.metadata().is_ok_and(|m| m.is_file()) {
        return Some(String::new());
    }
    let mut bytes = Vec::new();
    if file.take(MAX_DESCRIBED).read_to_end(&mut bytes).is_err() {
        return Some(String::new());
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn add_dir(dirs: &mut Vec<Dir>, path: &Path, own: bool) -> usize {
    let parent = dirs
        .iter()
        .rposition(|d| Some(d.path.as_path()) == path.parent())
        .expect("a parent is added before its children");
    let name = path
        .file_name()
        .expect("a skill dir is made of fixed names");
    dirs.push(Dir {
        path: path.to_path_buf(),
        name: c_name(name.as_bytes()),
        parent,
        own,
        fd: None,
    });
    dirs.len() - 1
}

fn c_name(name: &[u8]) -> CString {
    CString::new(name).expect("a fixed name has no NUL byte")
}

/// `openat(dir, name, flags | O_CLOEXEC, mode)`.
fn open_at(dir: &File, name: &CStr, flags: libc::c_int, mode: libc::c_uint) -> io::Result<File> {
    // SAFETY: `name` is NUL-terminated and outlives the call, and `dir` holds an open descriptor.
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_CLOEXEC,
            mode,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `fd` was just returned by openat and nothing else owns it.
    Ok(File::from(unsafe { OwnedFd::from_raw_fd(fd) }))
}

enum Kind {
    Missing,
    Directory,
    Regular,
    Symlink,
    Other,
}

/// What `name` in `dir` is, by `fstatat` without following a symlink.
fn kind_at(dir: &File, name: &CStr) -> io::Result<Kind> {
    // SAFETY: `stat` is a plain C struct, for which all zero bytes are a valid value.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: `name` is NUL-terminated, `st` is a valid out pointer, and `dir` is open.
    let r = unsafe {
        libc::fstatat(
            dir.as_raw_fd(),
            name.as_ptr(),
            &mut st,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if r != 0 {
        let e = io::Error::last_os_error();
        return match e.kind() {
            io::ErrorKind::NotFound => Ok(Kind::Missing),
            _ => Err(e),
        };
    }
    Ok(match st.st_mode & libc::S_IFMT {
        libc::S_IFDIR => Kind::Directory,
        libc::S_IFREG => Kind::Regular,
        libc::S_IFLNK => Kind::Symlink,
        _ => Kind::Other,
    })
}

/// A directory that is followed when it is a symlink: open (and checked by its descriptor like
/// an own directory), missing (`None`), or a problem.
fn probe_followed(parent: &File, name: &CStr, path: &Path) -> Result<Option<File>, Unusable> {
    let err = match open_at(
        parent,
        name,
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NONBLOCK,
        0,
    ) {
        Ok(fd) => {
            check_dir(&fd, path)?;
            return Ok(Some(fd));
        }
        Err(e) => e,
    };
    match kind_at(parent, name) {
        Ok(Kind::Missing) => Ok(None),
        // A file, a symlink to one, a dangling or looping symlink.
        _ if matches!(
            err.raw_os_error(),
            Some(libc::ENOTDIR | libc::ENOENT | libc::ELOOP)
        ) =>
        {
            Err(unusable(path, "not_a_directory"))
        }
        _ => Err(unusable(path, "cannot_create")),
    }
}

/// One of hali's own directories: never followed, and checked by its descriptor.
fn probe_own(parent: &File, name: &CStr, path: &Path) -> Result<Option<File>, Unusable> {
    match open_at(
        parent,
        name,
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        0,
    ) {
        Ok(fd) => {
            check_dir(&fd, path)?;
            Ok(Some(fd))
        }
        Err(_) => match kind_at(parent, name) {
            Ok(Kind::Missing) => Ok(None),
            Ok(Kind::Symlink) => Err(unusable(path, "symlink")),
            Ok(Kind::Regular | Kind::Other) => Err(unusable(path, "not_a_directory")),
            Ok(Kind::Directory) | Err(_) => Err(unusable(path, "cannot_create")),
        },
    }
}

/// A directory written into, by its descriptor: a directory of the effective user, not group-
/// or world-writable.
fn check_dir(fd: &File, path: &Path) -> Result<(), Unusable> {
    let meta = fd.metadata().map_err(|_| unusable(path, "cannot_create"))?;
    if !meta.is_dir() {
        return Err(unusable(path, "not_a_directory"));
    }
    match dir_problem(meta.uid(), meta.mode(), euid()) {
        Some(problem) => Err(not_safe(path, problem)),
        None => Ok(()),
    }
}

/// The bytes of the file `name` in the own directory `dir`: `None` when there is none. A
/// symlink, a directory, a FIFO or a device is refused without being opened for reading.
fn probe_file(dir: &File, name: &CStr, path: &Path) -> Result<Option<Vec<u8>>, Unusable> {
    match kind_at(dir, name) {
        Ok(Kind::Missing) => return Ok(None),
        Ok(Kind::Regular) => {}
        Ok(Kind::Symlink) => return Err(unusable(path, "symlink")),
        Ok(Kind::Directory | Kind::Other) => return Err(unusable(path, "not_a_file")),
        Err(_) => return Err(unusable(path, "cannot_create")),
    }
    // O_NOFOLLOW and O_NONBLOCK: a symlink or a FIFO swapped in since is not followed and cannot
    // block.
    let file = match open_at(
        dir,
        name,
        libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_NOCTTY,
        0,
    ) {
        Ok(file) => file,
        Err(e) if e.raw_os_error() == Some(libc::ELOOP) => return Err(unusable(path, "symlink")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        // A file hali cannot read is replaced, like any other that differs.
        Err(_) => return Ok(Some(Vec::new())),
    };
    let meta = file.metadata().map_err(|_| unusable(path, "not_a_file"))?;
    if !meta.is_file() {
        return Err(unusable(path, "not_a_file"));
    }
    // Someone else's file, or one others may write, is replaced whatever its bytes.
    if dir_problem(meta.uid(), meta.mode(), euid()).is_some() {
        return Ok(Some(Vec::new()));
    }
    let mut bytes = Vec::new();
    match file.take(MAX_INSTALLED + 1).read_to_end(&mut bytes) {
        Ok(_) => Ok(Some(bytes)),
        Err(_) => Ok(Some(Vec::new())),
    }
}

/// Creates the directory `name` in `parent` with mode 0700, whatever the umask, and opens it.
fn create_dir(parent: &File, name: &CStr, path: &Path, wrote: &mut bool) -> Result<File, Unusable> {
    // SAFETY: `name` is NUL-terminated and outlives the call, and `parent` is open.
    if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
        return Err(unusable(path, "cannot_create"));
    }
    *wrote = true;
    let fd = open_at(
        parent,
        name,
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        0,
    )
    .map_err(|_| unusable(path, "cannot_create"))?;
    // Made by this process, so it is the effective user's unless it was swapped meanwhile.
    let meta = fd.metadata().map_err(|_| unusable(path, "cannot_create"))?;
    if meta.uid() != euid() {
        return Err(not_safe(path, "not owned by you"));
    }
    fd.set_permissions(std::fs::Permissions::from_mode(0o700))
        .map_err(|_| unusable(path, "cannot_create"))?;
    Ok(fd)
}

/// Publishes `bytes` as `name` in `dir` atomically: a fresh temp file (`O_CREAT|O_EXCL|
/// O_NOFOLLOW`, mode 0600 whatever the umask) is written, synced and renamed over `name`, then
/// `dir` is synced. On any error the temp file is removed and `name` keeps its old bytes.
fn write_file(dir: &File, name: &CStr, bytes: &[u8], path: &Path) -> Result<(), Unusable> {
    let mut temp = b".".to_vec();
    temp.extend_from_slice(name.to_bytes());
    temp.extend_from_slice(format!(".{}.tmp", std::process::id()).as_bytes());
    let temp = c_name(&temp);
    let mut file = open_at(
        dir,
        &temp,
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_NOCTTY,
        0o600,
    )
    .map_err(|_| unusable(path, "cannot_create"))?;
    let written = file
        .set_permissions(std::fs::Permissions::from_mode(0o600))
        .and_then(|()| file.write_all(bytes))
        .and_then(|()| file.sync_all());
    drop(file);
    let renamed = written.and_then(|()| {
        // SAFETY: both names are NUL-terminated and outlive the call, and `dir` is open.
        let r = unsafe {
            libc::renameat(
                dir.as_raw_fd(),
                temp.as_ptr(),
                dir.as_raw_fd(),
                name.as_ptr(),
            )
        };
        if r == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    });
    if renamed.is_err() {
        // SAFETY: `temp` is NUL-terminated and outlives the call, and `dir` is open.
        unsafe { libc::unlinkat(dir.as_raw_fd(), temp.as_ptr(), 0) };
        return Err(unusable(path, "cannot_create"));
    }
    // The rename is durable once the directory is synced; the bytes are in place by now.
    let _ = dir.sync_all();
    Ok(())
}
