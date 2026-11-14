//! The Unix transport (Linux and macOS): a Unix socket in a directory private
//! to the user.
//!
//! On Linux that is `$XDG_RUNTIME_DIR`, on macOS the per-user temporary folder
//! (`/var/folders/…/T`). Both are mode 0700, so nobody else can even reach the
//! socket. We still check, once connected, that the process on
//! the other end runs as us — the same promise the Windows relay makes with the
//! pipe server's SID, for the same price.

use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::CONNECT_TIMEOUT;

/// `$XDG_RUNTIME_DIR/ciel.sock`, or `/run/user/<uid>/ciel.sock` when the
/// variable is missing (a hook started from a stripped-down environment). The
/// directory must be ours and closed to everyone else, or there is no relay.
/// Must match `platform::relay_socket_path()` in the app exactly.
#[cfg(target_os = "linux")]
fn socket_path() -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() })));
    is_private_dir(&dir).then(|| dir.join("ciel.sock"))
}

/// macOS: `ciel.sock` in the per-user temporary folder, asked of the system
/// with `confstr` rather than read from `$TMPDIR`, so a hook started from any
/// environment finds the same path as the app. Must match
/// `platform::relay_socket_path()` in the app exactly.
#[cfg(target_os = "macos")]
fn socket_path() -> Option<PathBuf> {
    let dir = darwin_user_temp_dir()?;
    is_private_dir(&dir).then(|| dir.join("ciel.sock"))
}

/// The folder `confstr(_CS_DARWIN_USER_TEMP_DIR)` names, without its trailing
/// slash.
#[cfg(target_os = "macos")]
fn darwin_user_temp_dir() -> Option<PathBuf> {
    let mut buf = vec![0u8; 1024];
    let len = unsafe {
        libc::confstr(libc::_CS_DARWIN_USER_TEMP_DIR, buf.as_mut_ptr() as *mut libc::c_char, buf.len())
    };
    // 0 is an error; a length past the buffer means it was cut short.
    if len == 0 || len > buf.len() {
        return None;
    }
    buf.truncate(len - 1);
    let dir = PathBuf::from(std::ffi::OsStr::from_bytes(&buf));
    dir.is_absolute().then_some(dir)
}

/// A real directory (not a symlink), owned by us, no access for group or others.
fn is_private_dir(dir: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::symlink_metadata(dir)
        .map(|m| {
            m.file_type().is_dir() && m.uid() == unsafe { libc::getuid() } && m.mode() & 0o077 == 0
        })
        .unwrap_or(false)
}

/// Opens the socket. Retries only while the app's backlog is full: any other
/// error — no socket, nobody listening — means there is nothing to talk to, and
/// waiting would only delay Claude Code.
pub fn connect() -> Option<UnixStream> {
    let path = socket_path()?;
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    loop {
        match try_connect(&path) {
            // Somebody else's server on our socket gets nothing from us.
            Ok(stream) => return server_is_same_user(&stream).then_some(stream),
            Err(err) => {
                if err.raw_os_error() != Some(libc::EAGAIN) || Instant::now() >= deadline {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(15));
            }
        }
    }
}

/// A non-blocking connect, so a full backlog answers EAGAIN at once instead of
/// parking us until the app gets round to accepting. Blocking again afterwards:
/// the main thread's budget bounds every read and write.
fn try_connect(path: &Path) -> io::Result<UnixStream> {
    let stream = new_socket()?;
    let fd = stream.as_raw_fd();

    let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
    let bytes = path.as_os_str().as_bytes();
    // Room for the terminating NUL, which zeroed() already put there.
    if bytes.len() >= addr.sun_path.len() {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    for (dst, src) in addr.sun_path.iter_mut().zip(bytes) {
        *dst = *src as libc::c_char;
    }

    let rc = unsafe {
        libc::connect(
            fd,
            &addr as *const libc::sockaddr_un as *const libc::sockaddr,
            std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    stream.set_nonblocking(false)?;
    Ok(stream)
}

/// A close-on-exec, non-blocking Unix stream socket, in one call on Linux.
#[cfg(target_os = "linux")]
fn new_socket() -> io::Result<UnixStream> {
    let fd = unsafe {
        libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK, 0)
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // Owns the descriptor from here on, so every early return closes it.
    Ok(unsafe { UnixStream::from_raw_fd(fd) })
}

/// macOS has no SOCK_CLOEXEC or SOCK_NONBLOCK: the same socket, with both
/// flags set by fcntl straight after. Nothing else in the hook forks, so the
/// gap between the two calls leaks nothing. SO_NOSIGPIPE keeps a server that
/// hangs up mid-write from killing us with SIGPIPE.
#[cfg(target_os = "macos")]
fn new_socket() -> io::Result<UnixStream> {
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // Owns the descriptor from here on, so every early return closes it.
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let on: libc::c_int = 1;
    let rc = unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_NOSIGPIPE,
            &on as *const libc::c_int as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    stream.set_nonblocking(true)?;
    Ok(stream)
}

/// True when the process serving the socket runs as the same user we do.
/// A failure to answer is treated as "not ours", as on Windows.
#[cfg(target_os = "linux")]
fn server_is_same_user(stream: &UnixStream) -> bool {
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut libc::ucred as *mut libc::c_void,
            &mut len,
        )
    };
    rc == 0
        && len as usize == std::mem::size_of::<libc::ucred>()
        && cred.uid == unsafe { libc::getuid() }
}

/// macOS: the same check through getpeereid, which reads the peer's
/// credentials the kernel recorded at connect time.
#[cfg(target_os = "macos")]
fn server_is_same_user(stream: &UnixStream) -> bool {
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    let rc = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) };
    rc == 0 && uid == unsafe { libc::getuid() }
}
