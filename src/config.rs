//! `~/.config/lvim-commander/config.toml` — every key optional:
//!
//! ```toml
//! theme   = "LvimEverforest_dark"  # an lvim theme (~/.config/lvim-commander/themes, then lvim-fuzzy's);
//!                                  # default: the theme lvim-fuzzy uses
//! delete  = "trash"                # F8: "trash" (the system trash) | "direct" (for good)
//! hidden  = false                  # show dotfiles at start (Alt-h toggles)
//! editor  = "nvim"                 # F6 / Enter on a text file; default $VISUAL, $EDITOR, nvim
//! preview = "nvim"                 # the quick view (F2): "nvim" (a real Neovim) | "file" | "off"
//! nvim    = "nvim"                 # the Neovim of the preview
//! pool    = true                   # take the preview's Neovim from `lvim-fuzzy-cli pool` when it runs
//! icons   = "auto"                 # Material icons as images where the terminal takes them (kitty,
//!                                  # ghostty, zellij with placeholders); "font": Nerd glyphs; "image"
//! zellij_images = true             # this zellij accepts kitty placeholders (a build with them)
//!
//! [bookmarks]                      # Alt-b: a name → a directory (with the recent ones after them)
//! home = "~"
//! code = "~/development"
//!
//! [plugins.lvim-fuzzy-plugin-open.types.pictures]   # which program Enter opens a file with is the open plugin's
//! mime   = ["image/*"]                              # configuration (see its README); this is lvc's own layer
//! engine = "imv"
//! [plugins.lvim-fuzzy-plugin-open.engines.imv]
//! kind    = "command"
//! command = "imv {}"                                # {} = the file
//! ```

use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delete {
    Trash,
    Direct,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewKind {
    Nvim,
    File,
    Off,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub theme: Option<String>,
    pub delete: Delete,
    pub hidden: bool,
    /// the bar of the Ctrl-c commands after Ctrl-c (Ctrl-c H toggles it)
    pub key_hints: bool,
    /// follow changes other programs make in the shown directories
    pub watch: bool,
    pub editor: String,
    pub preview: PreviewKind,
    pub nvim: String,
    pub pool: bool,
    /// Alt-b: a name → a directory (`~` allowed).
    pub bookmarks: HashMap<String, String>,
    /// File icons: Material images where the terminal takes them (auto), always glyphs (font).
    pub icons: lvim_fuzzy_ui::iconimg::Mode,
    /// Inside zellij: its build accepts kitty placeholders (as lvim-image's `zellij_placeholders`).
    pub zellij_images: bool,
    /// how a preview's picture reaches the terminal: "file" (it reads a temporary file: fast) or "direct"
    /// (the bytes through the terminal: for a terminal / multiplexer that does not take the file way)
    pub image_transfer_file: bool,
    /// `[actions.*]` (see plugins.rs).
    pub actions: Vec<crate::plugins::Action>,
    /// `[keys]`: the F-key actions (help view copy rename move edit new trash delete menu quit), every
    /// other command (see `COMMANDS`) and the actions of the config and of the plugins (`id`, or
    /// `plugin.id`) on other keys, e.g. `find = "alt-f"`, `up = "ctrl-p"`, `"git.log" = "alt-g"`.
    pub keys: Vec<(String, crate::plugins::KeySpec)>,
    /// `[previewers.*]`.
    pub previewers: Vec<crate::plugins::Previewer>,
    /// `[plugins.<name>]`: lvc's own overrides of the plugins' settings (the layer over the plugin's defaults and
    /// the shared `~/.config/lvim-plugins/<name>.toml`).
    pub plugins: toml::Table,
}

impl Default for Config {
    fn default() -> Config {
        let editor = std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "nvim".into());
        Config {
            theme: None,
            delete: Delete::Trash,
            hidden: false,
            key_hints: true,
            watch: true,
            editor,
            preview: PreviewKind::Nvim,
            nvim: "nvim".into(),
            pool: true,
            bookmarks: HashMap::new(),
            actions: Vec::new(),
            previewers: Vec::new(),
            plugins: toml::Table::new(),
            icons: lvim_fuzzy_ui::iconimg::Mode::Auto,
            zellij_images: true,
            image_transfer_file: true,
            keys: Vec::new(),
        }
    }
}

fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::fsx::home().join(".config"))
}

/// `~/.config/lvim-plugins`: the plugins directory every lvim tool shares (lvc, lfz, the picker), one directory
/// per plugin, each with a `plugin.toml`. `lvim-plugin install <git-url>` puts plugins here.
pub fn plugins_dir() -> PathBuf {
    lvim_fuzzy_plugin::default_root()
}

/// `~/.config/lvim-commander/plugins`: where lvc's plugins lived before the shared directory; still read (after
/// the shared one), so nothing has to move.
pub fn legacy_plugins_dir() -> PathBuf {
    config_home().join("lvim-commander/plugins")
}

/// Both plugin directories, the shared one first.
pub fn plugin_roots() -> Vec<PathBuf> {
    vec![plugins_dir(), legacy_plugins_dir()]
}

/// `~/.config/lvim-commander/themes`: the themes lvim-colorscheme generates for lvc.
pub fn themes_dir() -> PathBuf {
    config_home().join("lvim-commander/themes")
}

pub fn path() -> PathBuf {
    std::env::var_os("LVIM_COMMANDER_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| config_home().join("lvim-commander/config.toml"))
}

/// lvim-fuzzy's theme (its config's `theme`), so both programs wear the same one by default.
fn lvim_fuzzy_theme() -> Option<String> {
    if let Ok(t) = std::env::var("LVIM_FUZZY_THEME")
        && !t.is_empty()
    {
        return Some(t);
    }
    let text = std::fs::read_to_string(config_home().join("lvim-fuzzy/config.toml")).ok()?;
    let v: toml::Table = toml::from_str(&text).ok()?;
    v.get("theme")?.as_str().map(str::to_string)
}

/// Read the config; problems are returned to be shown (the defaults apply to what is wrong).
pub fn load() -> (Config, Vec<String>) {
    load_from(&path())
}

/// `load` from a given file.
pub fn load_from(p: &std::path::Path) -> (Config, Vec<String>) {
    let mut c = Config::default();
    let mut errs = Vec::new();
    let p = p.to_path_buf();
    if let Ok(text) = std::fs::read_to_string(&p) {
        match toml::from_str::<toml::Table>(&text) {
            Err(e) => errs.push(format!("{}: {e}", p.display())),
            Ok(t) => {
                for (k, val) in t {
                    let bad = |errs: &mut Vec<String>, what: &str| {
                        errs.push(format!("{}: `{k}` must be {what}", p.display()))
                    };
                    match k.as_str() {
                        "theme" => match val.as_str() {
                            Some(s) => c.theme = Some(s.into()),
                            None => bad(&mut errs, "a string"),
                        },
                        "delete" => match val.as_str() {
                            Some("trash") => c.delete = Delete::Trash,
                            Some("direct") => c.delete = Delete::Direct,
                            _ => bad(&mut errs, "\"trash\" or \"direct\""),
                        },
                        "watch" => match val.as_bool() {
                            Some(b) => c.watch = b,
                            None => bad(&mut errs, "true or false"),
                        },
                        "key_hints" => match val.as_bool() {
                            Some(b) => c.key_hints = b,
                            None => bad(&mut errs, "true or false"),
                        },
                        "hidden" => match val.as_bool() {
                            Some(b) => c.hidden = b,
                            None => bad(&mut errs, "true or false"),
                        },
                        "editor" => match val.as_str() {
                            Some(s) => c.editor = s.into(),
                            None => bad(&mut errs, "a string"),
                        },
                        "preview" => match val.as_str() {
                            Some("nvim") => c.preview = PreviewKind::Nvim,
                            Some("file") => c.preview = PreviewKind::File,
                            Some("off") => c.preview = PreviewKind::Off,
                            _ => bad(&mut errs, "\"nvim\", \"file\" or \"off\""),
                        },
                        "nvim" => match val.as_str() {
                            Some(s) => c.nvim = s.into(),
                            None => bad(&mut errs, "a string"),
                        },
                        "pool" => match val.as_bool() {
                            Some(b) => c.pool = b,
                            None => bad(&mut errs, "true or false"),
                        },
                        "open" => errs.push(format!(
                            "{}: [open] is gone: which program opens a file is the open plugin's configuration now — \
                             [plugins.lvim-fuzzy-plugin-open.types.<group>] (ext / mime / kind, engine) and \
                             [plugins.lvim-fuzzy-plugin-open.engines.<name>] (kind = \"command\", command = \"imv {{}}\")",
                            p.display()
                        )),
                        "bookmarks" => match val.as_table() {
                            Some(m) => {
                                for (name, dir) in m {
                                    match dir.as_str() {
                                        Some(s) => {
                                            c.bookmarks.insert(name.clone(), s.into());
                                        }
                                        None => errs.push(format!(
                                            "{}: bookmarks.{name} must be a directory string",
                                            p.display()
                                        )),
                                    }
                                }
                            }
                            None => bad(&mut errs, "a table"),
                        },
                        "icons" => match val.as_str().and_then(lvim_fuzzy_ui::iconimg::Mode::parse) {
                            Some(m) => c.icons = m,
                            None => bad(&mut errs, "\"auto\", \"image\" or \"font\""),
                        },
                        "image_transfer" => match val.as_str() {
                            Some("file") => c.image_transfer_file = true,
                            Some("direct") => c.image_transfer_file = false,
                            _ => bad(&mut errs, "\"file\" or \"direct\""),
                        },
                        "zellij_images" => match val.as_bool() {
                            Some(b) => c.zellij_images = b,
                            None => bad(&mut errs, "true or false"),
                        },
                        "leader" => errs.push(format!(
                            "{}: `leader` is gone: commands are key groups now (t tabs, m marks, a actions, y copy, f find, g go, v view); F1 or g? lists them",
                            p.display()
                        )),
                        "keys" => match val.as_table() {
                            Some(m) => {
                                for (name, key) in m {
                                    // (a name that is no command is a plugin / config action: checked once the plugins are loaded)
                                    match key.as_str().and_then(crate::plugins::KeySpec::parse).filter(|k| !k.chord) {
                                        Some(ks) => c.keys.push((name.clone(), ks)),
                                        None => errs.push(format!(
                                            "{}: keys.{name}: a key like \"f5\", \"shift-f8\", \"alt-x\" (the leader is gone: commands sit in key groups)",
                                            p.display()
                                        )),
                                    }
                                }
                            }
                            None => bad(&mut errs, "a table: action = \"key\""),
                        },
                        "actions" => {
                            let at = p.display().to_string();
                            c.actions = crate::plugins::parse_actions(&val, "config", None, crate::plugins::Runtime::Command, &at, &mut errs);
                        }
                        "previewers" => {
                            let at = p.display().to_string();
                            c.previewers = crate::plugins::parse_previewers(&val, None, &at, &mut errs);
                        }
                        "plugins" => match val.as_table() {
                            Some(t) if t.values().all(|x| x.is_table()) => c.plugins = t.clone(),
                            _ => bad(&mut errs, "a table of tables: [plugins.<plugin name>]"),
                        },
                        other => errs.push(format!(
                            "{}: unknown key `{other}` (theme, keys, delete, hidden, editor, preview, nvim, pool, icons, zellij_images, bookmarks, actions, previewers, plugins)",
                            p.display()
                        )),
                    }
                }
            }
        }
    }
    if c.theme.is_none() {
        c.theme = lvim_fuzzy_theme();
    }
    (c, errs)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A whole document, every key (toml 0.9 parses a `Value` as ONE value, not a file: the config was
    /// once read that way and every file failed).
    #[test]
    fn reads_a_whole_config() {
        let p = std::env::temp_dir().join(format!("lvc-cfg-{}.toml", std::process::id()));
        std::fs::write(
            &p,
            "theme = \"T\"\ndelete = \"direct\"\nhidden = true\neditor = \"vi\"\npreview = \"file\"\n\
             pool = false\n[bookmarks]\nh = \"~\"\n",
        )
        .unwrap();
        let (c, errs) = load_from(&p);
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(c.theme.as_deref(), Some("T"));
        assert_eq!(c.delete, Delete::Direct);
        assert!(c.hidden && !c.pool);
        assert_eq!(c.preview, PreviewKind::File);
        assert_eq!(c.bookmarks.get("h").map(String::as_str), Some("~"));
        std::fs::write(&p, "nope = 1\ndelete = \"maybe\"\n").unwrap();
        let (_, errs) = load_from(&p);
        assert_eq!(errs.len(), 2, "{errs:?}");
        // the old [open] table says where its job went
        std::fs::write(&p, "[open]\npng = \"imv {}\"\n").unwrap();
        let (_, errs) = load_from(&p);
        assert!(errs.len() == 1 && errs[0].contains("lvim-fuzzy-plugin-open"), "{errs:?}");
        let _ = std::fs::remove_file(&p);
    }
}

/// Every other command and its default key; `[keys]` moves any of them (`find = "alt-f"`, `up = "ctrl-p"`).
/// `leader x` = the leader key, then x. Two commands may share a key (`restore` / `reload`: one is for the
/// trash, the other for a directory); a default stops working only when every command on it has moved.
pub const COMMANDS: &[(&str, &str)] = &[
    ("find", "leader f"),
    ("grep", "leader g"),
    ("goto", "leader c"),
    ("bookmarks", "leader b"),
    ("shell", "leader x"),
    ("trash-view", "leader t"),
    ("restore", "leader r"),
    ("empty-trash", "leader e"),
    ("chmod", "leader m"),
    ("chown", "leader O"),
    ("compress", "leader z"),
    ("extract", "leader e"),
    ("undo", "leader u"),
    ("mark-all", "leader a"),
    ("invert-marks", "leader *"),
    ("mark-glob", "leader +"),
    ("unmark-glob", "leader -"),
    ("hidden", "leader h"),
    ("long", "leader l"),
    ("columns", "leader v"),
    ("new-tab", "leader n"),
    ("tab-next", "leader )"),
    ("tab-prev", "leader ("),
    ("rename-tab", "leader ,"),
    ("tab-left", "leader <"),
    ("tab-right", "leader >"),
    ("instances", "leader I"),
    ("close-tab", "leader w"),
    ("copy-path", "leader y"),
    ("copy-name", "leader Y"),
    ("copy-dir", "leader D"),
    ("symlink", "leader L"),
    ("sort", "leader s"),
    ("reverse", "leader S"),
    ("swap", "leader p"),
    ("dir-to-other", "leader o"),
    ("here-to-other", "leader i"),
    ("back", "leader ["),
    ("forward", "leader ]"),
    ("reload", "leader r"),
    ("keys-help", "leader ?"),
    ("hints", "leader H"),
    ("info", "leader ;"),
    ("local", "leader G"),
    ("redo", "leader U"),
    ("zoom", "leader Z"),
    ("viewer", "leader V"),
    ("refresh", "leader R"),
    ("sync", "leader /"),
    ("remote-list", "leader '"),
    ("remote-add", "leader }"),
    ("remote-edit", "leader \""),
    ("remote-delete", "leader :"),
    ("subshell", "leader {"),
    ("tasks", "leader !"),
    ("compare", "leader ="),
    ("yank", "leader @"),
    ("cut", "leader #"),
    ("paste", "leader $"),
    ("paste-other", "leader %"),
    ("paste-link", "leader ^"),
    ("paste-hard", "leader &"),
    ("paste-rename", "leader _"),
    ("paste-copy", "leader ~"),
    ("paste-clipboard", "leader `"),
    ("buffer-view", "leader |"),
    ("buffer-drop", "leader \\"),
    ("exit", "leader q"),
    ("up", "k"),
    ("down", "j"),
    ("parent", "h"),
    ("enter", "l"),
    ("mark", "space"),
    ("search", "/"),
    ("page-down", "ctrl-d"),
    ("page-up", "ctrl-u"),
    ("other-panel", "tab"),
];

/// Is `name` an F-key action or a command of `COMMANDS`?
pub fn is_command(name: &str) -> bool {
    FKEY_ACTIONS.iter().any(|(a, _)| *a == name) || COMMANDS.iter().any(|(a, _)| *a == name)
}

/// The actions of the F-keys and their default keys (`[keys]` moves them; the bar shows where they are).
pub const FKEY_ACTIONS: [(&str, &str); 11] = [
    ("help", "f1"),
    ("view", "f2"),
    ("copy", "f3"),
    ("rename", "f4"),
    ("move", "f5"),
    ("edit", "f6"),
    ("new", "f7"),
    ("trash", "f8"),
    ("delete", "shift-f8"),
    ("menu", "f9"),
    ("quit", "f10"),
];

/// What lvc remembers between runs (not settings: the state you left it in), in
/// `$XDG_STATE_HOME/lvim-commander/state.toml`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    /// the quick view (F2) was open
    pub quick: bool,
    /// the columns the panels show beside the name (Ctrl-c v)
    pub cols: Cols,
    /// the tabs as they were left, in their order, and which one was current
    pub tabs: Vec<TabSave>,
    pub tab: usize,
}

/// One panel of a saved tab: its directory and the entry the cursor was on.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct PanelSave {
    /// the directory; a panel in the trash keeps the one it came from (what `back` returns to)
    pub dir: PathBuf,
    pub cursor: String,
    /// the panel was showing the trash
    pub trash: bool,
}

