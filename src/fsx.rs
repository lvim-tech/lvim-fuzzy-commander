//! What a panel shows: the entries of ONE directory (not a walk), with what the long view needs, and
//! the small formatting helpers (sizes, permissions, dates, owner names).

use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[allow(unused_imports)]
pub use lvim_fuzzy_ui::fsinfo::{Entry, Kind, Sort, date_string, entry, human_size, list_dir, local_time, natural_cmp};

/// `drwxr-xr-x` from a mode.
pub fn mode_string(e: &Entry) -> String {
    let t = match e.kind {
        Kind::Dir => 'd',
        Kind::Link | Kind::LinkDir => 'l',
        Kind::File => '-',
        Kind::Other => {
            let f = e.mode & 0o170000;
            match f {
                0o020000 => 'c',
                0o060000 => 'b',
                0o010000 => 'p',
                0o140000 => 's',
                _ => '?',
            }
        }
    };
    let m = e.mode;
    let bit = |b: u32, c: char| if m & b != 0 { c } else { '-' };
    let mut s = String::with_capacity(10);
    s.push(t);
    s.push(bit(0o400, 'r'));
    s.push(bit(0o200, 'w'));
    s.push(match (m & 0o100 != 0, m & 0o4000 != 0) {
        (true, true) => 's',
        (false, true) => 'S',
        (true, false) => 'x',
        _ => '-',
    });
    s.push(bit(0o040, 'r'));
    s.push(bit(0o020, 'w'));
    s.push(match (m & 0o010 != 0, m & 0o2000 != 0) {
        (true, true) => 's',
        (false, true) => 'S',
        (true, false) => 'x',
        _ => '-',
    });
    s.push(bit(0o004, 'r'));
    s.push(bit(0o002, 'w'));
    s.push(match (m & 0o001 != 0, m & 0o1000 != 0) {
        (true, true) => 't',
        (false, true) => 'T',
        (true, false) => 'x',
        _ => '-',
    });
    s
}

/// A mode from what was typed: octal (`755`, `0644`) or symbolic as chmod takes it, relative to `current`
/// (`u+x`, `g-w,o-rwx`, `a=r`, `+X`, `go=` ...; `X` = execute for a directory or something already
/// executable).
pub fn parse_mode(input: &str, current: u32, is_dir: bool) -> Result<u32, String> {
    let t = input.trim();
    if t.is_empty() {
        return Err("empty mode".into());
    }
    if t.chars().all(|c| c.is_ascii_digit()) {
        let m = u32::from_str_radix(t, 8).map_err(|_| format!("not an octal mode: {t}"))?;
        return if m <= 0o7777 { Ok(m) } else { Err(format!("too large: {t}")) };
    }
    let mut mode = current & 0o7777;
    for clause in t.split(',') {
        let c: Vec<char> = clause.chars().collect();
        let mut i = 0;
        let mut who = 0u32;
        while i < c.len() && "ugoa".contains(c[i]) {
            who |= match c[i] {
                'u' => 0o4700,
                'g' => 0o2070,
                'o' => 0o1007,
                _ => 0o7777,
            };
            i += 1;
        }
        if who == 0 {
            who = 0o7777;
        }
        if i == c.len() {
            return Err(format!("no + - = in `{clause}`"));
        }
        while i < c.len() {
            let op = c[i];
            if !"+-=".contains(op) {
                return Err(format!("`{}` in `{clause}`: expected + - =", c[i]));
            }
            i += 1;
            let mut bits = 0u32;
            while i < c.len() && !"+-=".contains(c[i]) {
                bits |= match c[i] {
                    'r' => 0o444,
                    'w' => 0o222,
                    'x' => 0o111,
                    'X' if is_dir || current & 0o111 != 0 => 0o111,
                    'X' => 0,
                    's' => 0o6000,
                    't' => 0o1000,
                    ch => return Err(format!("`{ch}` in `{clause}`: expected r w x X s t")),
                };
                i += 1;
            }
            let bits = bits & who;
            match op {
                '+' => mode |= bits,
                '-' => mode &= !bits,
                _ => mode = (mode & !(who & 0o7777)) | bits,
            }
        }
    }
    Ok(mode)
}

