//! One panel: a directory (or the trash), its entries, the query that filters them (the lvim-fuzzy
//! matcher), the cursor, the marked entries and the history.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use lvim_fuzzy_core::{CaseMode, MatchConfig, Pattern, Scheme};

use crate::fsx::{self, Entry, Sort};
use crate::trash;

/// What the panel lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    Dir(PathBuf),
    /// Every trash on the system.
    Trash,
    /// A directory of a remote (the remote plugin: rclone, a mount, an engine of your own): its name and the path in it
    /// (`"/"`, or `""` for the top of an rclone remote).
    Remote { remote: String, path: String },
}

/// The parent of a remote path, or None at the top.
pub fn remote_parent(path: &str) -> Option<String> {
    let t = path.trim_end_matches('/');
    if t.is_empty() {
        return None;
    }
    Some(match t.rfind('/') {
        Some(0) => "/".to_string(),
        Some(i) => t[..i].to_string(),
        None => String::new(),
    })
}

/// `name` inside the remote directory `dir`.
pub fn remote_join(dir: &str, name: &str) -> String {
    format!("{}/{name}", dir.trim_end_matches('/'))
}

/// An archive shown as a directory: the archive, and the directory it was unpacked into (a cache; read-only in spirit:
/// what is changed there is not written back).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArcView {
    pub archive: PathBuf,
    pub root: PathBuf,
    /// Where the archive was entered from when that is not the directory it is in (a remote directory: its copy is in the cache).
    pub origin: Option<Place>,
}

/// Where a plugin's answer is about: the server (`None`: this machine) and the folder on it.
pub type PlaceKey = (Option<String>, PathBuf);

/// What a plugin said of a folder as a whole.
#[derive(Clone, Debug, Default)]
pub struct PluginState {
    pub key: PlaceKey,
    pub flags: Vec<String>,
}

pub struct Panel {
    pub place: Place,
    /// A remote directory is being read (the answer comes later).
    pub loading: bool,
    /// The panel wants its remote directory read: the app asks the plugin and clears this.
    pub remote_want: bool,
    /// The call that reads it, while it is on its way.
    pub remote_tag: Option<u64>,
    /// The entry the cursor was on when the remote directory was asked for again.
    remote_keep: Option<String>,
    /// Set while the panel is inside an archive (`place` is a directory under `archive.root`).
    pub archive: Option<ArcView>,
    /// The listed entries (for the trash: one per item, named by its original path).
    pub entries: Vec<Entry>,
    /// The trashed items behind `entries` (Trash only), index for index.
    pub trash_items: Vec<trash::Item>,
    /// Indices into `entries` that the query keeps, in display order.
    pub shown: Vec<usize>,
    /// The matched char positions of each shown row (for highlighting), parallel to `shown`.
    pub hl: Vec<Vec<usize>>,
    pub query: String,
    pub cursor: usize,
    pub offset: usize,
    /// Marked entries, by name.
    pub marked: BTreeSet<String>,
    pub hidden: bool,
    pub sort: Sort,
    pub reverse: bool,
    back: Vec<Place>,
    forward: Vec<Place>,
    /// The entry the cursor was on in each directory left (so coming back lands on it).
    last_in: std::collections::HashMap<PathBuf, String>,
    pub error: Option<String>,
    /// Something the panel refused to do and the app should say (a remote does not let go of the panel but by disconnecting).
    pub notice: Option<String>,
    /// What plugins said of the folder as a whole: plugin name → (the place, flags, the plugin's note).
    pub plugin_state: std::collections::HashMap<String, PluginState>,
    /// What plugins wrote in the path bar: bar id → (the place, pieces of (text, colour)).
    pub plugin_bars: std::collections::HashMap<String, (PlaceKey, Vec<(String, String)>)>,
    /// What plugins know of each entry: column id → (the place, name → flags).
    pub plugin_flags: std::collections::HashMap<String, (PlaceKey, std::collections::HashMap<String, Vec<String>>)>,
    /// What the plugins' columns answered for this panel's folder: column id → (the folder, name → pieces of (text, colour)).
    pub plugin_cells: std::collections::HashMap<String, (PlaceKey, std::collections::HashMap<String, Vec<(String, String)>>)>,
    /// The folder the plugins' columns were asked for, and whether they are to be asked again (the folder was read again).
    pub cells_dir: Option<PlaceKey>,
    pub cells_want: bool,
}