/// One tab as it was left: its own name (None: the directory's), both panels, the active one, the quick view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabSave {
    pub name: Option<String>,
    pub panels: [PanelSave; 2],
    pub active: usize,
    pub quick: bool,
}

/// The columns beside a name, right of it: each on or off (Ctrl-c v).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cols {
    pub kind: bool,
    pub mode: bool,
    pub owner: bool,
    pub group: bool,
    pub size: bool,
    pub date: bool,
}

impl Default for Cols {
    fn default() -> Cols {
        Cols { kind: false, mode: false, owner: false, group: false, size: true, date: true }
    }
}

impl Cols {
    /// (key, label) in the order they are drawn, left to right.
    pub const ALL: [(&'static str, &'static str); 6] = [
        ("kind", "type (DIR · EXE · LNK · BRK · FILE)"),
        ("mode", "permissions (rwxr-xr-x)"),
        ("owner", "owner"),
        ("group", "group"),
        ("size", "size (a directory: its entries, (23))"),
        ("date", "date modified (deleted, in the trash)"),
    ];

    pub fn get(&self, key: &str) -> bool {
        match key {
            "kind" => self.kind,
            "mode" => self.mode,
            "owner" => self.owner,
            "group" => self.group,
            "size" => self.size,
            "date" => self.date,
            _ => false,
        }
    }

