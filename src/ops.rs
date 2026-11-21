//! File operations, in Rust (no shell): copy, move, delete (to the trash or for good), make a directory,
//! rename. The long ones (copy, move across file systems, delete of a tree) run as a JOB on a worker
//! thread: it reports its progress, can be cancelled, and asks the UI what to do when a target exists.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::Arc;

/// What to do with a target that exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conflict {
    Overwrite,
    Skip,
    /// Keep both: the new one gets a free name (`a (2).txt`).
    Rename,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobKind {
    Copy,
    Move,
    Trash,
    Delete,
    /// An external program (an archiver).
    Extract,
    Compress,
}

/// How to take an operation back (Ctrl-z).
#[derive(Clone, Debug)]
pub enum Undo {
    /// `from` was moved / renamed to `to`: move it back.
    Moved { from: PathBuf, to: PathBuf },
    /// Put back from the trash.
    Trashed(crate::trash::Item),
    /// A copy or a new directory: undone by trashing it (never deleted for good).
    Created(PathBuf),
}

impl JobKind {
    pub fn label(self) -> &'static str {
        match self {
            JobKind::Copy => "Copy",
            JobKind::Move => "Move",
            JobKind::Trash => "Trash",
            JobKind::Delete => "Delete",
            JobKind::Extract => "Extract",
            JobKind::Compress => "Compress",
        }
    }
}

/// From the worker to the UI.
pub enum Report {
    /// Bytes done / total, and the file being worked on.
    Progress { done: u64, total: u64, file: String },
    /// A top-level target exists: the UI answers through `answer` (and may set "for all").
    Ask {
        target: PathBuf,
        answer: Sender<(Conflict, bool)>,
    },
    /// Finished: how many items were done, the errors met, and how to undo it.
    Done {
        count: usize,
        errors: Vec<String>,
        undo: Vec<Undo>,
    },
}

pub struct Job {
    pub kind: JobKind,
    pub rx: Receiver<Report>,
    pub cancel: Arc<AtomicBool>,
}

/// The total size of `p` (a tree is summed; symlinks count as themselves).
pub fn tree_size(p: &Path) -> u64 {
    let Ok(m) = std::fs::symlink_metadata(p) else {
        return 0;
    };
    if m.is_dir() {
        std::fs::read_dir(p)
            .map(|rd| rd.flatten().map(|e| tree_size(&e.path())).sum())
            .unwrap_or(0)
    } else {
        m.len()
    }
}

/// A free name next to `p`: `name (2).ext`, `name (3).ext`, ...
pub fn free_name(p: &Path) -> PathBuf {
    let parent = p.parent().unwrap_or(Path::new("."));
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.clone(), String::new()),
    };
    for n in 2.. {
        let c = parent.join(format!("{stem} ({n}){ext}"));
        if c.symlink_metadata().is_err() {
            return c;
        }
    }
    unreachable!()
}

/// Copy `src` to `dst` (a tree recursively; symlinks as symlinks; permissions and modification
/// times kept). `progress(bytes)` is called as data is written; returning false cancels.
pub fn copy_tree(src: &Path, dst: &Path, progress: &mut dyn FnMut(u64) -> bool) -> std::io::Result<()> {
    let m = std::fs::symlink_metadata(src)?;
    let ft = m.file_type();
    if ft.is_symlink() {
        let t = std::fs::read_link(src)?;
        if dst.symlink_metadata().is_ok() {
            remove_tree(dst)?;
        }
        std::os::unix::fs::symlink(t, dst)?;
    } else if ft.is_dir() {
        // into itself would never end
        if let (Ok(s), Ok(d)) = (std::fs::canonicalize(src), std::path::absolute(dst))
            && d.starts_with(&s)
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("cannot copy {} into itself", src.display()),
            ));
        }
        std::fs::create_dir_all(dst)?;
        for e in std::fs::read_dir(src)? {
            let e = e?;
            copy_tree(&e.path(), &dst.join(e.file_name()), progress)?;
        }
        let _ = std::fs::set_permissions(dst, m.permissions());
        set_mtime(dst, &m);
    } else {
        copy_file(src, dst, &m, progress)?;
    }
    Ok(())
}