/// Change the owner and / or group of `path` (the link itself for a symlink): `user:group`, `user`,
/// `:group`, names or numbers.
pub fn chown(path: &Path, spec: &str) -> Result<(), String> {
    let spec = spec.trim();
    let (u, g) = match spec.split_once(':') {
        Some((u, g)) => (u.trim(), g.trim()),
        None => (spec, ""),
    };
    let uid: libc::uid_t = if u.is_empty() {
        u32::MAX // unchanged
    } else if let Ok(n) = u.parse() {
        n
    } else {
        let c = std::ffi::CString::new(u).map_err(|_| "bad user name".to_string())?;
        // SAFETY: getpwnam returns a pointer into static storage or null; read at once.
        let pw = unsafe { libc::getpwnam(c.as_ptr()) };
        if pw.is_null() {
            return Err(format!("no user `{u}`"));
        }
        unsafe { (*pw).pw_uid }
    };
    let gid: libc::gid_t = if g.is_empty() {
        u32::MAX
    } else if let Ok(n) = g.parse() {
        n
    } else {
        let c = std::ffi::CString::new(g).map_err(|_| "bad group name".to_string())?;
        // SAFETY: as above, for the group database.
        let gr = unsafe { libc::getgrnam(c.as_ptr()) };
        if gr.is_null() {
            return Err(format!("no group `{g}`"));
        }
        unsafe { (*gr).gr_gid }
    };
    use std::os::unix::ffi::OsStrExt;
    let p = std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|_| "bad path".to_string())?;
    // SAFETY: lchown with a valid C string; -1 (u32::MAX) leaves that id unchanged.
    let r = unsafe { libc::lchown(p.as_ptr(), uid, gid) };
    if r == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

/// The ISO 8601 local timestamp the freedesktop trash writes (`2026-11-21T10:42:07`).
pub fn iso_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (y, mo, d, h, mi, s) = local_time(now);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}")
}

/// The people's accounts to choose an owner from: root and the users of uid 1000+ with a login shell
/// (not the service accounts, not `nobody`), from /etc/passwd, by name.
pub fn real_users() -> Vec<String> {
    let text = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
    let mut v: Vec<String> = text
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(':').collect();
            let uid: u32 = f.get(2)?.parse().ok()?;
            let shell = f.get(6).copied().unwrap_or("");
            let login = !(shell.ends_with("nologin") || shell.ends_with("false") || shell.is_empty());
            ((uid == 0 || (1000..60000).contains(&uid)) && login).then(|| f[0].to_string())
        })
        .collect();
    v.sort();
    if let Some(i) = v.iter().position(|u| u == "root") {
        let r = v.remove(i);
        v.push(r);
    }
    v
}

/// The groups to choose from: the ones this user is in first (only those can be given without root),
/// then the people's groups (gid 1000+) and root, from /etc/group.
pub fn real_groups() -> Vec<String> {
    let mut mine: Vec<String> = Vec::new();
    // SAFETY: getgroups fills the buffer we size by its first call.
    unsafe {
        let n = libc::getgroups(0, std::ptr::null_mut());
        if n > 0 {
            let mut g = vec![0 as libc::gid_t; n as usize];
            let n = libc::getgroups(n, g.as_mut_ptr());
            for &gid in g.iter().take(n.max(0) as usize) {
                mine.push(group_name(gid));
            }
        }
    }
    mine.push(group_name(unsafe { libc::getegid() }));
    mine.sort();
    mine.dedup();
    let text = std::fs::read_to_string("/etc/group").unwrap_or_default();
    let mut rest: Vec<String> = text
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(':').collect();
            let gid: u32 = f.get(2)?.parse().ok()?;
            (gid == 0 || (1000..60000).contains(&gid)).then(|| f[0].to_string())
        })
        .filter(|g| !mine.contains(g))
        .collect();
    rest.sort();
    mine.extend(rest);
    mine
}