    pub fn set(&mut self, key: &str, on: bool) {
        match key {
            "kind" => self.kind = on,
            "mode" => self.mode = on,
            "owner" => self.owner = on,
            "group" => self.group = on,
            "size" => self.size = on,
            "date" => self.date = on,
            _ => {}
        }
    }
}

fn state_path() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::fsx::home().join(".local/state"))
        .join("lvim-commander/state.toml")
}

/// The file buffer (`yy` / `yx`, then `pp` …): what was copied or cut, shared by every lvc.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Buffer {
    pub cut: bool,
    pub paths: Vec<PathBuf>,
    /// The remote the paths are on (they are paths in it), when they are not on this machine.
    pub remote: Option<String>,
    /// For a remote buffer: which of the paths are directories (parallel to `paths`).
    pub dirs: Vec<bool>,
}

fn buffer_path() -> PathBuf {
    state_path().with_file_name("buffer.toml")
}

pub fn load_buffer() -> Buffer {
    let t = toml::from_str::<toml::Table>(&std::fs::read_to_string(buffer_path()).unwrap_or_default()).unwrap_or_default();
    Buffer {
        cut: t.get("cut").and_then(toml::Value::as_bool).unwrap_or(false),
        paths: t
            .get("paths")
            .and_then(toml::Value::as_array)
            .map(|a| a.iter().filter_map(toml::Value::as_str).map(PathBuf::from).collect())
            .unwrap_or_default(),
        remote: t.get("remote").and_then(toml::Value::as_str).map(str::to_string).filter(|r| !r.is_empty()),
        dirs: t.get("dirs").and_then(toml::Value::as_array).map(|a| a.iter().filter_map(toml::Value::as_bool).collect()).unwrap_or_default(),
    }
}

