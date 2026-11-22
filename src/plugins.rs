//! Extending lvc: ACTIONS (a key and / or an F9 menu entry → a command), PREVIEWERS (a command that shows
//! a file in the quick view), and external PLUGINS that bring both.
//!
//! In the config:
//!
//! ```toml
//! [actions.gitlog]
//! title   = "git log of the file"
//! key     = "leader G"              # optional (the leader, then G); also listed in the F9 menu
//! command = "git log --oneline -- {}"
//! mode    = "wait"                  # see Mode
//! reload  = false                   # read the panels again after it
//!
//! [previewers.markdown]
//! command    = "glow -s dark {}"
//! extensions = ["md", "markdown"]
//! ```
//!
//! A PLUGIN is a directory in `~/.config/lvim-plugins/<name>/` (shared with the other lvim tools; the old
//! `~/.config/lvim-commander/plugins/` is read too; `lvim-plugin install <git-url>` fills it) with a `plugin.toml` holding the
//! same `[actions.*]` / `[previewers.*]` tables (and a `description`). Its commands run with the plugin's
//! directory in `$LVC_PLUGIN_DIR`, so a plugin ships its own scripts in any language.
//!
//! Placeholders in a command (each quoted for the shell): `{}` the entry under the cursor, `{+}` the marked
//! entries (or the one under the cursor), `{name}` its name, `{dir}` the panel's directory, `{other}` the
//! other panel's directory. The same reach the command as `$LVC_FILE`, `$LVC_FILES` (one per line),
//! `$LVC_DIR`, `$LVC_OTHER_DIR`.
//!
//! A button may run an operation of the APP's own (`builtin = "git-stage"`): the plugin says where the button is and what it is called, the
//! app does the work (the git and remote groups are made this way). Where the buttons are can be moved in the config: `[keys] "zoom" = "v z"`
//! (the app's commands) and `[plugins.<name>.keys] lint = "x a l"` (a plugin's); what is taken is an error, and the app does not start.
//!
//! A BUTTON of its own: an action may sit in the bottom bar, in a group of keys (the key groups are `t m a y p f g o x r v`; a letter
//! that is none of them makes a new group of the plugin's):
//!
//! ```toml
//! [actions.lint]
//! title        = "lint"
//! call         = "lint"
//! group        = "x"                # the group the button is in (`group_title` names a group of the plugin's own)
//! sub          = "a"                # optional: the sub-group in it (`sub_title` names one of the plugin's own)
//! button       = "l"                # the key of the button there
//! ```
//!
//! A COLUMN of its own: `[columns.<id>]` with `title` and `call` (a function of the plugin): lvc calls it with the folder and its names
//! (`{"call":…,"column":"plugin.id","dir":…,"names":[…]}`) and the answer is `{"op":"cells","column":"plugin.id","dir":…,"cells":{"name":[["text","colour"],…]}}`;
//! the column is in the columns list (`vc`) with the built-in ones, where it can be moved and turned off.
//!
//! A `control` command talks back: each line it prints is an instruction —
//! `cd PATH`, `other-cd PATH`, `select NAME`, `mark NAME`, `reload`, `message TEXT`, `error TEXT`.
//!
//! A plugin may name a RUNTIME in its `plugin.toml` (`runtime = "command" | "process" | "native" | "wasm" |
//! "lua"`, default `command`). Instead of a `command`, its actions then say `call = "name"`: a function of the
//! plugin. All the runtimes speak the same JSON: lvc sends the context ([`Ctx::to_json`]) and the plugin answers
//! with a list of instructions ([`parse_response`]) — the ones above plus `input`, `confirm`, `menu`, `preview`:
//!
//! ```json
//! [{"op":"cd","path":"/tmp"}, {"op":"message","text":"done"},
//!  {"op":"input","prompt":"Name","value":"x","then":"make"},
//!  {"op":"confirm","text":"Sure?","then":"go"},
//!  {"op":"menu","title":"Pick","items":[{"label":"One","call":"one"},
//!   {"label":"text a search matches","call":"two","cells":[["/dev/sdb1","blue"],["16G","cyan"]]}]},
//!  {"op":"preview","text":"shown in the quick view"},
//!  {"op":"text","title":"diff a b","text":"a text on the whole screen (a diff is coloured)"}]
//! ```

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub use lvim_fuzzy_plugin::Runtime;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// In the terminal; lvc waits for a key afterwards (to read the output).
    Wait,
    /// In the terminal, back to lvc as soon as it ends (an editor, a TUI program).
    Tty,
    /// In the background, no terminal; a toast says how it went.
    Silent,
    /// In the background; its output lines are instructions for lvc (see the module docs).
    Control,
}

#[derive(Clone, Debug)]
pub struct Action {
    pub id: String,
    pub title: String,
    pub key: Option<KeySpec>,
    /// The shell command; empty for a `call` action.
    pub command: String,
    /// The plugin function to call (runtime other than `command`).
    pub call: Option<String>,
    /// The app's own operation this button runs (`git-stage`, `remote-list` …): the plugin says where the button is and what it is called,
    /// the app does the work.
    pub builtin: Option<String>,
    pub mode: Mode,
    pub reload: bool,
    /// Where its button is in the bottom bar, when it has one.
    pub slot: Option<Slot>,
    /// When the button is there (`when = ["repo", "item:unstaged"]`): conditions on what the plugin said of this folder (`state`) and of the entries
    /// it is for (`flags` of the cells): `x` the folder has the flag, `!x` it has not, `item:x` some target has it, `one:x` the only target has it,
    /// `!item:x` no target has it. Empty: always.
    pub when: Vec<String>,
    /// "config", or the plugin it comes from.
    pub source: String,
    /// The plugin's directory ($LVC_PLUGIN_DIR).
    pub dir: Option<PathBuf>,
}

/// Where an action's button is: the group (and sub-group) of keys and the key there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot {
    pub group: char,
    pub sub: Option<char>,
    pub key: char,
    pub group_title: Option<String>,
    pub sub_title: Option<String>,
}

/// A column a plugin brings (`[columns.<id>]`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnDef {
    /// `plugin.id`
    pub id: String,
    pub title: String,
    pub plugin: String,
    /// The plugin function that answers with the cells.
    pub call: String,
    /// `position = "first"`: when the column is new, it goes first of the columns (not last).
    pub first: bool,
    /// `remote = true`: it is asked for a server's folder too (a plugin that can work over ssh).
    pub remote: bool,
    /// `always = true`: it is asked even when its column is off (it also writes the path bar and the folder's state).
    pub always: bool,
}