fn set_mtime(p: &Path, m: &std::fs::Metadata) {
    if let (Ok(f), Ok(t)) = (std::fs::File::open(p), m.modified()) {
        let _ = f.set_modified(t);
    }
}

fn copy_file(
    src: &Path,
    dst: &Path,
    m: &std::fs::Metadata,
    progress: &mut dyn FnMut(u64) -> bool,
) -> std::io::Result<()> {
    use std::io::{Read, Write};
    let mut from = std::fs::File::open(src)?;
    // a partial copy is written next to the target and renamed over it at the end: a cancelled or
    // failed copy never leaves a truncated file under the real name
    let tmp = dst.with_file_name(format!(
        ".{}.lvc-part",
        dst.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
    ));
    let mut to = std::fs::File::create(&tmp)?;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = from.read(&mut buf)?;
        if n == 0 {
            break;
        }
        to.write_all(&buf[..n])?;
        if !progress(n as u64) {
            drop(to);
            let _ = std::fs::remove_file(&tmp);
            return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "cancelled"));
        }
    }
    to.flush()?;
    let _ = to.set_permissions(m.permissions());
    if let Ok(t) = m.modified() {
        let _ = to.set_modified(t);
    }
    drop(to);
    if dst.symlink_metadata().is_ok_and(|d| d.is_dir()) {
        let _ = std::fs::remove_file(&tmp);
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("{} is a directory", dst.display()),
        ));
    }
    std::fs::rename(&tmp, dst)
}

/// Remove `p` for good (a tree recursively; a symlink itself, never what it points to).
pub fn remove_tree(p: &Path) -> std::io::Result<()> {
    let m = std::fs::symlink_metadata(p)?;
    if m.is_dir() {
        std::fs::remove_dir_all(p)
    } else {
        std::fs::remove_file(p)
    }
}

/// Move `src` to `dst`: a rename on the same file system, else copy + remove.
pub fn move_path(src: &Path, dst: &Path, progress: &mut dyn FnMut(u64) -> bool) -> std::io::Result<()> {
    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
            copy_tree(src, dst, progress)?;
            remove_tree(src)
        }
        // renaming a directory over an existing one: merge by copy (rename refuses non-empty ones)
        Err(e) if dst.is_dir() && src.is_dir() && e.raw_os_error() == Some(libc::ENOTEMPTY) => {
            copy_tree(src, dst, progress)?;
            remove_tree(src)
        }
        Err(e) => Err(e),
    }
}

/// Start a job over `sources`. For Copy / Move, `dest` is the directory they go into (or, for ONE
/// source, a full new path when it does not name an existing directory).
pub fn start(kind: JobKind, sources: Vec<PathBuf>, dest: Option<PathBuf>) -> Job {
    let (tx, rx) = channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let c = cancel.clone();
    std::thread::spawn(move || run(kind, sources, dest, tx, c));
    Job { kind, rx, cancel }
}