pub fn save_buffer(b: &Buffer) {
    let mut t = toml::Table::new();
    t.insert("cut".into(), b.cut.into());
    if let Some(r) = &b.remote {
        t.insert("remote".into(), r.clone().into());
        t.insert("dirs".into(), toml::Value::Array(b.dirs.iter().map(|d| toml::Value::Boolean(*d)).collect()));
    }
    t.insert("paths".into(), toml::Value::Array(b.paths.iter().map(|p| toml::Value::String(p.to_string_lossy().into_owned())).collect()));
    let p = buffer_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let tmp = p.with_extension(format!("tmp{}", std::process::id()));
    if std::fs::write(&tmp, toml::to_string(&t).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(&tmp, &p);
    }
}

/// The commands typed on the command line (`:` / `ax`), oldest first; kept between runs.
pub fn load_shell_history() -> Vec<String> {
    std::fs::read_to_string(state_path().with_file_name("shell_history")).unwrap_or_default().lines().map(str::to_string).collect()
}

pub fn save_shell_history(h: &[String]) {
    let p = state_path().with_file_name("shell_history");
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let from = h.len().saturating_sub(500);
    let _ = std::fs::write(&p, h[from..].join("\n") + "\n");
}

pub fn load_state() -> State {
    state_from_text(&std::fs::read_to_string(state_path()).unwrap_or_default())
}

