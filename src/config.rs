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
    /// `yy` / `yx` also put the files themselves in the desktop clipboard (`text/uri-list`), so a browser or a file manager can paste them.
    pub clipboard_files: bool,
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
    /// `[keys]` entries that are a PLACE in the groups (`"zoom" = "v z"`): where a command or a button sits, moved.
    pub positions: Vec<(String, crate::app::Pos)>,
    /// `[previewers.*]`.
    pub previewers: Vec<crate::plugins::Previewer>,
    /// `[plugins.<name>]`: lvc's own overrides of the plugins' settings (the layer over the plugin's defaults and
    /// the shared `~/.config/lvim-plugins/<name>.toml`).
    pub plugins: toml::Table,
    /// The lowest level of notification that shows as a card (an index of `LEVEL_NAMES`): that level and the ones above it show. The list of
    /// notifications (`oi`) keeps them all.
    pub notify_min: usize,
    /// How many cards are on the screen at most, how many notifications the list keeps, and the seconds a card of each level stays.
    pub notify_cards: usize,
    pub notify_history: usize,
    pub notify_secs: [u64; 6],
    /// With the shell function (`lvc --zsh`): quitting also changes the shell to the folder lvc was in. Off: only Shift-Enter does (the folder of the choice).
    pub cd_on_quit: bool,
    /// The settings of the panel that `config.toml` sets itself: the panel shows them as fixed there.
    pub fixed: Vec<String>,
}

/// The names of the levels of a notification, in order.
pub const LEVEL_NAMES: [&str; 6] = ["debug", "info", "status", "success", "warn", "error"];

/// How a setting of the panel is changed.
#[derive(Clone, Copy)]
pub enum Kind {
    Bool,
    /// One of these (`<` / `>` step through them).
    Choice(&'static [&'static str]),
    /// A number between `min` and `max`, stepped by `step`.
    Num { min: i64, max: i64, step: i64 },
    /// The editors that are installed (and the one set).
    Editor,
    /// The themes in the themes folder (and the one set).
    Theme,
}

/// One setting of the panel (`oS`): `id` is its key in the settings file (`a.b` is `b` in the table `a`), `tab` the tab it is on, `when` says when a
/// change takes effect ("" at once).
pub struct Def {
    pub id: &'static str,
    pub tab: usize,
    pub label: &'static str,
    pub kind: Kind,
    pub when: &'static str,
    pub help: &'static str,
}

impl Def {
    /// The key of the file this setting is under.
    pub fn key(&self) -> &str {
        self.id.split('.').next().unwrap_or(self.id)
    }
}

pub const TABS: [&str; 4] = ["General", "Files", "Notifications", "Display"];

const SECS: Kind = Kind::Num { min: 1, max: 60, step: 1 };

