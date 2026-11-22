//! The subshell (`Ctrl-o`, `a s`): a real shell under the panels, as Midnight Commander has. It runs on a pseudo
//! terminal and lives between visits; while it is shown lvc is out of the way (the terminal's own screen, with
//! the shell's history) and the bytes pass straight between the terminal and the shell, until Ctrl-o again.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// What ended a visit.
pub enum Left {
    /// Ctrl-o: the shell stays, lvc takes the terminal back.
    Detached,
    /// The shell ended (`exit`, Ctrl-d, killed).
    Exited,
}

pub struct Subshell {
    child: Child,
    master: File,
    /// the first visit says how to come back
    hinted: bool,
}

impl Subshell {
    /// Start `$SHELL` (else `sh`) in `dir` on a new pseudo terminal sized like the real one.
    pub fn start(dir: &Path) -> std::io::Result<Subshell> {
        let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
        let ws = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
        let (mut master, mut slave): (RawFd, RawFd) = (-1, -1);
        // SAFETY: openpty fills the two descriptors; nothing else is touched.
        let r = unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null(), &ws) };
        if r != 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: both descriptors were just made by openpty and are owned here.
        let (master, slave) = unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) };
        let shell = std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "sh".into());
        let mut cmd = Command::new(shell);
        cmd.current_dir(dir)
            .env("LVIM_COMMANDER", "1")
            .stdin(Stdio::from(slave.try_clone()?))
            .stdout(Stdio::from(slave.try_clone()?))
            .stderr(Stdio::from(slave));
        // SAFETY: only async-signal-safe calls between fork and exec: a new session, the pty as its terminal.
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = cmd.spawn()?;
        Ok(Subshell { child, master, hinted: false })
    }

    /// Type `text` into the shell as if it had been typed there.
    pub fn send(&self, text: &str) -> std::io::Result<()> {
        (&self.master).write_all(text.as_bytes())
    }

    /// Where the shell is now (its working directory), to bring the panel there.
    pub fn cwd(&self) -> Option<PathBuf> {
        std::fs::read_link(format!("/proc/{}/cwd", self.child.id())).ok()
    }

    fn resize(&self) {
        if let Ok((cols, rows)) = crossterm::terminal::size() {
            let ws = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
            // SAFETY: a plain ioctl on the pty master with a winsize that lives through the call.
            unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ as _, &ws) };
        }
    }

    /// Show the shell: pass the terminal's input to it and its output to the terminal until Ctrl-o or its end. The
    /// terminal must already be in raw mode, out of lvc's own screen.
    pub fn attach(&mut self) -> std::io::Result<Left> {
        let mut tty = OpenOptions::new().read(true).write(true).open("/dev/tty")?;
        self.resize();
        if !self.hinted {
            self.hinted = true;
            tty.write_all(b"\x1b[2m[lvc] Ctrl-o: back to the panels\x1b[0m\r\n")?;
        }
        let mut size = crossterm::terminal::size().unwrap_or((80, 24));
        // the shell redraws its prompt for the new screen (a nudge, as `Ctrl-l` would): the terminal's own screen has
        // what it drew before
        let mut buf = [0u8; 8192];
        loop {
            if let Ok(Some(_)) = self.child.try_wait() {
                return Ok(Left::Exited);
            }
            let mut fds = [
                libc::pollfd { fd: tty.as_raw_fd(), events: libc::POLLIN, revents: 0 },
                libc::pollfd { fd: self.master.as_raw_fd(), events: libc::POLLIN, revents: 0 },
            ];
            // SAFETY: two valid pollfd structs and their count.
            let n = unsafe { libc::poll(fds.as_mut_ptr(), 2, 200) };
            if n < 0 {
                let e = std::io::Error::last_os_error();
                if e.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(e);
            }
            let now = crossterm::terminal::size().unwrap_or(size);
            if now != size {
                size = now;
                self.resize();
            }
            if fds[0].revents & libc::POLLIN != 0 {
                let k = tty.read(&mut buf)?;
                if k == 0 {
                    return Ok(Left::Detached);
                }
                // Ctrl-o: what came before it goes to the shell, then lvc takes over
                if let Some(at) = buf[..k].iter().position(|&b| b == 0x0f) {
                    if at > 0 {
                        let _ = (&self.master).write_all(&buf[..at]);
                    }
                    return Ok(Left::Detached);
                }
                (&self.master).write_all(&buf[..k])?;
            }
            if fds[1].revents & (libc::POLLIN | libc::POLLHUP) != 0 {
                match (&self.master).read(&mut buf) {
                    Ok(0) | Err(_) => return Ok(Left::Exited),
                    Ok(k) => {
                        tty.write_all(&buf[..k])?;
                        tty.flush()?;
                    }
                }
            }
        }
    }
}

impl Drop for Subshell {
    fn drop(&mut self) {
        // SAFETY: a signal to the child this struct owns.
        unsafe { libc::kill(self.child.id() as i32, libc::SIGHUP) };
        let _ = self.child.wait();
    }
}