/// The name of user `uid` (cached), or the number.
pub fn user_name(uid: u32) -> String {
    static CACHE: Mutex<Option<HashMap<u32, String>>> = Mutex::new(None);
    let mut c = CACHE.lock().unwrap();
    let map = c.get_or_insert_with(HashMap::new);
    map.entry(uid)
        .or_insert_with(|| {
            // SAFETY: getpwuid_r writes into the buffers we pass; the result points into them.
            unsafe {
                let mut pw: libc::passwd = std::mem::zeroed();
                let mut buf = vec![0 as libc::c_char; 1024];
                let mut res: *mut libc::passwd = std::ptr::null_mut();
                if libc::getpwuid_r(uid, &mut pw, buf.as_mut_ptr(), buf.len(), &mut res) == 0
                    && !res.is_null()
                {
                    std::ffi::CStr::from_ptr(pw.pw_name).to_string_lossy().into_owned()
                } else {
                    uid.to_string()
                }
            }
        })
        .clone()
}

/// The name of group `gid` (cached), or the number.
pub fn group_name(gid: u32) -> String {
    static CACHE: Mutex<Option<HashMap<u32, String>>> = Mutex::new(None);
    let mut c = CACHE.lock().unwrap();
    let map = c.get_or_insert_with(HashMap::new);
    map.entry(gid)
        .or_insert_with(|| {
            // SAFETY: getgrgid_r writes into the buffers we pass; the result points into them.
            unsafe {
                let mut gr: libc::group = std::mem::zeroed();
                let mut buf = vec![0 as libc::c_char; 4096];
                let mut res: *mut libc::group = std::ptr::null_mut();
                if libc::getgrgid_r(gid, &mut gr, buf.as_mut_ptr(), buf.len(), &mut res) == 0
                    && !res.is_null()
                {
                    std::ffi::CStr::from_ptr(gr.gr_name).to_string_lossy().into_owned()
                } else {
                    gid.to_string()
                }
            }
        })
        .clone()
}

/// The user's home directory.
pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// `path` with the home directory shown as `~`.
pub fn tilde(path: &Path) -> String {
    let h = home();
    match path.strip_prefix(&h) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// The mount point `path` lives on: the highest ancestor on the same device.
pub fn mount_point(path: &Path) -> PathBuf {
    let Ok(dev) = std::fs::symlink_metadata(path).map(|m| m.dev()) else {
        return PathBuf::from("/");
    };
    let mut top = path.to_path_buf();
    let mut cur = path.parent();
    while let Some(p) = cur {
        match std::fs::metadata(p) {
            Ok(m) if m.dev() == dev => {
                top = p.to_path_buf();
                cur = p.parent();
            }
            _ => break,
        }
    }
    top
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        let mut v = vec!["file10", "File2", "file1", "a", "B"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["a", "B", "file1", "File2", "file10"]);
    }

    #[test]
    fn sizes_and_modes() {
        assert_eq!(human_size(512), "512");
        assert_eq!(human_size(1536), "1.5K");
        assert_eq!(human_size(30 * 1024 * 1024), "30M");
        let e = Entry {
            mode: 0o100755,
            ..Entry::parent()
        };
        let f = Entry { kind: Kind::File, ..e };
        assert_eq!(mode_string(&f), "-rwxr-xr-x");
    }

    #[test]
    fn modes_octal_and_symbolic() {
        assert_eq!(parse_mode("755", 0, false), Ok(0o755));
        assert_eq!(parse_mode("u+x", 0o644, false), Ok(0o744));
        assert_eq!(parse_mode("g-w,o-rwx", 0o775, false), Ok(0o750));
        assert_eq!(parse_mode("a=r", 0o777, false), Ok(0o444));
        assert_eq!(parse_mode("go=", 0o755, false), Ok(0o700));
        assert_eq!(parse_mode("+X", 0o644, true), Ok(0o755));
        assert_eq!(parse_mode("+X", 0o644, false), Ok(0o644));
        assert!(parse_mode("u+q", 0o644, false).is_err());
        assert!(parse_mode("99999", 0, false).is_err());
    }

    #[test]
    fn lists_one_level_dirs_first() {
        let d = std::env::temp_dir().join(format!("lvc-fsx-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("zdir/inner")).unwrap();
        std::fs::write(d.join("a.txt"), "x").unwrap();
        std::fs::write(d.join(".hidden"), "x").unwrap();
        let v = list_dir(&d, false, Sort::Name, false).unwrap();
        let names: Vec<&str> = v.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["zdir", "a.txt"]);
        assert_eq!(list_dir(&d, true, Sort::Name, false).unwrap().len(), 3);
        let _ = std::fs::remove_dir_all(&d);
    }
}