pub const DEFS: &[Def] = &[
    Def { id: "editor", tab: 0, label: "editor", kind: Kind::Editor, when: "", help: "the program F4 / e opens files with" },
    Def { id: "preview", tab: 0, label: "preview", kind: Kind::Choice(&["nvim", "file", "off"]), when: "next start", help: "nvim: Neovim shows the file; file: the plain text; off: no preview" },
    Def { id: "pool", tab: 0, label: "keep a Neovim ready", kind: Kind::Bool, when: "next preview", help: "a Neovim waits so the next preview opens at once" },
    Def { id: "key_hints", tab: 0, label: "key hints after Ctrl-c", kind: Kind::Bool, when: "", help: "the bar of the keys that follow a leader key (v H)" },
    Def { id: "theme", tab: 0, label: "theme", kind: Kind::Theme, when: "next start", help: "a theme of ~/.config/lvim-commander/themes" },
    Def { id: "cd_on_quit", tab: 1, label: "quitting changes the shell's folder", kind: Kind::Bool, when: "", help: "with the shell function (lvc --zsh): the shell goes to the folder lvc was in; off: only Shift-Enter changes it" },
    Def { id: "hidden", tab: 1, label: "hidden files", kind: Kind::Bool, when: "", help: "show the files that start with a dot (v h in a panel)" },
    Def { id: "delete", tab: 1, label: "delete goes to", kind: Kind::Choice(&["trash", "direct"]), when: "", help: "trash: recoverable; direct: gone at once" },
    Def { id: "clipboard_files", tab: 1, label: "copy files to the desktop clipboard", kind: Kind::Bool, when: "", help: "yy / yx also put the files themselves in it, so a browser or a file manager can paste them" },
    Def { id: "watch", tab: 1, label: "follow changes made outside", kind: Kind::Bool, when: "next start", help: "the panels refresh when another program changes a folder they show" },
    Def { id: "notifications", tab: 2, label: "show from the level", kind: Kind::Choice(&LEVEL_NAMES), when: "", help: "that level and the ones above it show as a card; the list (o i) keeps all" },
    Def { id: "notification_cards", tab: 2, label: "cards on the screen at most", kind: Kind::Num { min: 1, max: 8, step: 1 }, when: "", help: "the oldest card goes when there are more" },
    Def { id: "notification_history", tab: 2, label: "notifications the list keeps", kind: Kind::Num { min: 20, max: 1000, step: 20 }, when: "", help: "the list of o i" },
    Def { id: "notification_seconds.debug", tab: 2, label: "seconds a debug card stays", kind: SECS, when: "", help: "" },
    Def { id: "notification_seconds.info", tab: 2, label: "seconds an info card stays", kind: SECS, when: "", help: "" },
    Def { id: "notification_seconds.status", tab: 2, label: "seconds a status card stays", kind: SECS, when: "", help: "a status becomes its result when that comes" },
    Def { id: "notification_seconds.success", tab: 2, label: "seconds a success card stays", kind: SECS, when: "", help: "" },
    Def { id: "notification_seconds.warn", tab: 2, label: "seconds a warn card stays", kind: SECS, when: "", help: "" },
    Def { id: "notification_seconds.error", tab: 2, label: "seconds an error card stays", kind: SECS, when: "", help: "" },
    Def { id: "icons", tab: 3, label: "file icons", kind: Kind::Choice(&["auto", "image", "font"]), when: "next start", help: "auto: Material images where the terminal takes them; image: always; font: Nerd Font glyphs" },
    Def { id: "zellij_images", tab: 3, label: "images inside Zellij", kind: Kind::Bool, when: "next start", help: "for a Zellij build that takes kitty placeholders" },
    Def { id: "image_transfer", tab: 3, label: "how a picture reaches the terminal", kind: Kind::Choice(&["file", "direct"]), when: "next start", help: "file: through a temporary file (fast); direct: the bytes through the terminal" },
];

/// Is `k` a key of the settings file of the panel?
pub fn panel_key(k: &str) -> bool {
    DEFS.iter().any(|d| d.key() == k)
}

impl Config {
    /// The value of a setting of the panel, as text.
    pub fn get(&self, id: &str) -> String {
        let b = |v: bool| if v { "on" } else { "off" }.to_string();
        match id {
            "editor" => self.editor.clone(),
            "preview" => match self.preview {
                PreviewKind::Nvim => "nvim",
                PreviewKind::File => "file",
                PreviewKind::Off => "off",
            }
            .into(),
            "pool" => b(self.pool),
            "key_hints" => b(self.key_hints),
            "theme" => self.theme.clone().unwrap_or_default(),
            "hidden" => b(self.hidden),
            "delete" => if self.delete == Delete::Direct { "direct" } else { "trash" }.into(),
            "clipboard_files" => b(self.clipboard_files),
            "watch" => b(self.watch),
            "cd_on_quit" => b(self.cd_on_quit),
            "notifications" => LEVEL_NAMES[self.notify_min.min(5)].into(),
            "notification_cards" => self.notify_cards.to_string(),
            "notification_history" => self.notify_history.to_string(),
            "icons" => match self.icons {
                lvim_fuzzy_ui::iconimg::Mode::Auto => "auto",
                lvim_fuzzy_ui::iconimg::Mode::Image => "image",
                lvim_fuzzy_ui::iconimg::Mode::Font => "font",
            }
            .into(),
            "zellij_images" => b(self.zellij_images),
            "image_transfer" => if self.image_transfer_file { "file" } else { "direct" }.into(),
            other => other
                .strip_prefix("notification_seconds.")
                .and_then(|l| LEVEL_NAMES.iter().position(|n| *n == l))
                .map(|i| self.notify_secs[i].to_string())
                .unwrap_or_default(),
        }
    }

