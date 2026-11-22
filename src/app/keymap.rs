//! The keys of the bottom bar's groups, in one place: where every button is (`t n` next tab, `x a a` git add …), who put it there, and what it
//! does. The app's own commands and the buttons of the plugins are put in the same way, so the rules are the same for all of them:
//!
//! - a button is at (group, sub-group, key); a plugin may add buttons to a group the app made or another plugin made, and the first to
//!   name a group's letter makes it (a sub-group the same);
//! - what is taken is taken: a button on a place that has one is an ERROR, whoever it is — the app does not start on it;
//! - any button can be moved in the config (`[keys] "zoom" = "v z"` for the app's commands, `[plugins.<name>.keys] stage = "x a a"` for a plugin's
//!   — the user's say is not a conflict), and the result is checked as a whole.

use std::collections::HashMap;

use crate::plugins::Action;

/// What a button does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum What {
    /// A command of the app by its name (`config::COMMANDS`, an F-key action, or `top`).
    Command(String),
    /// The plugin's own action: an index into the actions.
    Action(usize),
}

/// A button.
#[derive(Clone, Debug)]
pub struct Btn {
    pub group: char,
    pub sub: Option<char>,
    pub key: char,
    pub label: String,
    pub what: What,
    /// The name a config uses for it: the command (`zoom`), or `plugin.action`.
    pub id: String,
    /// `app`, or the plugin (`config` for an action of the config).
    pub source: String,
}

/// A place in the groups: the group, the sub-group in it, the key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pos {
    pub group: char,
    pub sub: Option<char>,
    pub key: char,
}

/// `"x a a"` (group, sub-group, key) or `"v z"` (group, key): each one character, apart by spaces. Anything else is not a place (it is a key like
/// `alt-g`, `f5`).
pub fn parse_pos(s: &str) -> Option<Pos> {
    let t: Vec<char> = s.split_whitespace().map(|w| if w.chars().count() == 1 { w.chars().next() } else { None }).collect::<Option<Vec<_>>>()?;
    match t.as_slice() {
        [g, k] => Some(Pos { group: *g, sub: None, key: *k }),
        [g, s, k] => Some(Pos { group: *g, sub: Some(*s), key: *k }),
        _ => None,
    }
}

/// The keys of normal mode that a group of a plugin's own (or a button moved into a new group) cannot take: the moves, the preview, undo,
/// visual mode, search, the command line.
const RESERVED: &str = "hjklJKGuRVq./: ";

/// The groups and the buttons.
#[derive(Clone, Debug, Default)]
pub struct Keymap {
    pub btns: Vec<Btn>,
    /// (letter, title), in the order they are shown.
    pub groups: Vec<(char, String)>,
    /// (group, letter, title).
    pub subs: Vec<(char, char, String)>,
}

fn who(b: &Btn) -> String {
    match b.source.as_str() {
        "app" => format!("the app's `{}`", b.id),
        "config" => format!("`[actions.{}]` of the config", b.id),
        s => format!("`{}` of the plugin `{s}`", b.id.strip_prefix(&format!("{s}.")).unwrap_or(&b.id)),
    }
}

