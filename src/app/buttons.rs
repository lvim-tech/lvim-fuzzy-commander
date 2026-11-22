//! The buttons of the bottom bar that plugins add: an action with `group`, `button` (and `sub`) in its `plugin.toml` sits in a group of keys
//! (a built-in one, or a group of the plugin's own, whose letter is none of the built-in ones) and in a sub-group of it.

use super::*;

/// A button of a plugin: its key, its label, and the action it runs (an index into `App::actions`).
pub(super) struct SlotButton {
    pub key: char,
    pub label: String,
    pub action: usize,
}

impl App {
    /// The buttons plugins put in group `g` (in its sub-group `sub`).
    pub(super) fn slot_buttons(&self, g: char, sub: Option<char>) -> Vec<SlotButton> {
        self.actions
            .iter()
            .enumerate()
            .filter_map(|(i, a)| {
                let s = a.slot.as_ref()?;
                (s.group == g && s.sub == sub).then(|| SlotButton { key: s.key, label: a.title.clone(), action: i })
            })
            .collect()
    }

    /// The sub-groups plugins make in group `g`: the key and the label.
    pub(super) fn slot_subs(&self, g: char) -> Vec<(char, String)> {
        let mut out: Vec<(char, String)> = Vec::new();
        for a in &self.actions {
            let Some(s) = &a.slot else { continue };
            if s.group != g {
                continue;
            }
            if let Some(sub) = s.sub
                && !out.iter().any(|(k, _)| *k == sub)
            {
                out.push((sub, s.sub_title.clone().unwrap_or_else(|| sub.to_string())));
            }
        }
        out
    }

    /// The groups plugins make: a letter that is none of the built-in groups (nor `x`), and its title.
    pub(super) fn slot_groups(&self) -> Vec<(char, String)> {
        let mut out: Vec<(char, String)> = Vec::new();
        for a in &self.actions {
            let Some(s) = &a.slot else { continue };
            if GROUPS.iter().any(|(g, _, _)| *g == s.group) || out.iter().any(|(k, _)| *k == s.group) {
                continue;
            }
            out.push((s.group, s.group_title.clone().unwrap_or_else(|| s.group.to_string())));
        }
        out
    }

    /// The action behind the key `c` pressed in group `g` (sub-group `sub`), when a plugin put a button there.
    pub(super) fn slot_action(&self, g: char, sub: Option<char>, c: char) -> Option<usize> {
        self.slot_buttons(g, sub).into_iter().find(|b| b.key == c).map(|b| b.action)
    }
}