fn run(kind: JobKind, sources: Vec<PathBuf>, dest: Option<PathBuf>, tx: Sender<Report>, cancel: Arc<AtomicBool>) {
    let total: u64 = match kind {
        JobKind::Copy | JobKind::Move => sources.iter().map(|s| tree_size(s)).sum(),
        _ => sources.len() as u64,
    };
    let mut done = 0u64;
    let mut count = 0usize;
    let mut errors = Vec::new();
    let mut undo = Vec::new();
    let mut policy: Option<Conflict> = None;
    for src in &sources {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let file = src
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let _ = tx.send(Report::Progress {
            done,
            total,
            file: file.clone(),
        });
        let result = match kind {
            JobKind::Trash => crate::trash::trash(src).map(|it| undo.push(Undo::Trashed(it))),
            JobKind::Delete => remove_tree(src),
            JobKind::Extract | JobKind::Compress => Ok(()),
            JobKind::Copy | JobKind::Move => {
                let Some(dest) = &dest else { break };
                // into an existing directory, or (one source) to a new full path
                let mut target = if dest.is_dir() {
                    dest.join(&file)
                } else {
                    dest.clone()
                };
                if std::fs::canonicalize(src).ok() == std::fs::canonicalize(&target).ok()
                    && target.symlink_metadata().is_ok()
                {
                    if kind == JobKind::Copy {
                        target = free_name(&target); // a copy onto itself: a copy next to it
                    } else {
                        continue; // moving onto itself: nothing to do
                    }
                }
                if target.symlink_metadata().is_ok() {
                    let choice = match policy {
                        Some(p) => p,
                        None => {
                            let (atx, arx) = channel();
                            let _ = tx.send(Report::Ask {
                                target: target.clone(),
                                answer: atx,
                            });
                            let (c, all) = arx.recv().unwrap_or((Conflict::Cancel, false));
                            if all {
                                policy = Some(c);
                            }
                            c
                        }
                    };
                    match choice {
                        Conflict::Cancel => break,
                        Conflict::Skip => {
                            done += tree_size(src);
                            continue;
                        }
                        Conflict::Rename => target = free_name(&target),
                        Conflict::Overwrite => {
                            // a file over a directory (or the reverse) cannot be merged: replace it
                            if target.is_dir() != src.is_dir() {
                                if let Err(e) = remove_tree(&target) {
                                    errors.push(format!("{}: {e}", target.display()));
                                    continue;
                                }
                            }
                        }
                    }
                }
                let cancel2 = cancel.clone();
                let tx2 = tx.clone();
                let f2 = file.clone();
                let mut acc = done;
                let mut progress = |n: u64| {
                    acc += n;
                    let _ = tx2.send(Report::Progress {
                        done: acc,
                        total,
                        file: f2.clone(),
                    });
                    !cancel2.load(Ordering::Relaxed)
                };
                let r = if kind == JobKind::Copy {
                    copy_tree(src, &target, &mut progress)
                } else {
                    move_path(src, &target, &mut progress)
                };
                done = acc;
                if r.is_ok() {
                    undo.push(if kind == JobKind::Copy {
                        Undo::Created(target.clone())
                    } else {
                        Undo::Moved {
                            from: src.clone(),
                            to: target.clone(),
                        }
                    });
                }
                r
            }
        };
        match result {
            Ok(()) => count += 1,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => break,
            Err(e) => errors.push(format!("{}: {e}", src.display())),
        }
        if matches!(kind, JobKind::Trash | JobKind::Delete) {
            done += 1;
        }
    }
    let _ = tx.send(Report::Done { count, errors, undo });
}