impl Keymap {
    /// Build the groups from the app's own table `core` (letter, title, [(key, command)]), the buttons of the `actions`, and the places the
    /// user moved buttons to. `label` gives the label of a core command. Returns the map and every problem found (a map with problems is not
    /// to be used).
    pub fn build(core: &[(char, &str, &[(&str, &str)])], label: &dyn Fn(&str) -> String, actions: &[Action], moves: &[(String, Pos)]) -> (Keymap, Vec<String>) {
        let mut km = Keymap::default();
        let mut errs: Vec<String> = Vec::new();
        for (g, title, keys) in core {
            km.groups.push((*g, (*title).to_string()));
            for (k, name) in *keys {
                if let Some(c) = k.chars().next() {
                    km.btns.push(Btn { group: *g, sub: None, key: c, label: label(name), what: What::Command((*name).to_string()), id: (*name).to_string(), source: "app".into() });
                }
            }
        }
        let core_groups: Vec<char> = km.groups.iter().map(|(g, _)| *g).collect();
        // the buttons the plugins (and the config's actions) ask for
        let mut group_by: HashMap<char, String> = HashMap::new();
        let mut sub_by: HashMap<(char, char), String> = HashMap::new();
        for (i, a) in actions.iter().enumerate() {
            let Some(s) = &a.slot else { continue };
            let id = if a.source == "config" { a.id.clone() } else { format!("{}.{}", a.source, a.id) };
            let what = match &a.builtin {
                Some(n) => What::Command(n.clone()),
                None => What::Action(i),
            };
            let b = Btn { group: s.group, sub: s.sub, key: s.key, label: a.title.clone(), what, id, source: a.source.clone() };
            for (t, sub) in [(&s.group_title, None), (&s.sub_title, s.sub)] {
                let Some(t) = t else { continue };
                // a group the app made has its own title
                if sub.is_none() && core_groups.contains(&s.group) {
                    continue;
                }
                let (slot, what_) = match sub {
                    Some(k) => (sub_by.get(&(s.group, k)).cloned(), format!("the sub-group `{} {k}`", s.group)),
                    None => (group_by.get(&s.group).cloned(), format!("the group `{}`", s.group)),
                };
                match slot {
                    Some(t0) if t0 != *t => errs.push(format!("{} calls {what_} `{t}`, and another plugin calls it `{t0}`", who(&b))),
                    Some(_) => {}
                    None => match sub {
                        Some(k) => {
                            sub_by.insert((s.group, k), t.clone());
                        }
                        None => {
                            group_by.insert(s.group, t.clone());
                        }
                    },
                }
            }
            km.btns.push(b);
        }
        // the places the user moved buttons to
        for (id, pos) in moves {
            match km.btns.iter_mut().find(|b| b.id == *id) {
                Some(b) => {
                    b.group = pos.group;
                    b.sub = pos.sub;
                    b.key = pos.key;
                }
                None => errs.push(format!("keys: `{id}` is no command of the app and no button of a plugin (a plugin's is `plugin.action`)")),
            }
        }
        // the groups and sub-groups the buttons are in (the first to name one makes it)
        for b in &km.btns {
            if !km.groups.iter().any(|(g, _)| *g == b.group) {
                km.groups.push((b.group, group_by.get(&b.group).cloned().unwrap_or_else(|| b.group.to_string())));
            }
            if let Some(s) = b.sub
                && !km.subs.iter().any(|(g, k, _)| *g == b.group && *k == s)
            {
                km.subs.push((b.group, s, sub_by.get(&(b.group, s)).cloned().unwrap_or_else(|| s.to_string())));
            }
        }
        // the checks: nothing stands where something stands
        let mut seen: HashMap<(char, Option<char>, char), usize> = HashMap::new();
        // the numbers of the tab group are the app's
        let tab_numbers = "123456789";
        for (i, b) in km.btns.iter().enumerate() {
            // a group that is not the app's may not sit on a key of normal mode
            if !core_groups.contains(&b.group) && RESERVED.contains(b.group) {
                errs.push(format!("{} is in the group `{}`, but `{}` is a key of normal mode (the moves, the preview, undo, visual mode, search …)", who(b), b.group, b.group));
                continue;
            }
            if b.group == 't' && b.sub.is_none() && tab_numbers.contains(b.key) {
                errs.push(format!("{} is at `t {}`, which is the app's tab number", who(b), b.key));
                continue;
            }
            match seen.get(&(b.group, b.sub, b.key)) {
                Some(&j) if km.btns[j].what != b.what => {
                    let place = match b.sub {
                        Some(s) => format!("{} {s} {}", b.group, b.key),
                        None => format!("{} {}", b.group, b.key),
                    };
                    errs.push(format!("the key `{place}` is wanted by both {} and {}", who(&km.btns[j]), who(b)));
                }
                // the same thing twice on one key is no conflict (an app command with two keys, or two buttons for one command)
                Some(_) => {}
                None => {
                    seen.insert((b.group, b.sub, b.key), i);
                }
            }
        }
        // a sub-group's letter is a name at the group's top: many may share a sub-group, none may sit on the key of a button there
        for (g, sk, _) in &km.subs {
            if let Some(top) = km.btns.iter().find(|b| b.group == *g && b.sub.is_none() && b.key == *sk) {
                errs.push(format!("the sub-group `{g} {sk}` sits on the key of {}", who(top)));
            }
        }
        (km, errs)
    }