impl Panel {
    pub fn new(dir: PathBuf, hidden: bool) -> Panel {
        let mut p = Panel {
            place: Place::Dir(dir),
            loading: false,
            remote_want: false,
            remote_tag: None,
            remote_keep: None,
            archive: None,
            entries: Vec::new(),
            trash_items: Vec::new(),
            shown: Vec::new(),
            hl: Vec::new(),
            query: String::new(),
            cursor: 0,
            offset: 0,
            marked: BTreeSet::new(),
            hidden,
            sort: Sort::Name,
            reverse: false,
            back: Vec::new(),
            forward: Vec::new(),
            last_in: Default::default(),
            error: None,
            notice: None,
            plugin_state: Default::default(),
            plugin_bars: Default::default(),
            plugin_flags: Default::default(),
            plugin_cells: Default::default(),
            cells_dir: None,
            cells_want: true,
        };
        p.reload();
        p
    }

    pub fn dir(&self) -> Option<&Path> {
        match &self.place {
            Place::Dir(d) => Some(d),
            Place::Trash | Place::Remote { .. } => None,
        }
    }

    /// The remote and the path in it, when the panel shows a remote directory.
    pub fn remote(&self) -> Option<(&str, &str)> {
        match &self.place {
            Place::Remote { remote, path } => Some((remote, path)),
            _ => None,
        }
    }

    /// The path of an entry in the remote directory (None: not a remote panel, or the `..` row).
    pub fn remote_path_of(&self, e: &Entry) -> Option<String> {
        let (_, dir) = self.remote()?;
        if e.is_parent() { remote_parent(dir) } else { Some(remote_join(dir, &e.name)) }
    }

    /// The directory a remote listing arrived for: the entries (sorted, the hidden ones left out unless shown), the cursor
    /// back where it was.
    pub fn set_remote_entries(&mut self, list: Vec<lvim_fuzzy_plugin::RemoteEntry>) {
        let mut v: Vec<Entry> = list
            .into_iter()
            .filter(|e| self.hidden || !e.name.starts_with('.'))
            .map(|e| Entry {
                kind: match (e.dir, e.link.is_some()) {
                    (true, true) => fsx::Kind::LinkDir,
                    (true, false) => fsx::Kind::Dir,
                    (false, true) => fsx::Kind::Link,
                    (false, false) => fsx::Kind::File,
                },
                mode: e.mode.unwrap_or(if e.dir { 0o755 } else { 0o644 }),
                uid: e.uid.unwrap_or(0),
                gid: e.gid.unwrap_or(0),
                size: e.size,
                mtime: e.mtime,
                target: e.link,
                broken: false,
                name: e.name,
            })
            .collect();
        fsx::sort_entries(&mut v, self.sort, self.reverse);
        self.entries = std::iter::once(Entry::parent()).chain(v).collect();
        self.loading = false;
        self.error = None;
        let names: BTreeSet<&str> = self.entries.iter().map(|e| e.name.as_str()).collect();
        self.marked.retain(|m| names.contains(m.as_str()));
        self.refilter();
        if let Some(k) = self.remote_keep.take() {
            self.select_name(&k);
        }
    }

    /// The remote directory could not be read: the reason is shown in the panel.
    pub fn set_remote_error(&mut self, why: String) {
        self.loading = false;
        self.entries = vec![Entry::parent()];
        self.error = Some(why);
        self.refilter();
    }