    /// Set a setting of the panel from its text (as `get` gives it).
    pub fn set(&mut self, id: &str, v: &str) {
        let on = v == "on";
        match id {
            "editor" => self.editor = v.into(),
            "preview" => {
                self.preview = match v {
                    "file" => PreviewKind::File,
                    "off" => PreviewKind::Off,
                    _ => PreviewKind::Nvim,
                }
            }
            "pool" => self.pool = on,
            "key_hints" => self.key_hints = on,
            "theme" => self.theme = Some(v.into()),
            "hidden" => self.hidden = on,
            "delete" => self.delete = if v == "direct" { Delete::Direct } else { Delete::Trash },
            "clipboard_files" => self.clipboard_files = on,
            "watch" => self.watch = on,
            "cd_on_quit" => self.cd_on_quit = on,
            "notifications" => self.notify_min = LEVEL_NAMES.iter().position(|n| *n == v).unwrap_or(0),
            "notification_cards" => self.notify_cards = v.parse().unwrap_or(4),
            "notification_history" => self.notify_history = v.parse().unwrap_or(200),
            "icons" => self.icons = lvim_fuzzy_ui::iconimg::Mode::parse(v).unwrap_or(lvim_fuzzy_ui::iconimg::Mode::Auto),
            "zellij_images" => self.zellij_images = on,
            "image_transfer" => self.image_transfer_file = v != "direct",
            other => {
                if let Some(i) = other.strip_prefix("notification_seconds.").and_then(|l| LEVEL_NAMES.iter().position(|n| *n == l)) {
                    self.notify_secs[i] = v.parse().unwrap_or(3);
                }
            }
        }
    }
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
            clipboard_files: true,
            bookmarks: HashMap::new(),
            actions: Vec::new(),
            previewers: Vec::new(),
            plugins: toml::Table::new(),
            notify_min: 0,
            notify_cards: 4,
            notify_history: 200,
            notify_secs: [2, 3, 3, 3, 6, 9],
            cd_on_quit: false,
            fixed: Vec::new(),
            icons: lvim_fuzzy_ui::iconimg::Mode::Auto,
            zellij_images: true,
            image_transfer_file: true,
            keys: Vec::new(),
            positions: Vec::new(),
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
    load_with(&path(), Some(settings_path()))
}

/// `load` from a given file.
/// The config at `p` alone (no settings file under it).
#[cfg(test)]
pub fn load_from(p: &std::path::Path) -> (Config, Vec<String>) {
    load_with(p, None)
}

/// The config at `p` over the settings the panel kept in `sp`.
fn load_with(p: &std::path::Path, sp: Option<PathBuf>) -> (Config, Vec<String>) {
    let mut c = Config::default();
    let mut errs = Vec::new();
    let p = p.to_path_buf();
    // what the settings panel kept: under the config, which wins
    if let Some(sp) = sp
        && let Ok(text) = std::fs::read_to_string(&sp)
    {
        match toml::from_str::<toml::Table>(&text) {
            Err(e) => errs.push(format!("{}: {e}", sp.display())),
            Ok(t) => {
                for (k, val) in t {
                    if panel_key(&k) {
                        apply_key(&mut c, &k, val, &sp, &mut errs);
                    } else {
                        errs.push(format!("{}: `{k}` is not a setting of the panel ({})", sp.display(), "see the panel: oS"));
                    }
                }
            }
        }
    }
    if let Ok(text) = std::fs::read_to_string(&p) {
        match toml::from_str::<toml::Table>(&text) {
            Err(e) => errs.push(format!("{}: {e}", p.display())),
            Ok(t) => {
                for (k, val) in t {
                    if panel_key(&k) {
                        c.fixed.push(k.clone());
                    }
                    apply_key(&mut c, &k, val, &p, &mut errs);
                }
            }
        }
    }
    if c.theme.is_none() {
        c.theme = lvim_fuzzy_theme();
    }
    (c, errs)
}