    pub fn has_group(&self, g: char) -> bool {
        self.groups.iter().any(|(k, _)| *k == g)
    }

    /// The button at a place.
    pub fn find(&self, g: char, sub: Option<char>, key: char) -> Option<&Btn> {
        self.btns.iter().find(|b| b.group == g && b.sub == sub && b.key == key)
    }

    /// The buttons of a group (or of a sub-group of it), in the order they were put.
    pub fn items(&self, g: char, sub: Option<char>) -> impl Iterator<Item = &Btn> {
        self.btns.iter().filter(move |b| b.group == g && b.sub == sub)
    }

    /// The sub-groups of a group: (letter, title).
    pub fn subs_of(&self, g: char) -> impl Iterator<Item = (char, &str)> {
        self.subs.iter().filter(move |(k, _, _)| *k == g).map(|(_, s, t)| (*s, t.as_str()))
    }

    pub fn has_sub(&self, g: char, s: char) -> bool {
        self.subs.iter().any(|(k, c, _)| *k == g && *c == s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::{Mode, Slot};

    const CORE: &[(char, &str, &[(&str, &str)])] = &[('a', "actions", &[("z", "compress"), ("e", "extract")]), ('v', "view", &[("z", "zoom")])];

    fn label(n: &str) -> String {
        n.to_string()
    }

    fn act(source: &str, id: &str, group: char, sub: Option<char>, key: char, titles: (Option<&str>, Option<&str>)) -> Action {
        Action {
            id: id.into(),
            title: id.into(),
            key: None,
            command: "true".into(),
            call: None,
            builtin: None,
            mode: Mode::Silent,
            reload: false,
            slot: Some(Slot { group, sub, key, group_title: titles.0.map(str::to_string), sub_title: titles.1.map(str::to_string) }),
            when: Vec::new(),
            source: source.into(),
            dir: None,
        }
    }

    fn build(actions: &[Action], moves: &[(String, Pos)]) -> (Keymap, Vec<String>) {
        Keymap::build(CORE, &label, actions, moves)
    }

    const N: (Option<&str>, Option<&str>) = (None, None);

    #[test]
    fn positions_are_parsed() {
        assert_eq!(parse_pos("v z"), Some(Pos { group: 'v', sub: None, key: 'z' }));
        assert_eq!(parse_pos("x a a"), Some(Pos { group: 'x', sub: Some('a'), key: 'a' }));
        // a key, not a place
        assert_eq!(parse_pos("alt-g"), None);
        assert_eq!(parse_pos("ctrl-c L"), None);
        assert_eq!(parse_pos("f5"), None);
        assert_eq!(parse_pos("a"), None);
    }

    #[test]
    fn plugins_add_keys_to_a_group_and_none_takes_a_taken_one() {
        // `ak` is free, and `a` is a group of the app: it is added to
        let (km, e) = build(&[act("p", "k", 'a', None, 'k', N)], &[]);
        assert!(e.is_empty(), "{e:?}");
        assert!(km.find('a', None, 'k').is_some() && km.find('a', None, 'z').is_some());
        // `az` is the app's
        let (_, e) = build(&[act("p", "z", 'a', None, 'z', N)], &[]);
        assert_eq!(e.len(), 1);
        assert!(e[0].contains("`a z`") && e[0].contains("the app's `compress`") && e[0].contains("`z` of the plugin `p`") && e[0].contains("both"), "{e:?}");
        // two plugins on one key
        let (_, e) = build(&[act("p", "one", 'w', None, 'x', (Some("kit"), None)), act("q", "two", 'w', None, 'x', N)], &[]);
        assert_eq!(e.len(), 1, "{e:?}");
        assert!(e[0].contains("`w x`") && e[0].contains("plugin `p`"), "{e:?}");
        // a group of a plugin's own is made by the first and shared with the next
        let (km, e) = build(&[act("p", "one", 'w', None, 'x', (Some("kit"), None)), act("q", "two", 'w', None, 'y', (Some("kit"), None))], &[]);
        assert!(e.is_empty(), "{e:?}");
        assert_eq!(km.groups.iter().find(|(g, _)| *g == 'w').map(|(_, t)| t.as_str()), Some("kit"));
        // two titles for one group
        let (_, e) = build(&[act("p", "one", 'w', None, 'x', (Some("kit"), None)), act("q", "two", 'w', None, 'y', (Some("tools"), None))], &[]);
        assert!(e.iter().any(|m| m.contains("the group `w`")), "{e:?}");
        // a key of normal mode is not a group of a plugin's
        let (_, e) = build(&[act("p", "one", 'j', None, 'x', (Some("x"), None))], &[]);
        assert!(e[0].contains("key of normal mode"), "{e:?}");
        // a sub-group is shared; it may not sit on a button's key
        let (km, e) = build(&[act("p", "d", 'v', Some('d'), 'q', N), act("q", "e", 'v', Some('d'), 'r', N)], &[]);
        assert!(e.is_empty(), "{e:?}");
        assert!(km.has_sub('v', 'd'));
        let (_, e) = build(&[act("p", "d", 'v', Some('z'), 'q', N)], &[]);
        assert!(!e.is_empty(), "{e:?}");
        // the app's tab numbers
        let (_, e) = build(&[act("p", "one", 't', None, '3', N)], &[]);
        assert!(!e.is_empty(), "{e:?}");
    }

    #[test]
    fn the_config_moves_buttons_and_the_result_is_checked() {
        // the app's `zoom` moves to `v w`
        let (km, e) = build(&[], &[("zoom".into(), Pos { group: 'v', sub: None, key: 'w' })]);
        assert!(e.is_empty(), "{e:?}");
        assert!(km.find('v', None, 'w').is_some() && km.find('v', None, 'z').is_none());
        // a plugin's button moves out of the way of the app's
        let (km, e) = build(&[act("p", "z", 'a', None, 'z', N)], &[("p.z".into(), Pos { group: 'a', sub: None, key: 'k' })]);
        assert!(e.is_empty(), "{e:?}");
        assert_eq!(km.find('a', None, 'k').map(|b| b.id.as_str()), Some("p.z"));
        // moved onto a taken key: an error again
        let (_, e) = build(&[act("p", "k", 'a', None, 'k', N)], &[("p.k".into(), Pos { group: 'a', sub: None, key: 'e' })]);
        assert!(e.iter().any(|m| m.contains("`a e`")), "{e:?}");
        // a name that is nothing
        let (_, e) = build(&[], &[("nothing".into(), Pos { group: 'a', sub: None, key: 'k' })]);
        assert!(e[0].contains("`nothing`"), "{e:?}");
        // a button moved to a new group: the group is made
        let (km, e) = build(&[], &[("zoom".into(), Pos { group: 'w', sub: None, key: 'z' })]);
        assert!(e.is_empty() && km.has_group('w'), "{e:?}");
    }

    #[test]
    fn the_same_command_on_two_keys_is_not_a_conflict() {
        let mut a = act("p", "one", 'a', None, 'q', N);
        a.slot.as_mut().unwrap().key = 'q';
        let mut b = act("p", "two", 'a', None, 'w', N);
        a.builtin = Some("top".into());
        b.builtin = Some("top".into());
        let (km, e) = build(&[a, b], &[]);
        assert!(e.is_empty(), "{e:?}");
        assert!(matches!(km.find('a', None, 'q').map(|b| &b.what), Some(What::Command(n)) if n == "top"));
    }
}
