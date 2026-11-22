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
//!  {"op":"menu","title":"Pick","items":[{"label":"One","call":"one"}]},
//!  {"op":"preview","text":"shown in the quick view"}]
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
    pub mode: Mode,
    pub reload: bool,
    /// "config", or the plugin it comes from.
    pub source: String,
    /// The plugin's directory ($LVC_PLUGIN_DIR).
    pub dir: Option<PathBuf>,
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
        let (command, call) = match (s("command"), s("call")) {
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
                errs.push(format!("{at}: actions.{id} needs a `command` (or a `call`)"));
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
            if !["title", "key", "command", "call", "mode", "reload"].contains(&k.as_str()) {
                errs.push(format!(
                    "{at}: actions.{id}: unknown key `{k}` (title, key, command, call, mode, reload)"
                ));
            }
        }
        out.push(Action {
            id: id.clone(),
            title: s("title").unwrap_or_else(|| id.clone()),
            key,
            command,
            call,
            mode,
            reload: a.get("reload").and_then(|v| v.as_bool()).unwrap_or(false),
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
                other => errs.push(format!(
                    "{at}: unknown key `{other}` (name, description, runtime, hosts, api, version, author, license, repository, exec, events, library, module, allow_write, script, build, requires, config, actions, previewers, preview, open)"
                )),
            }
        }
        plugins.push(m.def);
    }
    (plugins, actions, previewers, errs)
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
    Input { prompt: String, value: String, then: String },
    /// A yes / no; on yes the plugin function `then` is called.
    Confirm { text: String, then: String },
    /// A list; the picked item's `call` is called.
    Menu { title: String, items: Vec<MenuEntry> },
    /// Text (ANSI allowed) for the quick view.
    Preview(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuEntry {
    pub label: String,
    pub call: String,
    pub arg: Option<String>,
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
            },
            "confirm" => Instr::Confirm { text: need("text")?, then: need("then")? },
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
                        label: label.to_string(),
                        call: call.to_string(),
                        arg: m.get("arg").and_then(|v| v.as_str()).map(str::to_string),
                    });
                }
                Instr::Menu { title: get("title").unwrap_or("").to_string(), items: entries }
            }
            "preview" => Instr::Preview(need("text")?),
            other => return Err(format!("instruction {i}: unknown op `{other}`")),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
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
        assert_eq!(r[3], Instr::Input { prompt: "N".into(), value: String::new(), then: "mk".into() });
        assert_eq!(
            r[5],
            Instr::Menu {
                title: "T".into(),
                items: vec![MenuEntry { label: "A".into(), call: "a".into(), arg: Some("1".into()) }]
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
    fn config_moves_plugin_keys() {
        let mk = |source: &str, id: &str, key: &str| Action {
            id: id.into(),
            title: id.into(),
            key: KeySpec::parse(key),
            command: "true".into(),
            call: None,
            mode: Mode::Silent,
            reload: false,
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
