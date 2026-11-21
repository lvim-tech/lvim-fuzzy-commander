//! The system trash, as the freedesktop.org Trash specification has it — the one file managers, `gio
//! trash` and `trash-cli` share, so what is trashed here shows in them and the other way round.
//!
//! - The HOME trash: `$XDG_DATA_HOME/Trash` (`~/.local/share/Trash`), with `files/` and `info/`.
//! - On another file system (a mount), the trash of its top directory: `$top/.Trash/$uid` when the
//!   admin made `$top/.Trash` (sticky, not a symlink), else `$top/.Trash-$uid`. A file is never COPIED
//!   to another device to trash it: it is renamed into the trash of its own file system.
//! - Each trashed `files/NAME` has `info/NAME.trashinfo`: its original path (absolute in the home trash,
//!   relative to the top directory in a mount's) and the deletion date.

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// One trashed item.
#[derive(Clone, Debug)]
pub struct Item {
    /// The trash directory it is in (the one holding `files/` and `info/`).
    pub trash: PathBuf,
    /// Its name in `files/` (and `info/<name>.trashinfo`).
    pub name: String,
    /// Where it was.
    pub original: PathBuf,
    /// `2026-11-21T10:42:07`, as written.
    pub deleted: String,
}

impl Item {
    pub fn path(&self) -> PathBuf {
        self.trash.join("files").join(&self.name)
    }

    fn info(&self) -> PathBuf {
        self.trash.join("info").join(format!("{}.trashinfo", self.name))
    }
}

fn uid() -> u32 {
    // SAFETY: getuid has no preconditions.
    unsafe { libc::getuid() }
}

/// The home trash directory.
pub fn home_trash() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::fsx::home().join(".local/share"))
        .join("Trash")
}

/// The trash a file at `path` goes to, and the directory its original path is recorded relative to
/// (None: absolute, the home trash).
fn trash_for(path: &Path) -> std::io::Result<(PathBuf, Option<PathBuf>)> {
    let home = home_trash();
    let _ = std::fs::create_dir_all(&home);
    let dev = std::fs::symlink_metadata(path)?.dev();
    let home_dev = std::fs::metadata(home.parent().unwrap_or(&home)).map(|m| m.dev());
    if home_dev.is_ok_and(|d| d == dev) {
        return Ok((home, None));
    }
    let top = crate::fsx::mount_point(path);
    // $top/.Trash/$uid — only when $top/.Trash is a real, sticky directory
    let admin = top.join(".Trash");
    if let Ok(m) = std::fs::symlink_metadata(&admin)
        && m.is_dir()
        && m.mode() & 0o1000 != 0
    {
        let t = admin.join(uid().to_string());
        if std::fs::create_dir_all(&t).is_ok() {
            return Ok((t, Some(top)));
        }
    }
    let t = top.join(format!(".Trash-{}", uid()));
    std::fs::create_dir_all(&t)?;
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&t, std::fs::Permissions::from_mode(0o700));
    Ok((t, Some(top)))
}