#[derive(Clone, Debug)]
pub struct Previewer {
    pub name: String,
    pub command: String,
    /// Lower case; empty = every file.
    pub extensions: Vec<String>,
    pub dir: Option<PathBuf>,
}

impl Previewer {
    pub fn applies(&self, name: &str) -> bool {
        if self.extensions.is_empty() {
            return true;
        }
        let lower = name.to_ascii_lowercase();
        self.extensions
            .iter()
            .any(|e| lower.ends_with(&format!(".{e}")) || lower == *e)
    }
}

/// A key as written in the config: `leader g` (the leader key, then g — as the built-in commands; `ctrl-c g`
/// is the old spelling), `f11`;
/// also `alt-g`, `ctrl-x`, `alt-shift-p`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeySpec {
    pub code: KeyCode,
    pub mods: KeyModifiers,
    /// after the leader
    pub chord: bool,
    pub text: String,
}

impl KeySpec {
    pub fn parse(s: &str) -> Option<KeySpec> {
        let t = s.trim();
        let low = t.to_ascii_lowercase();
        // `leader x` is the way to write it; `ctrl-c x` is the old spelling of the same
        for pre in ["leader ", "ctrl-c ", "c-c "] {
            if low.starts_with(pre) {
                let rest = t[pre.len()..].trim();
                let mut ch = rest.chars();
                let c = ch.next()?;
                let code = if ch.next().is_none() {
                    KeyCode::Char(c)
                } else {
                    match KeySpec::parse(rest)? {
                        KeySpec { code, mods, .. } if mods.is_empty() => code,
                        _ => return None,
                    }
                };
                return Some(KeySpec { code, mods: KeyModifiers::NONE, chord: true, text: format!("C-c {rest}") });
            }
        }
        let lower = low;
        let mut mods = KeyModifiers::NONE;
        let mut rest = lower.as_str();
        loop {
            if let Some(r) = rest.strip_prefix("alt-") {
                mods |= KeyModifiers::ALT;
                rest = r;
            } else if let Some(r) = rest.strip_prefix("ctrl-") {
                mods |= KeyModifiers::CONTROL;
                rest = r;
            } else if let Some(r) = rest.strip_prefix("shift-") {
                mods |= KeyModifiers::SHIFT;
                rest = r;
            } else {
                break;
            }
        }
        let named = match rest {
            "tab" => Some(KeyCode::Tab),
            "enter" | "return" => Some(KeyCode::Enter),
            "space" => Some(KeyCode::Char(' ')),
            "backspace" => Some(KeyCode::Backspace),
            "esc" | "escape" => Some(KeyCode::Esc),
            "up" => Some(KeyCode::Up),
            "down" => Some(KeyCode::Down),
            "left" => Some(KeyCode::Left),
            "right" => Some(KeyCode::Right),
            "pageup" => Some(KeyCode::PageUp),
            "pagedown" => Some(KeyCode::PageDown),
            "home" => Some(KeyCode::Home),
            "end" => Some(KeyCode::End),
            "delete" => Some(KeyCode::Delete),
            "insert" => Some(KeyCode::Insert),
            _ => None,
        };
        let code = if let Some(c) = named {
            c
        } else if let Some(n) = rest.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
            KeyCode::F(n)
        } else {
            let mut ch = rest.chars();
            let c = ch.next()?;
            if ch.next().is_some() {
                return None;
            }
            KeyCode::Char(c)
        };
        Some(KeySpec {
            code,
            mods,
            chord: false,
            text: s.trim().to_string(),
        })
    }

    /// `chord`: the key comes after the leader.
    pub fn matches(&self, k: &KeyEvent, chord: bool) -> bool {
        if self.chord != chord {
            return false;
        }
        if self.chord {
            return k.code == self.code;
        }
        let code_eq = match (self.code, k.code) {
            (KeyCode::Char(a), KeyCode::Char(b)) => a.eq_ignore_ascii_case(&b) && (a == b || self.mods.contains(KeyModifiers::SHIFT)),
            (a, b) => a == b,
        };
        // SHIFT on a letter is in the char itself; compare the other modifiers
        let strip = |m: KeyModifiers| m - KeyModifiers::SHIFT;
        code_eq && strip(k.modifiers) == strip(self.mods)
    }
}

fn parse_mode(s: &str) -> Option<Mode> {
    Some(match s {
        "wait" => Mode::Wait,
        "tty" => Mode::Tty,
        "silent" => Mode::Silent,
        "control" => Mode::Control,
        _ => return None,
    })
}

/// `[actions.*]` of a config or a plugin.
pub fn parse_actions(
    v: &toml::Value,
    source: &str,
    dir: Option<&Path>,
    runtime: Runtime,
    at: &str,
    errs: &mut Vec<String>,
) -> Vec<Action> {
    let mut out = Vec::new();
    let Some(t) = v.as_table() else {
        errs.push(format!("{at}: [actions] must be a table of tables"));
        return out;
    };
    for (id, a) in t {
        let Some(a) = a.as_table() else {
            errs.push(format!("{at}: actions.{id} must be a table"));
            continue;
        };
        let s = |k: &str| a.get(k).and_then(|v| v.as_str()).map(str::to_string);
        let builtin = s("builtin");
        if builtin.is_some() && (s("command").is_some() || s("call").is_some()) {
            errs.push(format!("{at}: actions.{id}: `builtin` is the app's own operation, with no `command` or `call`"));
            continue;
        }
        let (command, call) = match (s("command"), s("call")) {
            _ if builtin.is_some() => (String::new(), None),
            (Some(_), Some(_)) => {
                errs.push(format!("{at}: actions.{id}: `command` or `call`, not both"));
                continue;
            }
            (Some(c), None) => (c, None),
            (None, Some(c)) => {
                if runtime == Runtime::Command {
                    errs.push(format!(
                        "{at}: actions.{id}: `call` needs a plugin with a `runtime` (process, native, wasm, lua)"
                    ));
                    continue;
                }
                (String::new(), Some(c))
            }
            (None, None) => {
                errs.push(format!("{at}: actions.{id} needs a `command` (or a `call`, or a `builtin`)"));
                continue;
            }
        };
        let mode = match s("mode") {
            None => Mode::Wait,
            Some(m) => match parse_mode(&m) {
                Some(m) => m,
                None => {
                    errs.push(format!("{at}: actions.{id}.mode: wait, tty, silent or control"));
                    continue;
                }
            },
        };
        let key = match s("key") {
            None => None,
            Some(k) => match KeySpec::parse(&k).filter(|k| !k.chord) {
                Some(k) => Some(k),
                None => {
                    errs.push(format!("{at}: actions.{id}.key: `{k}` (f11, alt-x, ...; the leader is gone)"));
                    None
                }
            },
        };
        for k in a.keys() {
            if !["title", "key", "command", "call", "builtin", "mode", "reload", "group", "group_title", "sub", "sub_title", "button", "when"].contains(&k.as_str()) {
                errs.push(format!(
                    "{at}: actions.{id}: unknown key `{k}` (title, key, command, call, builtin, mode, reload, group, group_title, sub, sub_title, button, when)"
                ));
            }
        }
        let one = |k: &str| s(k).and_then(|v| (v.chars().count() == 1).then(|| v.chars().next().unwrap_or(' ')));
        let slot = match (one("group"), one("button")) {
            (Some(group), Some(key)) => Some(Slot { group, sub: one("sub"), key, group_title: s("group_title"), sub_title: s("sub_title") }),
            (None, None) => None,
            _ => {
                errs.push(format!("{at}: actions.{id}: a button needs `group` and `button`, each one character"));
                None
            }
        };
        out.push(Action {
            id: id.clone(),
            title: s("title").unwrap_or_else(|| id.clone()),
            key,
            command,
            call,
            builtin,
            mode,
            reload: a.get("reload").and_then(|v| v.as_bool()).unwrap_or(false),
            slot,
            when: match a.get("when") {
                Some(toml::Value::String(w)) => vec![w.clone()],
                Some(toml::Value::Array(l)) => l.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
                _ => Vec::new(),
            },
            source: source.to_string(),
            dir: dir.map(Path::to_path_buf),
        });
    }
    out
}