/// One `key = value` of a settings file (the config, or the panel's).
fn apply_key(c: &mut Config, k: &str, val: toml::Value, p: &std::path::Path, errs: &mut Vec<String>) {
    let bad = |errs: &mut Vec<String>, what: &str| errs.push(format!("{}: `{k}` must be {what}", p.display()));
    match k {
        "theme" => match val.as_str() {
            Some(s) => c.theme = Some(s.into()),
            None => bad(errs, "a string"),
        },
        "delete" => match val.as_str() {
            Some("trash") => c.delete = Delete::Trash,
            Some("direct") => c.delete = Delete::Direct,
            _ => bad(errs, "\"trash\" or \"direct\""),
        },
        "watch" => match val.as_bool() {
            Some(b) => c.watch = b,
            None => bad(errs, "true or false"),
        },
        "key_hints" => match val.as_bool() {
            Some(b) => c.key_hints = b,
            None => bad(errs, "true or false"),
        },
        "hidden" => match val.as_bool() {
            Some(b) => c.hidden = b,
            None => bad(errs, "true or false"),
        },
        "editor" => match val.as_str() {
            Some(s) => c.editor = s.into(),
            None => bad(errs, "a string"),
        },
        "preview" => match val.as_str() {
            Some("nvim") => c.preview = PreviewKind::Nvim,
            Some("file") => c.preview = PreviewKind::File,
            Some("off") => c.preview = PreviewKind::Off,
            _ => bad(errs, "\"nvim\", \"file\" or \"off\""),
        },
        "nvim" => match val.as_str() {
            Some(s) => c.nvim = s.into(),
            None => bad(errs, "a string"),
        },
        "pool" => match val.as_bool() {
            Some(b) => c.pool = b,
            None => bad(errs, "true or false"),
        },
        "git" | "git_ignored" => errs.push(format!(
            "{}: `{k}` is gone: git is the git plugin's now — install it with its `scripts/install.sh` and set `ignored` / `untracked` in its configuration",
            p.display()
        )),
        // (an early panel kept a list of the levels that show: the lowest of them is the level now)
        "notifications" if val.is_array() => {
            let low = val.as_array().and_then(|a| a.iter().filter_map(|v| v.as_str().and_then(|n| LEVEL_NAMES.iter().position(|l| *l == n))).min());
            c.notify_min = low.unwrap_or(0);
        }
        "notifications" => match val.as_str().and_then(|n| LEVEL_NAMES.iter().position(|l| *l == n)) {
            Some(i) => c.notify_min = i,
            None => bad(errs, "the lowest level that shows: debug, info, status, success, warn or error"),
        },
        "notification_cards" => match val.as_integer().filter(|n| (1..=8).contains(n)) {
            Some(n) => c.notify_cards = n as usize,
            None => bad(errs, "a number from 1 to 8"),
        },
        "notification_history" => match val.as_integer().filter(|n| (20..=1000).contains(n)) {
            Some(n) => c.notify_history = n as usize,
            None => bad(errs, "a number from 20 to 1000"),
        },
        "notification_seconds" => match val.as_table() {
            Some(t) => {
                for (level, secs) in t {
                    match (LEVEL_NAMES.iter().position(|n| n == level), secs.as_integer().filter(|n| (1..=60).contains(n))) {
                        (Some(i), Some(n)) => c.notify_secs[i] = n as u64,
                        _ => errs.push(format!("{}: notification_seconds.{level} must be a number of seconds, 1 to 60, of a level (debug info status success warn error)", p.display())),
                    }
                }
            }
            None => bad(errs, "a table: debug = 2, warn = 6, …"),
        },
        "cd_on_quit" => match val.as_bool() {
            Some(b) => c.cd_on_quit = b,
            None => bad(errs, "true or false"),
        },
        "clipboard_files" => match val.as_bool() {
            Some(b) => c.clipboard_files = b,
            None => bad(errs, "true or false"),
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
            None => bad(errs, "a table"),
        },
        "icons" => match val.as_str().and_then(lvim_fuzzy_ui::iconimg::Mode::parse) {
            Some(m) => c.icons = m,
            None => bad(errs, "\"auto\", \"image\" or \"font\""),
        },
        "image_transfer" => match val.as_str() {
            Some("file") => c.image_transfer_file = true,
            Some("direct") => c.image_transfer_file = false,
            _ => bad(errs, "\"file\" or \"direct\""),
        },
        "zellij_images" => match val.as_bool() {
            Some(b) => c.zellij_images = b,
            None => bad(errs, "true or false"),
        },
        "leader" => errs.push(format!(
            "{}: `leader` is gone: commands are key groups now (t tabs, m marks, a actions, y copy, f find, g go, v view); F1 or g? lists them",
            p.display()
        )),
        "keys" => match val.as_table() {
            Some(m) => {
                for (name, key) in m {
                    // a place in the groups (`"v z"`, `"x a a"`): a button moved
                    if let Some(pos) = key.as_str().and_then(crate::app::parse_pos) {
                        c.positions.push((name.clone(), pos));
                        continue;
                    }
                    // (a name that is no command is a plugin / config action: checked once the plugins are loaded)
                    match key.as_str().and_then(crate::plugins::KeySpec::parse).filter(|k| !k.chord) {
                        Some(ks) => c.keys.push((name.clone(), ks)),
                        None => errs.push(format!(
                            "{}: keys.{name}: a key like \"f5\", \"shift-f8\", \"alt-x\", or a place in the groups like \"v z\" or \"x a a\"",
                            p.display()
                        )),
                    }
                }
            }
            None => bad(errs, "a table: action = \"key\""),
        },
        "actions" => {
            let at = p.display().to_string();
            c.actions = crate::plugins::parse_actions(&val, "config", None, crate::plugins::Runtime::Command, &at, errs);
        }
        "previewers" => {
            let at = p.display().to_string();
            c.previewers = crate::plugins::parse_previewers(&val, None, &at, errs);
        }
        "plugins" => match val.as_table() {
            Some(t) if t.values().all(|x| x.is_table()) => c.plugins = t.clone(),
            _ => bad(errs, "a table of tables: [plugins.<plugin name>]"),
        },
        other => errs.push(format!(
            "{}: unknown key `{other}` (theme, keys, delete, hidden, notifications, key_hints, editor, preview, nvim, pool, clipboard_files, icons, zellij_images, bookmarks, actions, previewers, plugins)",
            p.display()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_may_be_a_key_or_a_place_in_the_groups() {
        let dir = std::env::temp_dir().join(format!("lvc-keys-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("config.toml");
        std::fs::write(&f, "[keys]\nzoom = \"v y\"\nfind = \"alt-f\"\n\"p.stage\" = \"x a z\"\nbroken = \"nonsense here too\"\n").unwrap();
        let (c, errs) = load_from(&f);
        assert_eq!(c.positions.len(), 2);
        assert!(c.positions.iter().any(|(n, p)| n == "zoom" && p.group == 'v' && p.key == 'y'));
        assert!(c.positions.iter().any(|(n, p)| n == "p.stage" && p.sub == Some('a')));
        // `alt-f` is a key, and a value that is neither is an error
        assert_eq!(c.keys.len(), 1);
        assert_eq!(errs.len(), 1, "{errs:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_columns_are_an_ordered_list_that_the_state_keeps() {
        let mut c = Cols::default();
        assert_eq!(c.list[0], ("kind".to_string(), false));
        assert!(c.get("size") && !c.get("kind"));
        // J / K: a column moves
        let at = c.move_by(0, 1);
        assert_eq!((at, c.list[1].0.as_str()), (1, "kind"));
        // a plugin's column joins at the end, once (or first, when it says so)
        c.ensure("p.size2", true, false);
        c.ensure("p.size2", false, false);
        c.ensure("p.marks", true, true);
        assert_eq!(c.list[0].0, "p.marks");
        c.list.remove(0);
        assert_eq!(c.list.last().unwrap(), &("p.size2".to_string(), true));
        // saved and read back: order and on / off
        c.set("size", false);
        let on: Vec<String> = c.list.iter().filter(|(_, on)| *on).map(|(k, _)| k.clone()).collect();
        let order: Vec<String> = c.list.iter().map(|(k, _)| k.clone()).collect();
        assert_eq!(Cols::from_state(Some(order), Some(on)), c);
        // an old state file (only what is on): the built-in order, those on
        let old = Cols::from_state(None, Some(vec!["size".into()]));
        assert!(old.get("size") && !old.get("date"));
        // a column the file does not know is on when it is on by default
        let new = Cols::from_state(Some(vec!["kind".into()]), None);
        assert_eq!(new.list[0].0, "kind");
        assert!(new.get("date") && !new.get("kind"));
    }

    #[test]
    fn bookmarks_are_kept_in_a_file_and_read_back() {
        let list = vec![
            Bookmark { name: "work".into(), remote: None, path: "/home/me/work".into() },
            Bookmark { name: "box".into(), remote: Some("neterra".into()), path: "/srv/app".into() },
        ];
        let text = render_bookmarks(&list);
        assert_eq!(parse_bookmarks(&text), list);
        // a broken file is no bookmarks, not a crash
        assert!(parse_bookmarks("[[bookmark\nname=").is_empty());
        assert!(parse_bookmarks("").is_empty());
    }

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
    ("bookmark-add", "leader B"),
    ("bookmark-delete", "leader W"),
    ("retarget", "leader X"),
    ("bookmark-rename", "leader N"),
    ("viewer", "leader V"),
    ("refresh", "leader R"),
    ("sync", "leader /"),
    ("subshell", "leader {"),
    ("tasks", "leader !"),
    ("cd", "leader P"),
    ("notifications", "leader T"),
    ("settings", "leader C"),
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

/// The columns beside a name, right of it, in the order they are drawn, each on or off (`vc`; `J` / `K` move one in the list). The built-in ones
/// and the ones plugins bring (`[columns.<id>]` in a `plugin.toml`, `<plugin>.<id>`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cols {
    pub list: Vec<(String, bool)>,
}

impl Default for Cols {
    fn default() -> Cols {
        Cols { list: Cols::BUILTIN.iter().map(|(k, _, on)| ((*k).to_string(), *on)).collect() }
    }
}

impl Cols {
    /// (key, label, on by default) in the order they start in.
    pub const BUILTIN: [(&'static str, &'static str, bool); 6] = [
        ("kind", "type (DIR · EXE · LNK · BRK · FILE)", false),
        ("mode", "permissions (rwxr-xr-x)", false),
        ("owner", "owner", false),
        ("group", "group", false),
        ("size", "size (a directory: its entries, (23))", true),
        ("date", "date modified (deleted, in the trash)", true),
    ];

    pub fn get(&self, key: &str) -> bool {
        self.list.iter().any(|(k, on)| k == key && *on)
    }

    /// Turn a column on or off (a column not in the list yet joins it at the end).
    pub fn set(&mut self, key: &str, on: bool) {
        match self.list.iter_mut().find(|(k, _)| k == key) {
            Some(e) => e.1 = on,
            None => self.list.push((key.to_string(), on)),
        }
    }

    /// Make sure a column is in the list (a plugin's, the first time it is seen): at the end — or first, for a column that says so — with `on` as its state.
    pub fn ensure(&mut self, key: &str, on: bool, first: bool) {
        if !self.list.iter().any(|(k, _)| k == key) {
            if first {
                self.list.insert(0, (key.to_string(), on));
            } else {
                self.list.push((key.to_string(), on));
            }
        }
    }

    /// Move the column at `i` one place up (`d < 0`) or down; returns where it is now.
    pub fn move_by(&mut self, i: usize, d: isize) -> usize {
        let to = (i as isize + d).clamp(0, self.list.len() as isize - 1) as usize;
        if to != i {
            let e = self.list.remove(i);
            self.list.insert(to, e);
        }
        to
    }

    /// The built-in label of a column.
    pub fn label(key: &str) -> Option<&'static str> {
        Cols::BUILTIN.iter().find(|(k, _, _)| *k == key).map(|(_, l, _)| *l)
    }

    /// From the state file: `order` (all the columns known, in order) and `on` (the ones that are on). A column the file does not know
    /// is on when it is on by default; one that is in `on` only (an older file) joins the order at the end.
    pub fn from_state(order: Option<Vec<String>>, on: Option<Vec<String>>) -> Cols {
        let mut c = Cols::default();
        if let Some(order) = &order {
            let mut list: Vec<(String, bool)> = Vec::new();
            for k in order.iter().filter(|k| k.as_str() != "git") {
                if !list.iter().any(|(x, _)| x == k) {
                    list.push((k.clone(), c.get(k)));
                }
            }
            for (k, v) in &c.list {
                if !list.iter().any(|(x, _)| x == k) {
                    list.push((k.clone(), *v));
                }
            }
            c.list = list;
        }
        if let Some(on) = on {
            for (k, v) in &mut c.list {
                *v = on.contains(k);
            }
            for k in on.iter().filter(|k| k.as_str() != "git") {
                c.ensure(k, true, false);
            }
        }
        c
    }
}

/// `$XDG_STATE_HOME/lvim-commander/settings.toml`: what the settings panel (`oS`) keeps. `config.toml` wins over it.
pub fn settings_path() -> PathBuf {
    state_path().with_file_name("settings.toml")
}

/// The panel's settings written down (all but what `config.toml` fixes).
pub fn save_settings(c: &Config) -> Result<(), String> {
    let mut t = toml::Table::new();
    for d in DEFS.iter().filter(|d| !c.fixed.iter().any(|f| f == d.key())) {
        let v = c.get(d.id);
        let val = match d.kind {
            Kind::Bool => toml::Value::Boolean(v == "on"),
            Kind::Num { .. } => toml::Value::Integer(v.parse().unwrap_or(0)),
            _ => toml::Value::String(v),
        };
        match d.id.split_once('.') {
            Some((table, key)) => {
                let e = t.entry(table).or_insert_with(|| toml::Value::Table(toml::Table::new()));
                if let Some(tt) = e.as_table_mut() {
                    tt.insert(key.into(), val);
                }
            }
            None => {
                t.insert(d.id.into(), val);
            }
        }
    }
    let path = settings_path();
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    std::fs::write(&path, format!("# written by lvc's settings panel (oS); config.toml wins over it\n{t}")).map_err(|e| format!("{}: {e}", path.display()))
}

fn state_path() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::fsx::home().join(".local/state"))
        .join("lvim-commander/state.toml")
}

/// A place the user keeps (`ba` adds the one the panel is in, `bl` lists them, `bd` deletes one): a name, a directory, and the remote when it is a
/// directory of one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bookmark {
    pub name: String,
    pub remote: Option<String>,
    pub path: String,
}

/// `$XDG_STATE_HOME/lvim-commander/bookmarks.toml`: shared by every lvc, written as the places are added and removed.
pub fn bookmarks_path() -> PathBuf {
    state_path().with_file_name("bookmarks.toml")
}

pub fn parse_bookmarks(text: &str) -> Vec<Bookmark> {
    let Ok(t) = text.parse::<toml::Table>() else { return Vec::new() };
    t.get("bookmark")
        .and_then(toml::Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|b| {
                    Some(Bookmark {
                        name: b.get("name")?.as_str()?.to_string(),
                        path: b.get("path")?.as_str()?.to_string(),
                        remote: b.get("remote").and_then(toml::Value::as_str).filter(|r| !r.is_empty()).map(str::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn render_bookmarks(list: &[Bookmark]) -> String {
    let mut out = String::from("# The places kept in lvc (ba adds, bl lists, bd deletes; d deletes and r renames in the list).\n");
    for b in list {
        let mut t = toml::Table::new();
        t.insert("name".into(), b.name.clone().into());
        if let Some(r) = &b.remote {
            t.insert("remote".into(), r.clone().into());
        }
        t.insert("path".into(), b.path.clone().into());
        let mut top = toml::Table::new();
        top.insert("bookmark".into(), toml::Value::Array(vec![toml::Value::Table(t)]));
        out.push('\n');
        out.push_str(&toml::to_string(&top).unwrap_or_default());
    }
    out
}

pub fn load_bookmarks() -> Vec<Bookmark> {
    std::fs::read_to_string(bookmarks_path()).map(|t| parse_bookmarks(&t)).unwrap_or_default()
}

pub fn save_bookmarks(list: &[Bookmark]) -> Result<(), String> {
    let p = bookmarks_path();
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    // written beside and moved: another lvc never reads half a file
    let tmp = p.with_extension(format!("toml.{}", std::process::id()));
    std::fs::write(&tmp, render_bookmarks(list)).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &p).map_err(|e| format!("{}: {e}", p.display()))
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
    let list = |k: &str| t.get(k).and_then(toml::Value::as_array).map(|a| a.iter().filter_map(toml::Value::as_str).map(str::to_string).collect::<Vec<_>>());
    let cols = Cols::from_state(list("column_order"), list("columns"));
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
    t.insert("columns".into(), toml::Value::Array(st.cols.list.iter().filter(|(_, on)| *on).map(|(k, _)| toml::Value::String(k.clone())).collect()));
    t.insert("column_order".into(), toml::Value::Array(st.cols.list.iter().map(|(k, _)| toml::Value::String(k.clone())).collect()));
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