fn state_from_text(text: &str) -> State {
    let t = toml::from_str::<toml::Table>(text).unwrap_or_default();
    let mut cols = Cols::default();
    if let Some(list) = t.get("columns").and_then(toml::Value::as_array) {
        let on: Vec<&str> = list.iter().filter_map(toml::Value::as_str).collect();
        for (k, _) in Cols::ALL {
            cols.set(k, on.contains(&k));
        }
    }
    let s = |t: &toml::Table, k: &str| t.get(k).and_then(toml::Value::as_str).unwrap_or("").to_string();
    let tabs: Vec<TabSave> = t
        .get("tabs")
        .and_then(toml::Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(toml::Value::as_table)
                .filter(|x| !s(x, "left").is_empty() && !s(x, "right").is_empty())
                .map(|x| TabSave {
                    name: Some(s(x, "name")).filter(|n| !n.is_empty()),
                    panels: [
                        PanelSave { dir: PathBuf::from(s(x, "left")), cursor: s(x, "left_cursor"), trash: x.get("left_trash").and_then(toml::Value::as_bool).unwrap_or(false) },
                        PanelSave { dir: PathBuf::from(s(x, "right")), cursor: s(x, "right_cursor"), trash: x.get("right_trash").and_then(toml::Value::as_bool).unwrap_or(false) },
                    ],
                    active: x.get("active").and_then(toml::Value::as_integer).unwrap_or(0).clamp(0, 1) as usize,
                    quick: x.get("quick").and_then(toml::Value::as_bool).unwrap_or(false),
                })
                .collect()
        })
        .unwrap_or_default();
    State {
        quick: t.get("quick").and_then(toml::Value::as_bool).unwrap_or(false),
        cols,
        tab: t.get("tab").and_then(toml::Value::as_integer).unwrap_or(0).max(0) as usize,
        tabs,
    }
}

