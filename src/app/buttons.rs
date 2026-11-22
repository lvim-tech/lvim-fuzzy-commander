//! The help of the key groups, made from the map of the keys (not written by hand): every group, the app's and the plugins', with where its
//! buttons are — so a button a plugin added, or a user moved, is in the help where it is.

use super::*;

impl App {
    /// One line per group: the group's letter and title, then its sub-groups and buttons (`key label`), as `?` shows them.
    pub(super) fn group_help_lines(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for (g, title) in &self.keymap.groups {
            let mut parts: Vec<String> = Vec::new();
            let mut seen: Vec<&What> = Vec::new();
            for b in self.keymap.items(*g, None) {
                if seen.contains(&&b.what) {
                    continue;
                }
                seen.push(&b.what);
                parts.push(format!("{} {}", b.key, b.label));
            }
            for (s, st) in self.keymap.subs_of(*g) {
                let inner: Vec<String> = self.keymap.items(*g, Some(s)).map(|b| format!("{} {}", b.key, b.label)).collect();
                parts.push(format!("{s} {st} [{}]", inner.join(" · ")));
            }
            out.push((format!("{g} …"), format!("{title}: {}", parts.join(" · "))));
        }
        out
    }
}