    /// Where the plugins' answers for this panel are about: a folder of this machine, or of a server. Nothing else (an archive, the trash) has one.
    pub fn place_key(&self) -> Option<PlaceKey> {
        match (&self.place, self.archive.is_some()) {
            (Place::Dir(d), false) => Some((None, d.clone())),
            (Place::Remote { remote, path }, _) => Some((Some(remote.clone()), PathBuf::from(path))),
            _ => None,
        }
    }

    pub fn is_trash(&self) -> bool {
        self.place == Place::Trash
    }

    /// Read the place again, keeping the cursor on the same entry when it is still there.
    pub fn reload(&mut self) {
        let keep = self.current().map(|e| e.name.clone());
        self.error = None;
        self.cells_want = true;
        match &self.place {
            Place::Dir(d) => {
                match fsx::list_dir(d, self.hidden, self.sort, self.reverse) {
                    Ok(v) => self.entries = v,
                    Err(e) => {
                        self.entries.clear();
                        self.error = Some(format!("{}: {e}", d.display()));
                    }
                }
                if d.parent().is_some() {
                    self.entries.insert(0, Entry::parent());
                }
                self.trash_items.clear();
            }
            Place::Remote { .. } => {
                // read by the plugin: the app asks, the panel shows `loading` until the answer comes
                self.remote_keep = keep.clone();
                self.trash_items.clear();
                self.entries = vec![Entry::parent()];
                self.loading = true;
                self.remote_want = true;
                self.remote_tag = None;
            }
            Place::Trash => {
                self.trash_items = trash::list();
                self.entries = self
                    .trash_items
                    .iter()
                    .map(|it| {
                        let mut e = fsx::entry(&it.path(), String::new()).unwrap_or_else(Entry::parent);
                        e.name = it.original.to_string_lossy().into_owned();
                        e
                    })
                    .collect();
            }
        }
        let names: BTreeSet<&str> = self.entries.iter().map(|e| e.name.as_str()).collect();
        self.marked.retain(|m| names.contains(m.as_str()));
        self.refilter();
        if let Some(k) = keep {
            self.select_name(&k);
        }
    }

    /// Apply the query: fuzzy over the names (the lvim-fuzzy matcher), best first; an empty query keeps
    /// the listing order.
    pub fn refilter(&mut self) {
        self.shown.clear();
        self.hl.clear();
        let q = self.query.trim();
        if q.is_empty() {
            self.shown = (0..self.entries.len()).collect();
            self.hl = vec![Vec::new(); self.shown.len()];
        } else {
            let cfg = MatchConfig {
                case: CaseMode::Smart,
                extended: true,
                exact: false,
                normalize: true,
                scheme: Scheme::Path,
                typos: 0,
                exact_window: lvim_fuzzy_core::EXACT_WINDOW,
            };
            let pat = Pattern::parse(q, &cfg);
            let mut scored: Vec<(i64, usize, Vec<usize>)> = Vec::new();
            for (i, e) in self.entries.iter().enumerate() {
                if e.is_parent() {
                    continue;
                }
                if let Some((score, pos)) = pat.positions(&e.name) {
                    scored.push((score_value(score), i, pos));
                }
            }
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            for (_, i, pos) in scored {
                self.shown.push(i);
                self.hl.push(pos);
            }
        }
        self.cursor = self.cursor.min(self.shown.len().saturating_sub(1));
    }

    pub fn current(&self) -> Option<&Entry> {
        self.shown.get(self.cursor).map(|&i| &self.entries[i])
    }

    pub fn current_trash_item(&self) -> Option<&trash::Item> {
        self.shown.get(self.cursor).and_then(|&i| self.trash_items.get(i))
    }

    /// The full path of an entry of this panel.
    pub fn path_of(&self, e: &Entry) -> PathBuf {
        match &self.place {
            Place::Dir(d) => {
                if e.is_parent() {
                    d.parent().unwrap_or(d).to_path_buf()
                } else {
                    d.join(&e.name)
                }
            }
            // a remote entry has no path on this machine
            Place::Remote { .. } => PathBuf::new(),
            // in the trash the entry is named by its original path; its data is in the trash
            Place::Trash => self
                .entries
                .iter()
                .position(|x| std::ptr::eq(x, e))
                .and_then(|i| self.trash_items.get(i))
                .map(|it| it.path())
                .unwrap_or_default(),
        }
    }