/// `[previewers.*]` of a config or a plugin.
pub fn parse_previewers(v: &toml::Value, dir: Option<&Path>, at: &str, errs: &mut Vec<String>) -> Vec<Previewer> {
    let mut out = Vec::new();
    let Some(t) = v.as_table() else {
        errs.push(format!("{at}: [previewers] must be a table of tables"));
        return out;
    };
    for (name, p) in t {
        let Some(p) = p.as_table() else {
            errs.push(format!("{at}: previewers.{name} must be a table"));
            continue;
        };
        let Some(command) = p.get("command").and_then(|v| v.as_str()) else {
            errs.push(format!("{at}: previewers.{name} needs a `command`"));
            continue;
        };
        let extensions = p
            .get("extensions")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|e| e.as_str())
                    .map(|e| e.trim_start_matches('.').to_ascii_lowercase())
                    .collect()
            })
            .unwrap_or_default();
        out.push(Previewer {
            name: name.clone(),
            command: command.to_string(),
            extensions,
            dir: dir.map(Path::to_path_buf),
        });
    }
    out
}

/// A loaded plugin (for the F9 menu's heading and `lvc --plugins`): the host crate's definition.
pub use lvim_fuzzy_plugin::PluginDef as Plugin;

/// Every plugin under `root` meant for lvc (see `lvim_fuzzy_plugin::manifest`), with its `[actions.*]` and
/// `[previewers.*]`, sorted by name.
pub fn load_plugins(roots: &[PathBuf]) -> (Vec<Plugin>, Vec<Action>, Vec<Previewer>, Vec<String>) {
    let (mans, mut errs) = lvim_fuzzy_plugin::manifest::load_roots(roots, "lvim-fuzzy-commander");
    let (mut plugins, mut actions, mut previewers) = (Vec::new(), Vec::new(), Vec::new());
    for m in mans {
        let at = m.def.dir.join("plugin.toml").display().to_string();
        let (name, dir, runtime) = (m.def.name.clone(), m.def.dir.clone(), m.def.runtime);
        for (k, v) in &m.table {
            match k.as_str() {
                "actions" => actions.extend(parse_actions(v, &name, Some(&dir), runtime, &at, &mut errs)),
                "previewers" => previewers.extend(parse_previewers(v, Some(&dir), &at, &mut errs)),
                // the providers a plugin offers every lvim tool (`[preview.<id>]`, `[open.<id>]`): the plugin host
                // reads them (lvim_fuzzy_plugin::Registry), not lvc's own plugin loader
                "preview" | "open" => {}
                // the columns a plugin brings: read by `load_columns`
                "columns" => {}
                // `places = true`: the plugin serves places (connections shown as folders); read by the places client of the host
                "places" => {}
                other => errs.push(format!(
                    "{at}: unknown key `{other}` (name, description, runtime, hosts, api, version, author, license, repository, exec, events, library, module, allow_write, script, build, requires, config, actions, previewers, columns, places, preview, open)"
                )),
            }
        }
        plugins.push(m.def);
    }
    (plugins, actions, previewers, errs)
}

/// The columns the plugins bring (`[columns.<id>]`): `plugin.id`, a title, and the plugin function that gives the cells.
pub fn load_columns(roots: &[PathBuf]) -> Vec<ColumnDef> {
    let (mans, _) = lvim_fuzzy_plugin::manifest::load_roots(roots, "lvim-fuzzy-commander");
    let mut out = Vec::new();
    for m in mans {
        let Some(t) = m.table.get("columns").and_then(|v| v.as_table()) else { continue };
        for (id, c) in t {
            let Some(c) = c.as_table() else { continue };
            let Some(call) = c.get("call").and_then(|v| v.as_str()) else { continue };
            out.push(ColumnDef {
                id: format!("{}.{id}", m.def.name),
                title: c.get("title").and_then(|v| v.as_str()).unwrap_or(id).to_string(),
                plugin: m.def.name.clone(),
                call: call.to_string(),
                first: c.get("position").and_then(|v| v.as_str()) == Some("first"),
                remote: c.get("remote").and_then(|v| v.as_bool()).unwrap_or(false),
                always: c.get("always").and_then(|v| v.as_bool()).unwrap_or(false),
            });
        }
    }
    out
}

/// The places the config moved plugins' buttons to: `[plugins.<name>.keys] action = "x a a"` of the app's config, and `keys` of the plugin's own
/// shared settings file (`~/.config/lvim-plugins/<name>.toml`). `(plugin.action, place)`; a value that is no place is a problem.
pub fn plugin_moves(own: &toml::Table, defs: &[Plugin], plugins_dir: &Path, errs: &mut Vec<String>) -> Vec<(String, crate::app::Pos)> {
    let mut out = Vec::new();
    let mut take = |name: &str, table: &toml::Value, at: &str, errs: &mut Vec<String>| {
        let Some(t) = table.as_table() else {
            errs.push(format!("{at}: `keys` must be a table: action = \"x a a\""));
            return;
        };
        for (action, v) in t {
            match v.as_str().and_then(crate::app::parse_pos) {
                Some(p) => out.push((format!("{name}.{action}"), p)),
                None => errs.push(format!("{at}: keys.{action}: a place in the groups like \"x a a\" (group, sub-group, key) or \"v z\" (group, key)")),
            }
        }
    };
    for d in defs {
        // the plugin's own shared settings file
        let file = plugins_dir.join(format!("{}.toml", d.name));
        if let Ok(text) = std::fs::read_to_string(&file)
            && let Ok(t) = text.parse::<toml::Table>()
            && let Some(k) = t.get("keys")
        {
            take(&d.name, k, &file.display().to_string(), errs);
        }
        // lvc's own layer over it
        if let Some(k) = own.get(&d.name).and_then(|v| v.as_table()).and_then(|t| t.get("keys")) {
            take(&d.name, k, &format!("[plugins.{}] of the config", d.name), errs);
        }
    }
    out
}

