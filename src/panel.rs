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
}

pub struct Panel {
    pub place: Place,
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
}

impl Panel {
    pub fn new(dir: PathBuf, hidden: bool) -> Panel {
        let mut p = Panel {
            place: Place::Dir(dir),
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
        };
        p.reload();
        p
    }

    pub fn dir(&self) -> Option<&Path> {
        match &self.place {
            Place::Dir(d) => Some(d),
            Place::Trash => None,
        }
    }

    pub fn is_trash(&self) -> bool {
        self.place == Place::Trash
    }

    /// Read the place again, keeping the cursor on the same entry when it is still there.
    pub fn reload(&mut self) {
        let keep = self.current().map(|e| e.name.clone());
        self.error = None;
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

    /// Go to `place` (history recorded); the query and marks start empty.
    pub fn go(&mut self, place: Place) {
        if place == self.place {
            return;
        }
        let old = std::mem::replace(&mut self.place, place);
        self.remember(&old);
        self.back.push(old);
        self.forward.clear();
        self.arrive();
    }

    fn remember(&mut self, old: &Place) {
        if let (Place::Dir(d), Some(e)) = (old, self.current()) {
            self.last_in.insert(d.clone(), e.name.clone());
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
        let Some(d) = self.dir().map(Path::to_path_buf) else {
            return;
        };
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
        if let Some(p) = self.back.pop() {
            let old = std::mem::replace(&mut self.place, p);
            self.remember(&old);
            self.forward.push(old);
            self.arrive();
        }
    }

    pub fn forward_in_history(&mut self) {
        if let Some(p) = self.forward.pop() {
            let old = std::mem::replace(&mut self.place, p);
            self.remember(&old);
            self.back.push(old);
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