fn state_to_text(st: &State) -> String {
    let mut t = toml::Table::new();
    t.insert("quick".into(), st.quick.into());
    t.insert(
        "columns".into(),
        toml::Value::Array(Cols::ALL.iter().filter(|(k, _)| st.cols.get(k)).map(|(k, _)| toml::Value::String((*k).into())).collect()),
    );
    t.insert("tab".into(), (st.tab as i64).into());
    let tabs: Vec<toml::Value> = st
        .tabs
        .iter()
        .map(|x| {
            let mut e = toml::Table::new();
            if let Some(n) = &x.name {
                e.insert("name".into(), n.clone().into());
            }
            e.insert("active".into(), (x.active as i64).into());
            e.insert("quick".into(), x.quick.into());
            e.insert("left".into(), x.panels[0].dir.to_string_lossy().into_owned().into());
            e.insert("left_cursor".into(), x.panels[0].cursor.clone().into());
            e.insert("right".into(), x.panels[1].dir.to_string_lossy().into_owned().into());
            e.insert("right_cursor".into(), x.panels[1].cursor.clone().into());
            e.insert("left_trash".into(), x.panels[0].trash.into());
            e.insert("right_trash".into(), x.panels[1].trash.into());
            toml::Value::Table(e)
        })
        .collect();
    t.insert("tabs".into(), toml::Value::Array(tabs));
    toml::to_string(&t).unwrap_or_default()
}

/// Written atomically (a temp file renamed over it); a failure is ignored — it is only a convenience.
pub fn save_state(st: &State) {
    let p = state_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let tmp = p.with_extension(format!("tmp{}", std::process::id()));
    if std::fs::write(&tmp, state_to_text(st)).is_ok() {
        let _ = std::fs::rename(&tmp, &p);
    }
}

#[cfg(test)]
mod state_tests {
    use super::*;

    #[test]
    fn the_tabs_survive_a_round_trip() {
        let tab = |name: Option<&str>, l: &str, r: &str, active, quick| TabSave {
            name: name.map(str::to_string),
            panels: [
                PanelSave { dir: PathBuf::from(l), cursor: "a \"quoted\" name".into(), trash: false },
                PanelSave { dir: PathBuf::from(r), cursor: String::new(), trash: quick },
            ],
            active,
            quick,
        };
        let st = State {
            quick: true,
            cols: Cols::default(),
            tab: 1,
            tabs: vec![tab(None, "/a", "/b", 0, false), tab(Some("build \u{1F527}"), "/c d", "/e", 1, true)],
        };
        assert_eq!(state_from_text(&state_to_text(&st)), st);
    }

    #[test]
    fn an_old_or_broken_state_gives_one_default_tab_set() {
        // the file an older lvc wrote: no tabs
        let st = state_from_text("quick = true\ncolumns = [\"size\"]\n");
        assert!(st.quick && st.tabs.is_empty() && st.tab == 0);
        assert!(state_from_text("not toml [").tabs.is_empty());
        // a tab without its directories is dropped
        let st = state_from_text("[[tabs]]\nname = \"x\"\n");
        assert!(st.tabs.is_empty());
    }
}

#[cfg(test)]
mod key_tests {
    use super::*;
    use crate::plugins::KeySpec;

    #[test]
    fn command_table_is_sound() {
        let mut seen = std::collections::HashSet::new();
        for (n, k) in COMMANDS {
            assert!(KeySpec::parse(k).is_some(), "{n}: {k}");
            assert!(seen.insert(*n), "duplicate command {n}");
            assert!(!FKEY_ACTIONS.iter().any(|(a, _)| a == n), "{n} is an F-key action");
        }
        assert!(is_command("find") && is_command("copy") && !is_command("git.log"));
    }
}