    pub fn current_path(&self) -> Option<PathBuf> {
        self.current().map(|e| self.path_of(e))
    }

    /// What an operation acts on: the marked entries, else the one under the cursor (never `..`).
    pub fn targets(&self) -> Vec<PathBuf> {
        if !self.marked.is_empty() {
            return self
                .entries
                .iter()
                .filter(|e| self.marked.contains(&e.name))
                .map(|e| self.path_of(e))
                .collect();
        }
        match self.current() {
            Some(e) if !e.is_parent() => vec![self.path_of(e)],
            _ => Vec::new(),
        }
    }

    /// The trashed items an operation in the trash view acts on.
    pub fn trash_targets(&self) -> Vec<trash::Item> {
        if !self.marked.is_empty() {
            return self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| self.marked.contains(&e.name))
                .filter_map(|(i, _)| self.trash_items.get(i).cloned())
                .collect();
        }
        self.current_trash_item().cloned().into_iter().collect()
    }

    pub fn select_name(&mut self, name: &str) {
        if let Some(pos) = self.shown.iter().position(|&i| self.entries[i].name == name) {
            self.cursor = pos;
        }
    }

    pub fn move_by(&mut self, d: isize) {
        let n = self.shown.len();
        if n == 0 {
            return;
        }
        self.cursor = (self.cursor as isize + d).clamp(0, n as isize - 1) as usize;
    }

    pub fn toggle_mark(&mut self) {
        if let Some(e) = self.current().cloned()
            && !e.is_parent()
            && !self.marked.remove(&e.name)
        {
            self.marked.insert(e.name);
        }
    }

    /// Mark (or unmark) every shown entry whose name matches a glob (`*.rs`, `a?c`).
    pub fn mark_glob(&mut self, glob: &str, on: bool) -> usize {
        let mut n = 0;
        for &i in &self.shown {
            let e = &self.entries[i];
            if e.is_parent() || !glob_match(glob, &e.name) {
                continue;
            }
            n += 1;
            if on {
                self.marked.insert(e.name.clone());
            } else {
                self.marked.remove(&e.name);
            }
        }
        n
    }

    pub fn invert_marks(&mut self) {
        for &i in &self.shown {
            let e = &self.entries[i];
            if e.is_parent() || e.is_dir() {
                continue;
            }
            if !self.marked.remove(&e.name) {
                self.marked.insert(e.name.clone());
            }
        }
    }

    /// The directory this panel was last in before the trash (the nearest one in its history).
    pub fn last_dir(&self) -> Option<PathBuf> {
        self.back.iter().rev().find_map(|p| if let Place::Dir(d) = p { Some(d.clone()) } else { None })
    }

    /// Is `to` a way out of the remote this panel is on? A remote is left by disconnecting (`rc`), not by walking out of it: only
    /// another remote, or the directories of an archive entered from it, are places it may go to.
    fn leaves_remote(&self, to: &Place) -> bool {
        matches!(self.place, Place::Remote { .. }) && !matches!(to, Place::Remote { .. })
    }

    const REMOTE_STAYS: &'static str = "this is the top of the remote: `r c` disconnects (and brings you back to your folders)";

    /// Go to `place` (history recorded); the query and marks start empty. From a remote only to a remote (see [`Panel::leave_remote`]).
    pub fn go(&mut self, place: Place) {
        if self.leaves_remote(&place) {
            self.notice = Some(Self::REMOTE_STAYS.to_string());
            return;
        }
        self.go_force(place);
    }

    fn go_force(&mut self, place: Place) {
        if place == self.place {
            return;
        }
        let old = std::mem::replace(&mut self.place, place);
        self.remember(&old);
        self.back.push(old);
        self.forward.clear();
        self.leave_archive_if_outside();
        self.arrive();
    }

    /// Disconnected: the panel goes back to the last directory of this machine it was in (home when it had none).
    pub fn leave_remote(&mut self) {
        if !matches!(self.place, Place::Remote { .. }) {
            return;
        }
        let d = self.last_dir().unwrap_or_else(fsx::home);
        self.go_force(Place::Dir(d));
    }

    /// Enter an unpacked archive: the panel shows `root` as the archive's top directory.
    pub fn enter_archive(&mut self, archive: PathBuf, root: PathBuf) {
        let origin = match &self.place {
            Place::Remote { .. } => Some(self.place.clone()),
            _ => None,
        };
        self.archive = Some(ArcView { archive, root: root.clone(), origin });
        self.go_force(Place::Dir(root));
    }

    /// The panel's directory is no longer under the archive's root: it is not shown as an archive any more.
    fn leave_archive_if_outside(&mut self) {
        let outside = match (&self.archive, &self.place) {
            (Some(a), Place::Dir(d)) => !d.starts_with(&a.root),
            (Some(_), Place::Trash | Place::Remote { .. }) => true,
            _ => false,
        };
        if outside {
            self.archive = None;
        }
    }

    fn remember(&mut self, old: &Place) {
        match (old, self.current()) {
            (Place::Dir(d), Some(e)) => {
                self.last_in.insert(d.clone(), e.name.clone());
            }
            (Place::Remote { remote, path }, Some(e)) if !e.is_parent() => {
                self.last_in.insert(PathBuf::from(format!("{remote}:{path}")), e.name.clone());
            }
            _ => {}
        }
    }

    fn arrive(&mut self) {
        self.query.clear();
        self.marked.clear();
        self.cursor = 0;
        self.offset = 0;
        self.reload();
        if let Place::Dir(d) = &self.place
            && let Some(n) = self.last_in.get(d).cloned()
        {
            self.select_name(&n);
        }
        // a remote directory arrives later: the cursor lands then
        if let Place::Remote { remote, path } = &self.place
            && let Some(n) = self.last_in.get(&PathBuf::from(format!("{remote}:{path}"))).cloned()
        {
            self.remote_keep = Some(n);
        }
    }

    /// Enter the directory under the cursor (`..` goes up, landing on the directory left).
    pub fn enter(&mut self) -> bool {
        let Some(e) = self.current().cloned() else {
            return false;
        };
        if !e.is_dir() || self.is_trash() {
            return false;
        }
        if e.is_parent() {
            self.up();
            return true;
        }
        if let Place::Remote { remote, path } = &self.place {
            let target = Place::Remote { remote: remote.clone(), path: remote_join(path, &e.name) };
            self.go(target);
            return true;
        }
        let Some(d) = self.dir() else { return false };
        let target = d.join(&e.name);
        self.go(Place::Dir(target));
        true
    }

    /// Up one directory, the cursor on the one left.
    pub fn up(&mut self) {
        if self.is_trash() {
            self.back_in_history();
            return;
        }
        if let Place::Remote { remote, path } = self.place.clone() {
            match remote_parent(&path) {
                Some(parent) => {
                    self.last_in.insert(PathBuf::from(format!("{remote}:{parent}")), path.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string());
                    self.go(Place::Remote { remote, path: parent });
                }
                // the top of the remote: it does not let go (a disconnect does)
                None => self.notice = Some(Self::REMOTE_STAYS.to_string()),
            }
            return;
        }
        let Some(d) = self.dir().map(Path::to_path_buf) else {
            return;
        };
        if let Some(a) = self.archive.clone()
            && d == a.root
        {
            // the top of the archive: back to where it was entered from (a remote directory), or to the directory it lives in
            if let Some(origin) = a.origin.clone() {
                // what was walked inside the archive leaves no trace in the history: back is where it was entered from
                while matches!(self.back.last(), Some(Place::Dir(d)) if d.starts_with(&a.root)) {
                    self.back.pop();
                }
                if self.back.last() == Some(&origin) {
                    self.back.pop();
                }
                let old = std::mem::replace(&mut self.place, origin);
                self.remember(&old);
                self.forward.clear();
                self.archive = None;
                self.arrive();
                return;
            }
            if let (Some(parent), Some(name)) = (a.archive.parent(), a.archive.file_name()) {
                self.last_in.insert(parent.to_path_buf(), name.to_string_lossy().into_owned());
                self.go(Place::Dir(parent.to_path_buf()));
            }
            return;
        }
        let Some(parent) = d.parent().map(Path::to_path_buf) else {
            return;
        };
        let left = d
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.last_in.insert(parent.clone(), left);
        self.go(Place::Dir(parent));
    }

    pub fn back_in_history(&mut self) {
        if let Some(to) = self.back.last()
            && self.leaves_remote(to)
        {
            self.notice = Some(Self::REMOTE_STAYS.to_string());
            return;
        }
        if let Some(p) = self.back.pop() {
            let old = std::mem::replace(&mut self.place, p);
            self.remember(&old);
            self.forward.push(old);
            self.leave_archive_if_outside();
            self.arrive();
        }
    }

    pub fn forward_in_history(&mut self) {
        if let Some(to) = self.forward.last()
            && self.leaves_remote(to)
        {
            self.notice = Some(Self::REMOTE_STAYS.to_string());
            return;
        }
        if let Some(p) = self.forward.pop() {
            let old = std::mem::replace(&mut self.place, p);
            self.remember(&old);
            self.back.push(old);
            self.leave_archive_if_outside();
            self.arrive();
        }
    }

    /// The total size of the marked files (directories count as nothing until measured).
    pub fn marked_size(&self) -> u64 {
        self.entries
            .iter()
            .filter(|e| self.marked.contains(&e.name) && !e.is_dir())
            .map(|e| e.size)
            .sum()
    }

    /// Keep the cursor in view for a list `h` rows tall.
    pub fn scroll(&mut self, h: usize) {
        if h == 0 {
            return;
        }
        if self.cursor < self.offset {
            self.offset = self.cursor;
        } else if self.cursor >= self.offset + h {
            self.offset = self.cursor + 1 - h;
        }
        self.offset = self.offset.min(self.shown.len().saturating_sub(h));
    }
}