/// `[keys]` of the app config over the actions: the plugins are loaded first, then the config moves their
/// keys (`"plugin.action" = "alt-g"`; a config action by its `id`). A name that is neither an action nor a
/// built-in command (`builtin`) is an error.
pub fn apply_key_overrides(
    actions: &mut [Action],
    keys: &[(String, KeySpec)],
    builtin: impl Fn(&str) -> bool,
    errs: &mut Vec<String>,
) {
    for (name, ks) in keys {
        if builtin(name) {
            continue;
        }
        match actions.iter_mut().find(|a| {
            if a.source == "config" { a.id == *name } else { format!("{}.{}", a.source, a.id) == *name }
        }) {
            Some(a) => a.key = Some(ks.clone()),
            None => errs.push(format!(
                "keys.{name}: no such command or action (a plugin's is `plugin.action`, see lvc --plugins)"
            )),
        }
    }
}

/// What a command is run on.
pub struct Ctx {
    pub file: Option<PathBuf>,
    pub files: Vec<PathBuf>,
    pub dir: PathBuf,
    pub other: Option<PathBuf>,
}

impl Ctx {
    /// The context a plugin function receives (JSON): the file under the cursor, the marked ones, both
    /// directories and the active panel. `call` is the function, `arg` what a previous `input` / `menu`
    /// instruction collected.
    /// An empty `call` (an event, not a call) leaves the field out.
    pub fn to_json(&self, call: &str, arg: Option<&str>, active: usize) -> serde_json::Value {
        let path = |p: &PathBuf| p.to_string_lossy().into_owned();
        let mut j = serde_json::json!({
            "call": call,
            "arg": arg,
            "file": self.file.as_ref().map(path),
            "files": self.files.iter().map(path).collect::<Vec<_>>(),
            "dir": path(&self.dir),
            "other_dir": self.other.as_ref().map(path),
            "panel": if active == 0 { "left" } else { "right" },
            "host": "lvim-fuzzy-commander",
        });
        if call.is_empty() {
            j.as_object_mut().map(|o| o.remove("call"));
        }
        j
    }
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The command with its placeholders filled in (each value quoted for `sh -c`).
pub fn expand(template: &str, c: &Ctx) -> String {
    let file = c.file.as_ref().map(|p| quote(&p.to_string_lossy())).unwrap_or_default();
    let name = c
        .file
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| quote(&n.to_string_lossy()))
        .unwrap_or_default();
    let files = c
        .files
        .iter()
        .map(|p| quote(&p.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ");
    let dir = quote(&c.dir.to_string_lossy());
    let other = c.other.as_ref().map(|p| quote(&p.to_string_lossy())).unwrap_or_default();
    template
        .replace("{+}", &files)
        .replace("{name}", &name)
        .replace("{dir}", &dir)
        .replace("{other}", &other)
        .replace("{}", &file)
}

/// A `sh -c` command for `template` in the panel's directory, with the LVC_* variables.
pub fn command(template: &str, c: &Ctx, plugin_dir: Option<&Path>) -> std::process::Command {
    let mut cmd = std::process::Command::new("sh");
    cmd.arg("-c").arg(expand(template, c)).current_dir(&c.dir);
    cmd.env("LVC_DIR", &c.dir);
    if let Some(f) = &c.file {
        cmd.env("LVC_FILE", f);
    }
    cmd.env(
        "LVC_FILES",
        c.files
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("\n"),
    );
    if let Some(o) = &c.other {
        cmd.env("LVC_OTHER_DIR", o);
    }
    if let Some(d) = plugin_dir {
        cmd.env("LVC_PLUGIN_DIR", d);
    }
    cmd
}

/// One instruction from a `control` command's output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Instr {
    Cd(PathBuf),
    OtherCd(PathBuf),
    Select(String),
    Mark(String),
    Reload,
    Message(String),
    Error(String),
    /// A text field; the answer goes to the plugin function `then`.
    /// `data`: a note the plugin hands itself — it comes back as `ctx.data` in the call `then`.
    Input { prompt: String, value: String, then: String, data: Option<String> },
    /// A yes / no; on yes the plugin function `then` is called.
    Confirm { title: Option<String>, text: String, then: String, data: Option<String> },
    /// A list; the picked item's `call` is called.
    Menu { title: String, items: Vec<MenuEntry> },
    /// Text (ANSI allowed) for the quick view.
    Preview(String),
    /// A text on the whole screen with its title (the viewer's keys; a diff is coloured): a plugin's long answer.
    Text(TextSpec),
    /// A form (a dialog of fields): the plugin asks for several things at once; OK calls `then` with the values.
    Form(FormSpec),
    /// Go to a place a plugin serves (a panel on a connection of a places plugin) in the active panel.
    Go { remote: String, path: String },
    /// The panels on this place go back to where they were on this machine (the connection was closed or deleted).
    Leave { remote: String },
    /// The plugin's settings changed (it wrote them itself): read them again.
    Reconfigure,
    /// Leave the whole-screen text.
    Close,
    /// Ask for the folders' cells / bar / state again (something changed that the tool cannot see).
    Refresh,
    /// A notification: shown for a moment (an error stays longer) and kept in the list of notifications (`oo` → Notifications). `level`: `info`, `warn`
    /// or `error`; `title` says who or what it is about (the plugin's name when empty).
    Notify { level: String, title: String, text: String },
    /// A badge at the right of the title bar, the plugin's own (`text` empty: gone).
    Badge { text: String, colour: String },
    /// The cells of a column a plugin brings, for the names of a folder: `[(text, colour)]` pieces each.
    /// `flags`: per entry, facts the plugin has about it (`unstaged`, `tracked` …) — for the `when` of its buttons.
    Cells { column: String, dir: PathBuf, remote: Option<String>, cells: Vec<(String, Vec<(String, String)>)>, flags: Vec<(String, Vec<String>)> },
    /// A piece of the path bar: (text, colour) segments for the folder `dir`, written by the bar `bar` of a plugin.
    Bar { bar: String, dir: PathBuf, remote: Option<String>, segments: Vec<(String, String)> },
    /// What the plugin says of the folder as a whole (`repo`, `staged`, `busy` …): flags for the `when` of its buttons.
    State { dir: PathBuf, remote: Option<String>, flags: Vec<String> },
}

/// One line of a form. `kind` is `text` (typed, with `suggest` as a drop-down on Enter) or `choice` (one of `options`, ← → or Enter). A line with
/// `when` (`{"type": ["0", "3"]}`) is there only while the field `type` has one of those values; `locked` cannot be changed.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FormField {
    pub id: String,
    pub label: String,
    pub choice: bool,
    pub hint: String,
    pub suggest: Vec<(String, String)>,
    pub suggest_empty: String,
    pub options: Vec<(String, String)>,
    pub when: Vec<(String, Vec<String>)>,
    pub locked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FormSpec {
    pub title: String,
    pub fields: Vec<FormField>,
    pub values: std::collections::BTreeMap<String, String>,
    pub then: String,
    pub data: Option<String>,
    pub note: String,
}

/// A key of a list's item or of a whole-screen text: pressed, it calls the plugin function `call` (with `arg`, or the item's own `arg`); with
/// `confirm` a yes / no with that text comes first. `label` is what the button at the bottom says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemKey {
    pub key: char,
    pub label: String,
    pub call: String,
    pub arg: Option<String>,
    pub confirm: Option<String>,
}

