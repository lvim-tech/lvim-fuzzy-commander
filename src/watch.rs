//! Changes made from outside: the directories the panels show (and the trash) are watched with inotify,
//! so a file another program creates, deletes, renames or writes shows at once.
//!
//! No thread: the inotify descriptor is non-blocking and read once per turn of the main loop, which wakes
//! at least every 250 ms. A burst of events (a big copy, a build) is one reload per turn.

use std::collections::HashMap;
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

const MASK: u32 = libc::IN_CREATE
    | libc::IN_DELETE
    | libc::IN_MOVED_FROM
    | libc::IN_MOVED_TO
    | libc::IN_CLOSE_WRITE
    | libc::IN_MODIFY
    | libc::IN_ATTRIB
    | libc::IN_DELETE_SELF
    | libc::IN_MOVE_SELF;

pub struct Watcher {
    fd: libc::c_int,
    /// watch descriptor → directory, and back
    by_wd: HashMap<libc::c_int, PathBuf>,
    by_dir: HashMap<PathBuf, libc::c_int>,
}

impl Watcher {
    /// None where inotify is not available (then nothing is watched; the program works as before).
    pub fn new() -> Option<Watcher> {
        // SAFETY: plain syscall; the descriptor is owned by the Watcher and closed in Drop.
        let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        (fd >= 0).then(|| Watcher {
            fd,
            by_wd: HashMap::new(),
            by_dir: HashMap::new(),
        })
    }

    /// Watch exactly `dirs` (adds the new ones, drops the ones no longer shown).
    pub fn set(&mut self, dirs: &[PathBuf]) {
        let gone: Vec<PathBuf> = self.by_dir.keys().filter(|d| !dirs.contains(d)).cloned().collect();
        for d in gone {
            if let Some(wd) = self.by_dir.remove(&d) {
                self.by_wd.remove(&wd);
                // SAFETY: a watch of our own descriptor; an already-removed one just fails.
                unsafe { libc::inotify_rm_watch(self.fd, wd) };
            }
        }
        for d in dirs {
            if self.by_dir.contains_key(d) {
                continue;
            }
            let Ok(c) = CString::new(d.as_os_str().as_bytes()) else {
                continue;
            };
            // SAFETY: a valid C path; the result is checked.
            let wd = unsafe { libc::inotify_add_watch(self.fd, c.as_ptr(), MASK | libc::IN_ONLYDIR) };
            if wd >= 0 {
                self.by_wd.insert(wd, d.clone());
                self.by_dir.insert(d.clone(), wd);
            }
        }
    }

    /// The watched directories something happened in since the last call (each once).
    pub fn changed(&mut self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = Vec::new();
        let mut buf = [0u8; 16384];
        loop {
            // SAFETY: reading into our own buffer; non-blocking, so EAGAIN ends the loop.
            let n = unsafe { libc::read(self.fd, buf.as_mut_ptr().cast(), buf.len()) };
            if n <= 0 {
                break;
            }
            let n = n as usize;
            let mut off = 0;
            let head = std::mem::size_of::<libc::inotify_event>();
            while off + head <= n {
                // SAFETY: the kernel writes whole events; read the header unaligned from the buffer.
                let ev: libc::inotify_event = unsafe { std::ptr::read_unaligned(buf.as_ptr().add(off).cast()) };
                off += head + ev.len as usize;
                if ev.mask & libc::IN_IGNORED != 0 {
                    // the directory went away (or the watch was removed): forget it
                    if let Some(d) = self.by_wd.remove(&ev.wd) {
                        self.by_dir.remove(&d);
                        if !out.contains(&d) {
                            out.push(d);
                        }
                    }
                    continue;
                }
                if let Some(d) = self.by_wd.get(&ev.wd)
                    && !out.contains(d)
                {
                    out.push(d.clone());
                }
            }
        }
        out
    }

    /// Wake the UI (`wake`) whenever an event is waiting: a thread blocked on the descriptor, so a change
    /// shows at once (the events themselves are read by `changed`, on the UI thread).
    pub fn notify_on(&self, wake: std::sync::mpsc::Sender<()>) {
        let fd = self.fd;
        std::thread::spawn(move || {
            loop {
                let mut p = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
                // SAFETY: one pollfd of ours; a closed descriptor ends the thread (POLLNVAL / error)
                let r = unsafe { libc::poll(&mut p, 1, 1000) };
                if r < 0 || p.revents & (libc::POLLNVAL | libc::POLLERR) != 0 {
                    return;
                }
                if r > 0 {
                    if wake.send(()).is_err() {
                        return;
                    }
                    // the UI reads them; give it the time to, rather than spin on a still-readable fd
                    std::thread::sleep(std::time::Duration::from_millis(30));
                }
            }
        });
    }

    #[cfg(test)]
    pub fn watches(&self, dir: &std::path::Path) -> bool {
        self.by_dir.contains_key(dir)
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        // SAFETY: our descriptor, closed once.
        unsafe { libc::close(self.fd) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sees_a_file_made_outside() {
        let base = std::env::temp_dir().join(format!("lvc-watch-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let mut w = Watcher::new().expect("inotify");
        w.set(std::slice::from_ref(&base));
        assert!(w.watches(&base));
        assert!(w.changed().is_empty());
        std::fs::write(base.join("new.txt"), "x").unwrap();
        assert_eq!(w.changed(), vec![base.clone()]);
        std::fs::remove_file(base.join("new.txt")).unwrap();
        assert_eq!(w.changed(), vec![base.clone()]);
        w.set(&[]);
        assert!(!w.watches(&base));
        std::fs::remove_dir(&base).unwrap();
    }
}
