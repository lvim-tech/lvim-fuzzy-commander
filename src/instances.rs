//! Several lvc at once: each listens on a socket of its own, `$XDG_RUNTIME_DIR/lvim-commander/<pid>.sock`,
//! so the others (and `lvc --list` / `lvc --send`) find it and talk to it.
//!
//! The protocol is the `control` actions' one, a line each: `cd PATH`, `other-cd PATH`, `select NAME`,
//! `mark NAME`, `reload`, `message TEXT`, `error TEXT` — plus `status`, answered with one line:
//! `<pid>\t<left dir>\t<right dir>\t<active 0|1>`.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Where the sockets are: the runtime directory (private to the user), else a per-user one in /tmp.
pub fn dir() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()) {
        Some(d) => PathBuf::from(d).join("lvim-commander"),
        // SAFETY: getuid cannot fail.
        None => std::env::temp_dir().join(format!("lvim-commander-{}", unsafe { libc::getuid() })),
    }
}

/// This instance's socket; removed when dropped (lvc ends).
pub struct Server {
    path: PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Listen: `status` is answered from `status` (kept current by the UI), every other line goes to
/// `on_lines` (one connection's lines at once).
pub fn serve<F>(status: Arc<Mutex<String>>, on_lines: F) -> Option<Server>
where
    F: Fn(Vec<String>) + Send + 'static,
{
    let d = dir();
    std::fs::create_dir_all(&d).ok()?;
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700));
    let path = d.join(format!("{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).ok()?;
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut s) = conn else { continue };
            let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
            let mut lines = Vec::new();
            let mut r = BufReader::new(match s.try_clone() {
                Ok(c) => c,
                Err(_) => continue,
            });
            let mut line = String::new();
            while r.read_line(&mut line).is_ok_and(|n| n > 0) {
                let l = line.trim_end().to_string();
                line.clear();
                if l == "status" {
                    let st = status.lock().unwrap().clone();
                    let _ = writeln!(s, "{st}");
                    let _ = s.flush();
                    // a status question is the whole conversation
                    break;
                }
                if !l.is_empty() {
                    lines.push(l);
                }
            }
            if !lines.is_empty() {
                on_lines(lines);
            }
        }
    });
    Some(Server { path })
}

/// Another running lvc.
#[derive(Clone, Debug)]
pub struct Peer {
    pub pid: u32,
    pub left: String,
    pub right: String,
    pub active: usize,
}

impl Peer {
    /// The directory of its active panel.
    pub fn dir(&self) -> &str {
        if self.active == 1 { &self.right } else { &self.left }
    }
}

fn ask_status(path: &std::path::Path) -> Option<Peer> {
    let mut s = UnixStream::connect(path).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(500))).ok()?;
    writeln!(s, "status").ok()?;
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).ok()?;
    let f: Vec<&str> = line.trim_end().split('\t').collect();
    Some(Peer {
        pid: f.first()?.parse().ok()?,
        left: f.get(1)?.to_string(),
        right: f.get(2)?.to_string(),
        active: f.get(3).and_then(|a| a.parse().ok()).unwrap_or(0),
    })
}

/// The other running instances (not this one); a socket nobody answers is left over: removed.
pub fn peers() -> Vec<Peer> {
    let me = std::process::id();
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir()) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        let Some(pid) = p.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        if pid == me || p.extension().is_none_or(|x| x != "sock") {
            continue;
        }
        match ask_status(&p) {
            Some(peer) => out.push(peer),
            None => {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
    out.sort_by_key(|p| p.pid);
    out
}

/// Send `lines` to instance `pid`.
pub fn send(pid: u32, lines: &[String]) -> Result<(), String> {
    let path = dir().join(format!("{pid}.sock"));
    let mut s = UnixStream::connect(&path).map_err(|e| format!("lvc {pid}: {e}"))?;
    for l in lines {
        writeln!(s, "{l}").map_err(|e| e.to_string())?;
    }
    Ok(())
}