/// A better match first; between equals, the shorter (tighter) match.
fn score_value(s: lvim_fuzzy_core::Score) -> i64 {
    (s.score as i64) * 1024 - (s.end.saturating_sub(s.begin) as i64).min(1023)
}

/// `*` any run, `?` one char; case-sensitive (as the shell).
pub fn glob_match(glob: &str, name: &str) -> bool {
    let g: Vec<char> = glob.chars().collect();
    let n: Vec<char> = name.chars().collect();
    let (mut gi, mut ni) = (0usize, 0usize);
    let (mut star, mut mark) = (None, 0usize);
    while ni < n.len() {
        if gi < g.len() && (g[gi] == '?' || g[gi] == n[ni]) {
            gi += 1;
            ni += 1;
        } else if gi < g.len() && g[gi] == '*' {
            star = Some(gi);
            mark = ni;
            gi += 1;
        } else if let Some(s) = star {
            gi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    while gi < g.len() && g[gi] == '*' {
        gi += 1;
    }
    gi == g.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(glob_match("*.rs", "main.rs"));
        assert!(!glob_match("*.rs", "main.rs.bak"));
        assert!(glob_match("a?c*", "abcdef"));
        assert!(glob_match("*", ""));
    }

    #[test]
    fn walks_in_and_out_keeping_the_cursor() {
        let d = std::env::temp_dir().join(format!("lvc-panel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("alpha/inner")).unwrap();
        std::fs::create_dir_all(d.join("beta")).unwrap();
        std::fs::write(d.join("gamma.txt"), "x").unwrap();
        let mut p = Panel::new(d.clone(), false);
        // `..`, alpha, beta, gamma.txt
        assert_eq!(p.shown.len(), 4);
        p.select_name("beta");
        assert!(p.enter());
        assert_eq!(p.dir().unwrap(), d.join("beta"));
        p.up();
        assert_eq!(p.current().unwrap().name, "beta", "back on the directory left");
        // the query filters (fuzzy) and `..` drops out
        p.query = "gmt".into();
        p.refilter();
        assert_eq!(p.shown.len(), 1);
        assert_eq!(p.current().unwrap().name, "gamma.txt");
        p.query.clear();
        p.refilter();
        assert_eq!(p.mark_glob("*.txt", true), 1);
        assert_eq!(p.targets(), vec![d.join("gamma.txt")]);
        p.back_in_history();
        assert_eq!(p.dir().unwrap(), d.join("beta"));
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[cfg(test)]
mod remote_tests {
    use super::*;
    use lvim_fuzzy_plugin::RemoteEntry;

    fn entry(name: &str, dir: bool, size: u64) -> RemoteEntry {
        RemoteEntry { name: name.into(), dir, size, mtime: 1, mode: None, uid: None, gid: None, link: None }
    }

    #[test]
    fn remote_paths_join_and_go_up() {
        assert_eq!(remote_join("/a/b", "c"), "/a/b/c");
        assert_eq!(remote_join("/", "c"), "/c");
        assert_eq!(remote_join("", "c"), "/c");
        assert_eq!(remote_parent("/a/b"), Some("/a".into()));
        assert_eq!(remote_parent("/a"), Some("/".into()));
        assert_eq!(remote_parent("/"), None);
        assert_eq!(remote_parent(""), None);
        assert_eq!(remote_parent("top/sub"), Some("top".into()));
        assert_eq!(remote_parent("top"), Some(String::new()));
    }

    #[test]
    fn a_remote_listing_fills_the_panel_sorted_without_the_hidden_ones() {
        let mut p = Panel::new(std::env::temp_dir(), false);
        p.go(Place::Remote { remote: "r".into(), path: "/".into() });
        // the directory is asked for, not read: the panel waits
        assert!(p.loading && p.remote_want && p.remote().is_some() && p.dir().is_none());
        assert!(p.current_path().is_some_and(|x| x.as_os_str().is_empty()));
        p.set_remote_entries(vec![entry("b.txt", false, 5), entry(".hid", false, 1), entry("adir", true, 0), entry("a.txt", false, 2)]);
        assert!(!p.loading);
        let names: Vec<&str> = p.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["..", "adir", "a.txt", "b.txt"]);
        // a path in the remote, for what is under the cursor
        p.cursor = 1;
        assert_eq!(p.current().map(|e| p.remote_path_of(e)), Some(Some("/adir".to_string())));
        // enter goes into the remote directory, `..` at the top comes back to where the panel was
        assert!(p.enter());
        assert_eq!(p.remote(), Some(("r", "/adir")));
        p.up();
        assert_eq!(p.remote(), Some(("r", "/")));
        // the top of a remote does not let go: it says so (a disconnect brings the panel back)
        p.up();
        assert!(p.remote().is_some() && p.notice.is_some());
        assert!(p.notice.take().unwrap().contains("r c"));
        p.go(Place::Dir(std::env::temp_dir()));
        assert!(p.remote().is_some() && p.notice.take().is_some());
        p.leave_remote();
        assert!(p.remote().is_none() && p.dir().is_some());
    }

    #[test]
    fn a_failed_listing_leaves_the_reason() {
        let mut p = Panel::new(std::env::temp_dir(), false);
        p.go(Place::Remote { remote: "r".into(), path: "/".into() });
        p.set_remote_error("no route".into());
        assert!(!p.loading);
        assert_eq!(p.error.as_deref(), Some("no route"));
        assert_eq!(p.entries.len(), 1);
    }
}