/// Percent-encode a path as the spec's `Path=` wants (RFC 2396: keep `/` and the unreserved set).
fn encode(p: &str) -> String {
    let mut out = String::new();
    for b in p.bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or(""), 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Move `path` into the trash of its file system; returns the trashed item.
pub fn trash(path: &Path) -> std::io::Result<Item> {
    let abs = std::path::absolute(path)?;
    // the trash of the file's own file system; where that cannot be made (its top directory belongs to
    // root, as a mount under /mnt usually does) the home trash, as the spec allows — the file is copied
    // there, then removed
    let own = trash_for(&abs).and_then(|(t, rel)| {
        std::fs::create_dir_all(t.join("files"))?;
        std::fs::create_dir_all(t.join("info"))?;
        Ok((t, rel))
    });
    let (trash, rel_to) = match own {
        Ok(v) => v,
        Err(_) => {
            let t = home_trash();
            std::fs::create_dir_all(t.join("files"))?;
            std::fs::create_dir_all(t.join("info"))?;
            (t, None)
        }
    };
    let base = abs
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let recorded = match &rel_to {
        Some(top) => abs.strip_prefix(top).unwrap_or(&abs).to_path_buf(),
        None => abs.clone(),
    };
    let deleted = crate::fsx::iso_now();
    // a unique name: the info file is created first, atomically (create_new), so two trashings of the
    // same name never take the same slot
    let mut n = 1;
    loop {
        let name = if n == 1 {
            base.clone()
        } else {
            match base.rsplit_once('.') {
                Some((stem, ext)) if !stem.is_empty() => format!("{stem}.{n}.{ext}"),
                _ => format!("{base}.{n}"),
            }
        };
        let info = trash.join("info").join(format!("{name}.trashinfo"));
        let dest = trash.join("files").join(&name);
        if dest.symlink_metadata().is_ok() {
            n += 1;
            continue;
        }
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&info) {
            Ok(mut f) => {
                use std::io::Write;
                write!(
                    f,
                    "[Trash Info]\nPath={}\nDeletionDate={deleted}\n",
                    encode(&recorded.to_string_lossy())
                )?;
                drop(f);
                match std::fs::rename(&abs, &dest) {
                    Ok(()) => {}
                    // the home trash on another file system: copy, then remove the original
                    Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
                        if let Err(e) = crate::ops::copy_tree(&abs, &dest, &mut |_| true) {
                            // nothing lost: the original is untouched, the partial copy goes
                            let _ = crate::ops::remove_tree(&dest);
                            let _ = std::fs::remove_file(&info);
                            return Err(e);
                        }
                        // the copy is whole: it stays in the trash even if the original cannot all go
                        crate::ops::remove_tree(&abs)?;
                    }
                    Err(e) => {
                        let _ = std::fs::remove_file(&info);
                        return Err(e);
                    }
                }
                return Ok(Item {
                    trash,
                    name,
                    original: abs,
                    deleted,
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(e),
        }
    }
}

/// Every trash directory: the home one and the top directories' of the mounted file systems.
pub fn trash_dirs() -> Vec<(PathBuf, Option<PathBuf>)> {
    let mut out = vec![(home_trash(), None)];
    let mounts = std::fs::read_to_string("/proc/self/mounts").unwrap_or_default();
    for line in mounts.lines() {
        let Some(mp) = line.split_whitespace().nth(1) else {
            continue;
        };
        let mp = PathBuf::from(mp.replace("\\040", " "));
        for t in [
            mp.join(".Trash").join(uid().to_string()),
            mp.join(format!(".Trash-{}", uid())),
        ] {
            if t.join("info").is_dir() && !out.iter().any(|(d, _)| d == &t) {
                out.push((t, Some(mp.clone())));
            }
        }
    }
    out
}

/// Everything in every trash, newest first.
pub fn list() -> Vec<Item> {
    let mut out = Vec::new();
    for (dir, top) in trash_dirs() {
        let Ok(rd) = std::fs::read_dir(dir.join("info")) else {
            continue;
        };
        for e in rd.flatten() {
            let fname = e.file_name().to_string_lossy().into_owned();
            let Some(name) = fname.strip_suffix(".trashinfo") else {
                continue;
            };
            let text = std::fs::read_to_string(e.path()).unwrap_or_default();
            let mut path = None;
            let mut deleted = String::new();
            for l in text.lines() {
                if let Some(v) = l.strip_prefix("Path=") {
                    path = Some(decode(v));
                } else if let Some(v) = l.strip_prefix("DeletionDate=") {
                    deleted = v.to_string();
                }
            }
            let Some(p) = path else { continue };
            let original = match &top {
                Some(t) if !p.starts_with('/') => t.join(p),
                _ => PathBuf::from(p),
            };
            // an info file without its file (removed by hand) is left out
            if dir.join("files").join(name).symlink_metadata().is_err() {
                continue;
            }
            out.push(Item {
                trash: dir.clone(),
                name: name.to_string(),
                original,
                deleted,
            });
        }
    }
    out.sort_by(|a, b| b.deleted.cmp(&a.deleted));
    out
}

/// Put `item` back where it was. Refuses to overwrite: Err(AlreadyExists) when something is there.
pub fn restore(item: &Item, to: Option<&Path>) -> std::io::Result<PathBuf> {
    let dest = to.map(Path::to_path_buf).unwrap_or_else(|| item.original.clone());
    if dest.symlink_metadata().is_ok() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("{} exists", dest.display()),
        ));
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::rename(item.path(), &dest) {
        Ok(()) => {}
        // the original place is on another file system (a moved mount): copy, then remove
        Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
            crate::ops::copy_tree(&item.path(), &dest, &mut |_| true)?;
            crate::ops::remove_tree(&item.path())?;
        }
        Err(e) => return Err(e),
    }
    let _ = std::fs::remove_file(item.info());
    Ok(dest)
}

/// Delete `item` for good.
pub fn purge(item: &Item) -> std::io::Result<()> {
    crate::ops::remove_tree(&item.path())?;
    let _ = std::fs::remove_file(item.info());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_like_the_spec() {
        assert_eq!(encode("/a b/ж.txt"), "/a%20b/%D0%B6.txt");
        assert_eq!(decode("/a%20b/%D0%B6.txt"), "/a b/ж.txt");
    }

    /// Trash two files of the same name, list them, restore one, purge the other.
    #[test]
    fn trash_list_restore_purge() {
        let base = std::env::temp_dir().join(format!("lvc-trash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("data/a")).unwrap();
        std::fs::create_dir_all(base.join("data/b")).unwrap();
        // a private home trash on the same file system as the files
        // SAFETY: the test sets the variable before any other thread reads it in this process' tests
        // of this module; other tests do not read XDG_DATA_HOME.
        unsafe { std::env::set_var("XDG_DATA_HOME", base.join("share")) };
        std::fs::write(base.join("data/a/x.txt"), "1").unwrap();
        std::fs::write(base.join("data/b/x.txt"), "2").unwrap();
        let i1 = trash(&base.join("data/a/x.txt")).unwrap();
        let i2 = trash(&base.join("data/b/x.txt")).unwrap();
        assert_ne!(i1.name, i2.name, "a second x.txt gets its own slot");
        assert!(!base.join("data/a/x.txt").exists());
        let items: Vec<Item> = list()
            .into_iter()
            .filter(|i| i.original.starts_with(&base))
            .collect();
        assert_eq!(items.len(), 2);
        let back = restore(&i1, None).unwrap();
        assert_eq!(std::fs::read_to_string(back).unwrap(), "1");
        purge(&i2).unwrap();
        assert!(list().iter().all(|i| !i.original.starts_with(&base)));
        let _ = std::fs::remove_dir_all(&base);
    }
}