/// A text on the whole screen. `lines` are plain, or — `spans` — each in coloured pieces (the line is their joined text). `diff`: lines are coloured
/// by what a diff / commit text starts them with. `anchors` are lines `]` and `[` move among (`current` is the one lit); `keys` are the buttons
/// of the screen: their call gets `arg` = `{"anchor": n, "line": l}` and the `data` back.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TextSpec {
    pub title: String,
    pub lines: Vec<String>,
    pub spans: Vec<Vec<(String, String)>>,
    pub diff: bool,
    pub anchors: Vec<usize>,
    pub current: usize,
    pub keys: Vec<ItemKey>,
    pub data: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuEntry {
    pub keys: Vec<ItemKey>,
    /// What Enter does, for the button at the bottom (`go` when not said).
    pub enter: Option<String>,
    pub label: String,
    pub call: String,
    pub arg: Option<String>,
    /// The row as columns: (text, theme colour) each; the tool aligns them one under another. Empty: the row is just its `label`.
    pub cells: Vec<(String, String)>,
}

/// The `keys` of an item or a text: `[{"key": "d", "label": "delete", "call": "delete", "arg"?, "confirm"?}]` (a key that is not one character is left out).
fn item_keys(v: Option<&serde_json::Value>) -> Vec<ItemKey> {
    v.and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|k| {
                    let s = |n: &str| k.get(n).and_then(|v| v.as_str()).map(str::to_string);
                    let key = s("key")?;
                    let mut cs = key.chars();
                    let (c, rest) = (cs.next()?, cs.next());
                    rest.is_none().then_some(ItemKey { key: c, label: s("label").unwrap_or_default(), call: s("call")?, arg: s("arg"), confirm: s("confirm") })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn parse_instr(line: &str) -> Option<Instr> {
    let line = line.trim_end();
    let (verb, arg) = line.split_once(' ').unwrap_or((line, ""));
    let arg = arg.trim();
    Some(match verb {
        "cd" if !arg.is_empty() => Instr::Cd(PathBuf::from(arg)),
        "other-cd" if !arg.is_empty() => Instr::OtherCd(PathBuf::from(arg)),
        "select" if !arg.is_empty() => Instr::Select(arg.to_string()),
        "mark" if !arg.is_empty() => Instr::Mark(arg.to_string()),
        "reload" => Instr::Reload,
        "message" => Instr::Message(arg.to_string()),
        "error" => Instr::Error(arg.to_string()),
        _ => return None,
    })
}

/// A plugin's answer: a JSON array of instruction objects (`{"op": "cd", "path": ...}`, see the module
/// docs). An unknown `op` or a missing field is an error naming the item.
pub fn parse_response(text: &str) -> Result<Vec<Instr>, String> {
    let v: serde_json::Value = serde_json::from_str(text.trim()).map_err(|e| format!("bad JSON: {e}"))?;
    let arr = match &v {
        serde_json::Value::Array(a) => a.as_slice(),
        serde_json::Value::Null => &[],
        _ => return Err("the answer must be an array of instructions".into()),
    };
    let mut out = Vec::new();
    for (i, it) in arr.iter().enumerate() {
        let get = |k: &str| it.get(k).and_then(|v| v.as_str());
        let need = |k: &str| get(k).map(str::to_string).ok_or_else(|| format!("instruction {i}: missing `{k}`"));
        let Some(op) = get("op") else {
            return Err(format!("instruction {i}: missing `op`"));
        };
        out.push(match op {
            "cd" => Instr::Cd(PathBuf::from(need("path")?)),
            "other-cd" => Instr::OtherCd(PathBuf::from(need("path")?)),
            "select" => Instr::Select(need("name")?),
            "mark" => Instr::Mark(need("name")?),
            "reload" => Instr::Reload,
            "message" => Instr::Message(need("text")?),
            "error" => Instr::Error(need("text")?),
            "input" => Instr::Input {
                prompt: get("prompt").unwrap_or("").to_string(),
                value: get("value").unwrap_or("").to_string(),
                then: need("then")?,
                data: get("data").map(str::to_string),
            },
            "confirm" => Instr::Confirm { title: get("title").map(str::to_string), text: need("text")?, then: need("then")?, data: get("data").map(str::to_string) },
            "notify" => Instr::Notify { level: get("level").unwrap_or("info").to_string(), title: get("title").unwrap_or("").to_string(), text: need("text")? },
            "go" => Instr::Go { remote: need("remote")?, path: get("path").unwrap_or("").to_string() },
            "leave" => Instr::Leave { remote: need("remote")? },
            "reconfigure" => Instr::Reconfigure,
            "form" => {
                let pairs = |v: Option<&serde_json::Value>| -> Vec<(String, String)> {
                    v.and_then(|v| v.as_array())
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| match x {
                                    serde_json::Value::String(s) => Some((s.clone(), s.clone())),
                                    o => Some((o.get("shown")?.as_str()?.to_string(), o.get("value")?.as_str()?.to_string())),
                                })
                                .collect()
                        })
                        .unwrap_or_default()
                };
                let mut spec = FormSpec { title: get("title").unwrap_or("").to_string(), then: need("then")?, data: get("data").map(str::to_string), note: get("note").unwrap_or("").to_string(), ..Default::default() };
                for f in it.get("fields").and_then(|v| v.as_array()).ok_or_else(|| format!("instruction {i}: missing `fields`"))? {
                    let s = |k: &str| f.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let options = f.get("options").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|o| Some((o.get("label")?.as_str()?.to_string(), o.get("value")?.as_str()?.to_string()))).collect()).unwrap_or_default();
                    spec.fields.push(FormField {
                        id: s("id"),
                        label: s("label"),
                        choice: s("kind") == "choice",
                        hint: s("hint"),
                        suggest: pairs(f.get("suggest")),
                        suggest_empty: s("suggest_empty"),
                        options,
                        when: f.get("when").and_then(|w| w.as_object()).map(|o| o.iter().map(|(k, v)| (k.clone(), v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default())).collect()).unwrap_or_default(),
                        locked: f.get("locked").and_then(|v| v.as_bool()).unwrap_or(false),
                    });
                }
                if let Some(o) = it.get("values").and_then(|v| v.as_object()) {
                    spec.values = o.iter().map(|(k, v)| (k.clone(), v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))).collect();
                }
                Instr::Form(spec)
            }
            "close" => Instr::Close,
            "refresh" => Instr::Refresh,
            "badge" => Instr::Badge { text: get("text").unwrap_or("").to_string(), colour: get("colour").unwrap_or("").to_string() },
            "menu" => {
                let items = it
                    .get("items")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| format!("instruction {i}: missing `items`"))?;
                let mut entries = Vec::new();
                for (j, m) in items.iter().enumerate() {
                    let (Some(label), Some(call)) = (
                        m.get("label").and_then(|v| v.as_str()),
                        m.get("call").and_then(|v| v.as_str()),
                    ) else {
                        return Err(format!("instruction {i}: items[{j}] needs `label` and `call`"));
                    };
                    entries.push(MenuEntry {
                        keys: item_keys(m.get("keys")),
                        enter: m.get("enter").and_then(|v| v.as_str()).map(str::to_string),
                        label: label.to_string(),
                        call: call.to_string(),
                        arg: m.get("arg").and_then(|v| v.as_str()).map(str::to_string),
                        cells: m
                            .get("cells")
                            .and_then(|c| c.as_array())
                            .map(|a| a.iter().filter_map(|p| Some((p.get(0)?.as_str()?.to_string(), p.get(1).and_then(|c| c.as_str()).unwrap_or("").to_string()))).collect())
                            .unwrap_or_default(),
                    });
                }
                Instr::Menu { title: get("title").unwrap_or("").to_string(), items: entries }
            }
            "preview" => Instr::Preview(need("text")?),
            "bar" => Instr::Bar {
                bar: need("bar")?,
                dir: PathBuf::from(need("dir")?),
                remote: it.get("remote").and_then(|r| r.as_str()).map(str::to_string),
                segments: it
                    .get("segments")
                    .and_then(|c| c.as_array())
                    .map(|a| a.iter().filter_map(|p| Some((p.get(0)?.as_str()?.to_string(), p.get(1).and_then(|c| c.as_str()).unwrap_or("").to_string()))).collect())
                    .unwrap_or_default(),
            },
            "state" => Instr::State {
                dir: PathBuf::from(need("dir")?),
                remote: it.get("remote").and_then(|r| r.as_str()).map(str::to_string),
                flags: it.get("flags").and_then(|c| c.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(),
            },
            "text" => {
                let mut t = TextSpec { title: get("title").unwrap_or("").to_string(), diff: it.get("diff").and_then(|v| v.as_bool()).unwrap_or(false), ..Default::default() };
                match it.get("lines").and_then(|v| v.as_array()) {
                    // each line a string, or a list of [text, colour] pieces
                    Some(ls) => {
                        for l in ls {
                            match l {
                                serde_json::Value::String(s) => {
                                    t.lines.push(s.clone());
                                    t.spans.push(Vec::new());
                                }
                                serde_json::Value::Array(a) => {
                                    let segs: Vec<(String, String)> = a.iter().filter_map(|p| Some((p.get(0)?.as_str()?.to_string(), p.get(1).and_then(|c| c.as_str()).unwrap_or("").to_string()))).collect();
                                    t.lines.push(segs.iter().map(|(s, _)| s.as_str()).collect());
                                    t.spans.push(segs);
                                }
                                _ => return Err(format!("instruction {i}: a line of `lines` is a string or a list of pieces")),
                            }
                        }
                        if t.spans.iter().all(Vec::is_empty) {
                            t.spans.clear();
                        }
                    }
                    None => t.lines = need("text")?.lines().map(str::to_string).collect(),
                }
                t.anchors = it.get("anchors").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|n| n.as_u64().map(|n| n as usize)).collect()).unwrap_or_default();
                t.current = it.get("current").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                t.keys = item_keys(it.get("keys"));
                t.data = get("data").map(str::to_string);
                Instr::Text(t)
            }
            "cells" => {
                let cells = it
                    .get("cells")
                    .and_then(|c| c.as_object())
                    .map(|o| {
                        o.iter()
                            .map(|(name, segs)| {
                                let segs = segs
                                    .as_array()
                                    .map(|a| a.iter().filter_map(|p| Some((p.get(0)?.as_str()?.to_string(), p.get(1).and_then(|c| c.as_str()).unwrap_or("").to_string()))).collect())
                                    .unwrap_or_default();
                                (name.clone(), segs)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let flags = it
                    .get("flags")
                    .and_then(|c| c.as_object())
                    .map(|o| o.iter().map(|(n, f)| (n.clone(), f.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default())).collect())
                    .unwrap_or_default();
                Instr::Cells { column: need("column")?, dir: PathBuf::from(need("dir")?), remote: it.get("remote").and_then(|r| r.as_str()).map(str::to_string), cells, flags }
            }
            other => return Err(format!("instruction {i}: unknown op `{other}`")),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_places_a_plugins_buttons_were_moved_to_come_from_the_config_and_the_plugins_own_file() {
        let dir = std::env::temp_dir().join(format!("lvc-moves-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("p.toml"), "[keys]\nlint = \"x a l\"\nbad = \"nope\"\n").unwrap();
        std::fs::create_dir_all(dir.join("p")).unwrap();
        std::fs::write(dir.join("p/plugin.toml"), "description = \"d\"\nhosts = [\"lvim-fuzzy-commander\"]\n").unwrap();
        let (defs, _, _, _) = load_plugins(&[dir.clone()]);
        assert_eq!(defs.len(), 1);
        let own: toml::Table = toml::from_str("[p.keys]\nfmt = \"v f\"\n").unwrap();
        let mut errs = Vec::new();
        let m = plugin_moves(&own, &defs, &dir, &mut errs);
        assert!(m.iter().any(|(n, p)| n == "p.lint" && p.sub == Some('a') && p.key == 'l'));
        assert!(m.iter().any(|(n, p)| n == "p.fmt" && p.group == 'v' && p.key == 'f'));
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].contains("keys.bad"), "{errs:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_action_may_have_a_button_and_a_plugin_may_have_a_column() {
        let dir = std::env::temp_dir().join(format!("lvc-btn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("p")).unwrap();
        std::fs::write(
            dir.join("p/plugin.toml"),
            "description = \"d\"\nruntime = \"process\"\nexec = \"./run\"\nhosts = [\"lvim-fuzzy-commander\"]\n[actions.lint]\ntitle = \"lint\"\ncall = \"lint\"\ngroup = \"x\"\nsub = \"a\"\nbutton = \"l\"\n[actions.own]\ntitle = \"mine\"\ncall = \"own\"\ngroup = \"k\"\ngroup_title = \"kit\"\nbutton = \"o\"\n[actions.bad]\ncall = \"b\"\ngroup = \"x\"\n[columns.size2]\ntitle = \"Size 2\"\ncall = \"cells\"\n",
        )
        .unwrap();
        let (_, actions, _, errs) = load_plugins(&[dir.clone()]);
        let lint = actions.iter().find(|a| a.id == "lint").unwrap();
        assert_eq!(lint.slot, Some(Slot { group: 'x', sub: Some('a'), key: 'l', group_title: None, sub_title: None }));
        let own = actions.iter().find(|a| a.id == "own").unwrap();
        assert_eq!(own.slot.as_ref().and_then(|s| s.group_title.as_deref()), Some("kit"));
        // a button needs both its group and its key
        assert!(errs.iter().any(|e| e.contains("a button needs `group` and `button`")));
        assert!(actions.iter().find(|a| a.id == "bad").unwrap().slot.is_none());
        let cols = load_columns(&[dir.clone()]);
        assert_eq!(cols, vec![ColumnDef { id: "p.size2".into(), title: "Size 2".into(), plugin: "p".into(), call: "cells".into(), first: false, remote: false, always: false }]);
        // the answer of a column function
        let r = parse_response(r#"[{"op":"cells","column":"p.size2","dir":"/d","cells":{"a.txt":[["12","green"],["K","comment"]]}}]"#).unwrap();
        // a text on the whole screen
        let t = parse_response(r#"[{"op":"text","title":"diff a b","text":"+x\n-y"}]"#).unwrap();
        assert!(matches!(&t[0], Instr::Text(t) if t.title == "diff a b" && t.lines.iter().any(|l| l.contains("+x"))));
        assert!(matches!(&r[0], Instr::Cells { column, dir, cells, .. } if column == "p.size2" && dir == std::path::Path::new("/d") && cells[0].0 == "a.txt" && cells[0].1.len() == 2));
        let _ = std::fs::remove_dir_all(&dir);
    }

    use super::*;

    #[test]
    fn keys() {
        let k = KeySpec::parse("alt-g").unwrap();
        assert!(k.matches(&KeyEvent::new(KeyCode::Char('g'), KeyModifiers::ALT), false));
        assert!(!k.matches(&KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE), false));
        let f = KeySpec::parse("f11").unwrap();
        assert!(f.matches(&KeyEvent::new(KeyCode::F(11), KeyModifiers::NONE), false));
        let c = KeySpec::parse("ctrl-c G").unwrap();
        assert!(c.matches(&KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT), true));
        assert!(!c.matches(&KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE), true));
        assert!(!c.matches(&KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT), false));
        assert_eq!(c.text, "C-c G");
        assert!(KeySpec::parse("alt-gg").is_none());
    }

    #[test]
    fn expands_quoted() {
        let c = Ctx {
            file: Some(PathBuf::from("/d/a b.txt")),
            files: vec![PathBuf::from("/d/a b.txt"), PathBuf::from("/d/it's")],
            dir: PathBuf::from("/d"),
            other: Some(PathBuf::from("/o")),
        };
        assert_eq!(expand("cat {}", &c), "cat '/d/a b.txt'");
        assert_eq!(expand("rm {+}", &c), r"rm '/d/a b.txt' '/d/it'\''s'");
        assert_eq!(expand("cp {} {other}/{name}", &c), "cp '/d/a b.txt' '/o'/'a b.txt'");
    }

    #[test]
    fn plugins_load_and_talk_back() {
        let root = std::env::temp_dir().join(format!("lvc-plugins-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("hello")).unwrap();
        std::fs::write(
            root.join("hello/plugin.toml"),
            "description = \"says hello\"\n[actions.hi]\ntitle = \"Hi\"\nkey = \"alt-y\"\ncommand = \"echo message hi from $LVC_PLUGIN_DIR\"\nmode = \"control\"\n[previewers.txt]\ncommand = \"cat {}\"\nextensions = [\".TXT\"]\n",
        )
        .unwrap();
        let (plugins, actions, previewers, errs) = load_plugins(&[root.clone()]);
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(plugins.len(), 1);
        assert_eq!(actions[0].mode, Mode::Control);
        assert!(previewers[0].applies("a.txt"));
        let c = Ctx {
            file: None,
            files: vec![],
            dir: root.clone(),
            other: None,
        };
        let out = command(&actions[0].command, &c, actions[0].dir.as_deref()).output().unwrap();
        let line = String::from_utf8_lossy(&out.stdout).lines().next().unwrap().to_string();
        assert_eq!(
            parse_instr(&line),
            Some(Instr::Message(format!("hi from {}", root.join("hello").display())))
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_providers_of_the_lvim_plugins_are_not_unknown_keys() {
        // lvim-fuzzy-plugin-preview / -open: [preview.*] and [open.*] belong to the plugin host, not to lvc's loader
        let root = std::env::temp_dir().join(format!("lvc-providers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("p")).unwrap();
        std::fs::write(
            root.join("p/plugin.toml"),
            "runtime = \"process\"\nexec = \"true\"\n[config.types.text]\nengine = \"editor\"\n[preview.any]\npriority = 50\n[open.any]\npriority = 50\n",
        )
        .unwrap();
        let (plugins, _, _, errs) = load_plugins(&[root.clone()]);
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(plugins.len(), 1);
        // a real mistake is still said
        std::fs::write(root.join("p/plugin.toml"), "runtime = \"process\"\nexec = \"true\"\n[nonsense]\nx = 1\n").unwrap();
        let (_, _, _, errs) = load_plugins(&[root.clone()]);
        assert!(errs.len() == 1 && errs[0].contains("unknown key `nonsense`"), "{errs:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn call_actions_and_runtime() {
        let root = std::env::temp_dir().join(format!("lvc-runtimes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("rt")).unwrap();
        std::fs::create_dir_all(root.join("bad")).unwrap();
        std::fs::create_dir_all(root.join("cmd")).unwrap();
        std::fs::write(
            root.join("rt/plugin.toml"),
            "runtime = \"process\"\nexec = \"./run\"\nevents = [\"cd\"]\n[actions.x]\ntitle = \"X\"\nkey = \"alt-x\"\ncall = \"do_x\"\n",
        )
        .unwrap();
        std::fs::write(root.join("bad/plugin.toml"), "runtime = \"java\"\n").unwrap();
        std::fs::write(root.join("cmd/plugin.toml"), "[actions.y]\ncall = \"f\"\n[actions.z]\ncommand = \"a\"\ncall = \"b\"\n").unwrap();
        let (plugins, actions, _, errs) = load_plugins(&[root.clone()]);
        let rt = actions.iter().find(|a| a.id == "x").unwrap();
        assert_eq!(rt.call.as_deref(), Some("do_x"));
        assert!(rt.command.is_empty());
        let rtp = plugins.iter().find(|p| p.name == "rt").unwrap();
        assert_eq!(rtp.runtime, Runtime::Process);
        assert_eq!(rtp.exec.as_deref(), Some("./run"));
        assert_eq!(rtp.events, vec!["cd".to_string()]);
        assert_eq!(errs.len(), 3, "{errs:?}");
        assert!(errs.iter().any(|e| e.contains("runtime: command, process")));
        assert!(errs.iter().any(|e| e.contains("needs a plugin with a `runtime`")));
        assert!(errs.iter().any(|e| e.contains("not both")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn context_json() {
        let c = Ctx {
            file: Some(PathBuf::from("/d/a")),
            files: vec![PathBuf::from("/d/a"), PathBuf::from("/d/b")],
            dir: PathBuf::from("/d"),
            other: None,
        };
        let j = c.to_json("f", Some("typed"), 1);
        assert_eq!(j["call"], "f");
        assert_eq!(j["arg"], "typed");
        assert_eq!(j["file"], "/d/a");
        assert_eq!(j["files"][1], "/d/b");
        assert_eq!(j["dir"], "/d");
        assert!(j["other_dir"].is_null());
        assert_eq!(j["panel"], "right");
    }

    #[test]
    fn responses() {
        let r = parse_response(
            r#"[{"op":"cd","path":"/tmp"},{"op":"message","text":"ok"},{"op":"reload"},
                {"op":"input","prompt":"N","then":"mk"},{"op":"confirm","text":"?","then":"go"},
                {"op":"menu","title":"T","items":[{"label":"A","call":"a","arg":"1"}]},
                {"op":"preview","text":"hi"}]"#,
        )
        .unwrap();
        assert_eq!(r.len(), 7);
        assert_eq!(r[0], Instr::Cd(PathBuf::from("/tmp")));
        assert_eq!(r[3], Instr::Input { prompt: "N".into(), value: String::new(), then: "mk".into(), data: None });
        assert_eq!(
            r[5],
            Instr::Menu {
                title: "T".into(),
                items: vec![MenuEntry { keys: Vec::new(), enter: None, label: "A".into(), call: "a".into(), arg: Some("1".into()), cells: Vec::new() }]
            }
        );
        assert_eq!(parse_response("null").unwrap(), vec![]);
        assert!(parse_response("{}").is_err());
        assert!(parse_response(r#"[{"op":"nope"}]"#).unwrap_err().contains("unknown op"));
        assert!(parse_response(r#"[{"op":"cd"}]"#).unwrap_err().contains("missing `path`"));
        assert!(parse_response(r#"[{"op":"menu","items":[{"label":"x"}]}]"#).is_err());
    }

    #[test]
    fn leader_and_named_keys() {
        let a = KeySpec::parse("leader f").unwrap();
        let b = KeySpec::parse("ctrl-c f").unwrap();
        assert_eq!(a, b);
        assert!(a.chord);
        assert_eq!(KeySpec::parse("tab").unwrap().code, KeyCode::Tab);
        assert_eq!(KeySpec::parse("space").unwrap().code, KeyCode::Char(' '));
        assert_eq!(KeySpec::parse("ctrl-pageup").unwrap().mods, KeyModifiers::CONTROL);
        assert_eq!(KeySpec::parse("leader space").unwrap().code, KeyCode::Char(' '));
    }

    #[test]
    fn bar_state_and_flags_are_instructions_and_a_button_has_a_when() {
        let r = parse_response(r#"[
            {"op":"cells","column":"p.m","dir":"/d","remote":"srv","cells":{"a":[["M","yellow"]]},"flags":{"a":["unstaged","file"]}},
            {"op":"bar","bar":"git","dir":"/d","segments":[[" main","magenta"],[" ~2","yellow"]]},
            {"op":"state","dir":"/d","flags":["repo","dirty"]}
        ]"#)
        .unwrap();
        assert!(matches!(&r[0], Instr::Cells { remote: Some(s), flags, .. } if s == "srv" && flags[0].1 == ["unstaged", "file"]));
        assert!(matches!(&r[1], Instr::Bar { bar, segments, remote: None, .. } if bar == "git" && segments.len() == 2 && segments[1].1 == "yellow"));
        assert!(matches!(&r[2], Instr::State { flags, .. } if flags == &["repo", "dirty"]));
    }

    #[test]
    fn config_moves_plugin_keys() {
        let mk = |source: &str, id: &str, key: &str| Action {
            id: id.into(),
            title: id.into(),
            key: KeySpec::parse(key),
            command: "true".into(),
            call: None,
            builtin: None,
            mode: Mode::Silent,
            reload: false,
            slot: None,
            when: Vec::new(),
            source: source.into(),
            dir: None,
        };
        let mut acts = vec![mk("git", "log", "leader G"), mk("config", "mine", "alt-m")];
        let keys = vec![
            ("git.log".to_string(), KeySpec::parse("alt-g").unwrap()),
            ("mine".to_string(), KeySpec::parse("alt-n").unwrap()),
            ("find".to_string(), KeySpec::parse("alt-f").unwrap()),
            ("git.nope".to_string(), KeySpec::parse("alt-z").unwrap()),
        ];
        let mut errs = Vec::new();
        apply_key_overrides(&mut acts, &keys, |n| n == "find", &mut errs);
        assert_eq!(acts[0].key.as_ref().unwrap().text, "alt-g");
        assert_eq!(acts[1].key.as_ref().unwrap().text, "alt-n");
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].contains("git.nope"));
    }
}