/// Run an external program (an archiver) as a job: done when it exits; its error output is the
/// error. `made` is what it creates (for undo).
pub fn start_command(kind: JobKind, mut cmd: std::process::Command, label: String, made: Option<PathBuf>) -> Job {
    let (tx, rx) = channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let c = cancel.clone();
    std::thread::spawn(move || {
        let _ = tx.send(Report::Progress {
            done: 0,
            total: 1,
            file: label.clone(),
        });
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());
        let child = cmd.spawn();
        let mut child = match child {
            Ok(ch) => ch,
            Err(e) => {
                let _ = tx.send(Report::Done {
                    count: 0,
                    errors: vec![format!("{label}: {e}")],
                    undo: Vec::new(),
                });
                return;
            }
        };
        // poll, so Esc can stop it
        let status = loop {
            if c.load(Ordering::Relaxed) {
                let _ = child.kill();
            }
            match child.try_wait() {
                Ok(Some(st)) => break Some(st),
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
                Err(_) => break None,
            }
        };
        let mut err = String::new();
        if let Some(mut e) = child.stderr.take() {
            use std::io::Read;
            let _ = e.read_to_string(&mut err);
        }
        let ok = status.is_some_and(|s| s.success());
        let _ = tx.send(Report::Done {
            count: ok as usize,
            errors: if ok {
                Vec::new()
            } else {
                let mut v: Vec<String> = err.lines().map(str::to_string).filter(|l| !l.is_empty()).collect();
                if v.is_empty() {
                    v.push(format!("{label}: failed"));
                }
                v
            },
            undo: if ok { made.into_iter().map(Undo::Created).collect() } else { Vec::new() },
        });
    });
    Job { kind, rx, cancel }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lvc-ops-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn finish(job: Job, answer: Conflict) -> (usize, Vec<String>) {
        loop {
            match job.rx.recv().unwrap() {
                Report::Ask { answer: a, .. } => a.send((answer, true)).unwrap(),
                Report::Done { count, errors, .. } => return (count, errors),
                Report::Progress { .. } => {}
            }
        }
    }

    #[test]
    fn copies_a_tree_and_resolves_conflicts() {
        let d = tmp("copy");
        std::fs::create_dir_all(d.join("src/sub")).unwrap();
        std::fs::write(d.join("src/sub/a.txt"), "new").unwrap();
        std::fs::write(d.join("src/b.txt"), "b").unwrap();
        std::os::unix::fs::symlink("b.txt", d.join("src/link")).unwrap();
        std::fs::create_dir_all(d.join("dst/src/sub")).unwrap();
        std::fs::write(d.join("dst/src/sub/a.txt"), "old").unwrap();

        // skip: the existing tree stays as it was
        let (n, e) = finish(start(JobKind::Copy, vec![d.join("src")], Some(d.join("dst"))), Conflict::Skip);
        assert_eq!((n, e.len()), (0, 0));
        assert_eq!(std::fs::read_to_string(d.join("dst/src/sub/a.txt")).unwrap(), "old");

        // overwrite: merged, the file replaced, the symlink kept a symlink
        let (n, e) = finish(start(JobKind::Copy, vec![d.join("src")], Some(d.join("dst"))), Conflict::Overwrite);
        assert_eq!((n, e.len()), (1, 0), "{e:?}");
        assert_eq!(std::fs::read_to_string(d.join("dst/src/sub/a.txt")).unwrap(), "new");
        assert!(std::fs::symlink_metadata(d.join("dst/src/link")).unwrap().file_type().is_symlink());

        // rename: a second copy next to it
        let (n, _) = finish(start(JobKind::Copy, vec![d.join("src/b.txt")], Some(d.join("src"))), Conflict::Rename);
        assert_eq!(n, 1);
        assert!(d.join("src/b (2).txt").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn moves_and_deletes() {
        let d = tmp("move");
        std::fs::create_dir_all(d.join("a")).unwrap();
        std::fs::write(d.join("a/f"), "x").unwrap();
        std::fs::create_dir_all(d.join("to")).unwrap();
        let (n, _) = finish(start(JobKind::Move, vec![d.join("a")], Some(d.join("to"))), Conflict::Cancel);
        assert_eq!(n, 1);
        assert!(d.join("to/a/f").exists() && !d.join("a").exists());
        // one source to a new full path = rename
        let (n, _) = finish(start(JobKind::Move, vec![d.join("to/a")], Some(d.join("to/b"))), Conflict::Cancel);
        assert_eq!(n, 1);
        assert!(d.join("to/b/f").exists());
        let (n, _) = finish(start(JobKind::Delete, vec![d.join("to")], None), Conflict::Cancel);
        assert_eq!(n, 1);
        assert!(!d.join("to").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn refuses_to_copy_into_itself() {
        let d = tmp("self");
        std::fs::create_dir_all(d.join("a")).unwrap();
        let (n, e) = finish(start(JobKind::Copy, vec![d.join("a")], Some(d.join("a"))), Conflict::Cancel);
        assert_eq!(n, 0);
        assert_eq!(e.len(), 1, "{e:?}");
        let _ = std::fs::remove_dir_all(&d);
    }
}
