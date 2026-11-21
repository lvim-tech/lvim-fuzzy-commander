//! The application: two panels (Midnight Commander style), the quick view, the dialogs, the jobs, and the
//! keys that drive them.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use crossterm::style::Attribute;
use lvim_fuzzy_ui::canvas::{Canvas, Rect, str_width, truncate};
use lvim_fuzzy_ui::filepreview::Loader;
use lvim_fuzzy_ui::lvim::Look;
use lvim_fuzzy_ui::nvim::Nvim;
use lvim_fuzzy_ui::theme::Style;

use crate::config::{Config, Delete, PreviewKind};
use crate::fsx::{self, Kind};
use crate::ops::{self, Conflict, Job, JobKind, Report};
use crate::panel::{Panel, Place};
use crate::term::Term;

/// A plugin function to call: the plugin's name and the function.
#[derive(Clone, Debug)]
struct PluginCall {
    plugin: String,
    call: String,
}

/// What a text-input dialog does with its text.
#[derive(Clone, Debug)]
enum InputAction {
    /// The text goes to a plugin function (an `input` instruction).
    Plugin(PluginCall),
    Mkdir,
    NewFile,
    Rename(PathBuf),
    /// The name of the current tab (empty: back to the directory's name).
    TabName,
    CopyTo(Vec<PathBuf>),
    MoveTo(Vec<PathBuf>),
    Mark(bool),
    Goto,
    Shell,
    Chmod(Vec<PathBuf>),
    Chown(Vec<PathBuf>),
    Compress(PathBuf, Vec<String>),
    /// links to these in this directory; `relative`: the link holds a relative path (Tab toggles)
    Symlink { targets: Vec<PathBuf>, dir: PathBuf, relative: bool },
}

/// What a yes/no dialog confirms.
#[derive(Clone, Debug)]
enum ConfirmAction {
    /// Yes calls a plugin function (a `confirm` instruction).
    Plugin(PluginCall),
    Delete(Vec<PathBuf>, bool),
    Purge(Vec<crate::trash::Item>),
    EmptyTrash,
}

enum Dialog {
    Input {
        title: String,
        value: String,
        cur: usize,
        action: InputAction,
    },
    Confirm {
        title: String,
        lines: Vec<String>,
        action: ConfirmAction,
    },
    Conflict {
        target: PathBuf,
        answer: Sender<(Conflict, bool)>,
    },
    Message {
        title: String,
        lines: Vec<String>,
    },
    /// Ctrl-c m: the permissions, as a field (octal or symbolic) and as ticks; each follows the other.
    Mode {
        paths: Vec<PathBuf>,
        value: String,
        cur: usize,
        /// the mode the field is read against (symbolic `u+x` is relative to it)
        base: u32,
        is_dir: bool,
        /// what the ticks show: the field's mode when it reads, else the last one that did
        bits: u32,
        /// 0 = the field, 1..=12 = a tick (MODE_BITS)
        focus: usize,
    },
    /// Ctrl-c O: the owner and the group, each a drop-down of the real users / groups.
    Owner {
        paths: Vec<PathBuf>,
        users: Vec<String>,
        groups: Vec<String>,
        user: usize,
        group: usize,
        /// 0 = user, 1 = group, 2 = OK, 3 = Cancel
        focus: usize,
        /// an open drop-down: (the query typed, the selected row among the matches)
        open: Option<(String, usize)>,
    },
    /// Ctrl-c v: the columns beside the names, as ticks (they change at once).
    Columns { sel: usize },
    /// A list to pick from: bookmarks and recent directories (Alt-b), the user actions (F9).
    Menu {
        title: String,
        items: Vec<(String, MenuItem)>,
        sel: usize,
    },
}

/// The ticks of the permissions dialog: owner r w x, group r w x, others r w x, setuid, setgid, sticky.
const MODE_BITS: [u32; 12] = [
    0o400, 0o200, 0o100, 0o040, 0o020, 0o010, 0o004, 0o002, 0o001, 0o4000, 0o2000, 0o1000,
];

#[derive(Clone, Debug)]
enum MenuItem {
    /// A `menu` instruction's entry: the plugin function to call, with an optional argument.
    Plugin(PluginCall, Option<String>),
    Dir(PathBuf),
    /// An index into `App::actions`.
    Action(usize),
    /// A built-in operation (the F9 menu lists them before the user actions).
    Builtin(Builtin),
    /// Another instance (Ctrl-c I), then what to do with it.
    Peer(crate::instances::Peer),
    PeerDo(crate::instances::Peer, PeerDo),
}

#[derive(Clone, Copy, Debug)]
enum PeerDo {
    /// this panel goes to its directory
    GoThere,
    /// it goes to this panel's directory
    BringHere,
    /// copy / move the marked items into its directory
    Copy,
    Move,
}

#[derive(Clone, Copy, Debug)]
enum Builtin {
    Copy,
    Move,
    Rename,
    NewFile,
    Trash,
    Delete,
    Chmod,
    Chown,
    Compress,
    Extract,
    Edit,
    View,
    Find,
    Grep,
    Goto,
    Bookmarks,
    Shell,
    TrashView,
    Hidden,
    Long,
    Sort,
    Swap,
    Undo,
    Reload,
    Help,
}

/// The commands after Ctrl-c, as the which-key bar shows them: (key, label, where: 0 = everywhere,
/// 1 = in a directory, 2 = in the trash).
const CHORDS: &[(&str, &str, u8, &str)] = &[
    ("f", "find files", 1, "find"),
    ("g", "grep", 1, "grep"),
    ("c", "go to dir", 0, "goto"),
    ("b", "bookmarks", 0, "bookmarks"),
    ("x", "shell", 1, "shell"),
    ("t", "trash", 0, "trash-view"),
    ("r", "restore", 2, "restore"),
    ("e", "empty trash", 2, "empty-trash"),
    ("m", "mode", 1, "chmod"),
    ("O", "owner", 1, "chown"),
    ("z", "compress", 1, "compress"),
    ("e", "extract", 1, "extract"),
    ("u", "undo", 0, "undo"),
    ("a", "mark all", 0, "mark-all"),
    ("*", "invert marks", 0, "invert-marks"),
    ("+", "mark glob", 0, "mark-glob"),
    ("-", "unmark glob", 0, "unmark-glob"),
    ("h", "hidden", 0, "hidden"),
    ("l", "long view", 0, "long"),
    ("v", "columns", 0, "columns"),
    ("n", "new tab", 0, "new-tab"),
    (",", "rename tab", 0, "rename-tab"),
    ("<", "tab left", 0, "tab-left"),
    (">", "tab right", 0, "tab-right"),
    ("I", "other lvc", 0, "instances"),
    ("w", "close tab", 0, "close-tab"),
    ("y", "copy path", 0, "copy-path"),
    ("Y", "copy name", 0, "copy-name"),
    ("D", "copy dir", 0, "copy-dir"),
    ("L", "symlink", 1, "symlink"),
    ("s", "sort", 0, "sort"),
    ("S", "reverse", 0, "reverse"),
    ("p", "swap panels", 0, "swap"),
    ("o", "dir → other", 0, "dir-to-other"),
    ("i", "here → other", 0, "here-to-other"),
    ("[", "back", 0, "back"),
    ("]", "forward", 0, "forward"),
    ("r", "reload", 1, "reload"),
    ("?", "help", 0, "keys-help"),
    ("H", "hide hints", 0, "hints"),
    ("q", "quit", 0, "exit"),
];

const BUILTINS: &[(Builtin, &str, &str)] = &[
    (Builtin::Copy, "Copy", "F3"),
    (Builtin::Move, "Move", "F5"),
    (Builtin::Rename, "Rename", "F4"),
    (Builtin::NewFile, "New file or directory (name/)", "F7"),
    (Builtin::Trash, "Move to the trash", "F8"),
    (Builtin::Delete, "Delete for good", "S-F8"),
    (Builtin::Chmod, "Change the mode (permissions)", "C-c m"),
    (Builtin::Chown, "Change the owner and group", "C-c O"),
    (Builtin::Compress, "Compress", "C-c z"),
    (Builtin::Extract, "Extract", "C-c e"),
    (Builtin::Edit, "Edit", "F6"),
    (Builtin::View, "Quick view", "F2"),
    (Builtin::Find, "Find files (lvim-fuzzy)", "C-c f"),
    (Builtin::Grep, "Grep (lvim-fuzzy)", "C-c g"),
    (Builtin::Goto, "Go to a directory", "C-c c"),
    (Builtin::Bookmarks, "Bookmarks · recent", "C-c b"),
    (Builtin::Shell, "Shell command here", "C-c x"),
    (Builtin::TrashView, "The trash", "C-c t"),
    (Builtin::Hidden, "Show / hide hidden files", "."),
    (Builtin::Long, "Long view", "C-c l"),
    (Builtin::Sort, "Sort order", "C-c s"),
    (Builtin::Swap, "Swap the panels", "C-c p"),
    (Builtin::Undo, "Undo", "C-c u"),
    (Builtin::Reload, "Reload", "C-c r"),
    (Builtin::Help, "Keys", "F1"),
];

/// What a background user action reports back.
struct ActionDone {
    title: String,
    /// `control` output: one instruction per line.
    lines: Vec<String>,
    control: bool,
    reload: bool,
    ok: bool,
    err: String,
}

struct RunningJob {
    job: Job,
    done: u64,
    total: u64,
    file: String,
    started: Instant,
}

struct Toast {
    text: String,
    error: bool,
    until: Instant,
}

/// What the program prints when it ends (`--print-dir`).
pub struct Outcome {
    pub dir: Option<PathBuf>,
}

pub struct App {
    /// Ctrl-c was pressed: the next key is a command (every command outside the F-keys is Ctrl-c <key>).
    chord: bool,
    /// the F-key actions: (action, its key now, the key it stands for in the code: its default F-key)
    fkeys: Vec<(&'static str, crate::plugins::KeySpec, KeyEvent)>,
    /// `[keys]` for every other command: (name, the key it is on now, its default key).
    cmdkeys: Vec<(&'static str, crate::plugins::KeySpec, crate::plugins::KeySpec)>,
    /// The key that starts a command (`leader`), as the bar shows it (C-c) and in words (Ctrl-c).
    leader: crate::plugins::KeySpec,
    leader_label: String,
    leader_name: String,
    /// a press made by lvc itself (a button of the bar): already the action's own F-key, not translated
    synthetic: bool,
    /// Ctrl-j: the focus is on the button bar (the F-keys), at this button; h / l move, Enter presses
    footer_focus: Option<usize>,
    /// `g` was typed in the list: a second `g` goes to the top (gg, as in Neovim)
    pending_g: bool,
    /// where the buttons were drawn (x from, x to, the F-key), for the mouse
    footer_btns: Vec<(u16, u16, u8)>,
    footer_y: u16,
    /// Ctrl-c H: whether the bar of the Ctrl-c commands shows
    chord_hints: bool,
    cfg: Config,
    look: Look,
    panels: [Panel; 2],
    active: usize,
    /// the tabs: each two panels, the active one and the quick view; the current tab lives in `panels`
    /// / `active` / `quick`, its slot here is None
    tabs: Vec<Option<TabState>>,
    /// A tab's own name (Ctrl-c ,), by the tab's place; None = the name of the active panel's directory.
    tab_names: Vec<Option<String>>,
    /// What the last save was for (tabs, their names and directories, the current one): saved again when it changes.
    saved_sig: String,
    tab: usize,
    /// where the tab names were drawn on the title bar (x from, x to, tab), for the mouse
    tab_btns: std::cell::RefCell<Vec<(u16, u16, usize)>>,
    /// F2: the preview takes the place of the inactive panel.
    quick: bool,
    /// for the other instances: this one's socket, and what it answers to `status`
    _server: Option<crate::instances::Server>,
    status: std::sync::Arc<std::sync::Mutex<String>>,
    /// the columns beside the names (Ctrl-c v); remembered with the quick view
    cols: crate::config::Cols,
    /// how many entries a directory holds, for the size column: path → (its mtime, the count)
    dir_counts: DirCounts,
    dialog: Option<Dialog>,
    job: Option<RunningJob>,
    toast: Option<Toast>,
    help: bool,
    /// The help's selected row and its scroll.
    help_sel: usize,
    help_off: usize,
    /// The terminal's height at the last frame (the help's page size).
    term_rows: usize,
    loader: Loader,
    nvim: Option<Nvim>,
    nvim_failed: bool,
    /// The file the preview shows, and its scroll.
    pv_path: Option<String>,
    /// media previews (pictures + lines), made off the UI thread; the same component lfz uses
    /// Draws the plugins' answers (and keeps the picture on the terminal): the component lfz uses too.
    drawer: lvim_fuzzy_ui::provided::Drawer,
    /// The position in a multi-page file (a PDF page, a video step) the plugin is asked for; 0 = the first.
    plug_skip: u32,
    /// The plugin host the open plugin is asked through (Enter on a file); made at the first Enter.
    opener: Option<lvim_fuzzy_plugin::Previewer>,
    /// the shown directories, watched for changes made by other programs (config `watch`)
    watcher: Option<crate::watch::Watcher>,
    /// The file last asked of Neovim (asked once, painted later).
    pv_path_sent: Option<String>,
    pv_off: usize,
    wake_tx: Sender<()>,
    wake_rx: Receiver<()>,
    /// Where each panel's rows are drawn (for the mouse): (x, y, w, h).
    list_rects: [Rect; 2],
    last_click: Option<(Instant, usize, usize)>,
    quit: bool,
    /// The user actions (config `[actions]` + plugins) and previewers.
    actions: Vec<crate::plugins::Action>,
    previewers: Vec<crate::plugins::Previewer>,
    /// Background actions finishing.
    act_tx: Sender<ActionDone>,
    act_rx: Receiver<ActionDone>,
    /// Previewer outputs by (previewer, path).
    pv_out: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, Vec<String>>>>,
    /// The query field has the keys (Ctrl-f / `/`); otherwise letters are commands (j / k ...).
    searching: bool,
    /// What Ctrl-z takes back, newest last (one entry per operation).
    undo: Vec<Vec<ops::Undo>>,
    /// Directories visited, newest first (Alt-b).
    recent: Vec<PathBuf>,
    /// The plugin host: starts plugins on their first call, collects their answers (lvim-plugin).
    host: lvim_fuzzy_plugin::Host,
    /// The preview plugins (None = none installed), the request in flight and the answers so far, by file.
    plug: Option<lvim_fuzzy_plugin::Previewer>,
    plug_wait: Option<(u64, PathBuf, u32)>,
    plug_answers: std::collections::HashMap<(PathBuf, u32), lvim_fuzzy_ui::provided::Answer>,
}

/// What the preview plugins made of a file (see `App::draw_plugin_preview`).
enum PluginShow {
    /// Drawn (or asked and waiting).
    Drawn,
    /// The plugin's configuration says Neovim shows this type.
    Nvim,
    /// ... the tool's own highlighter.
    Builtin,
    /// No plugin has a preview for it: the built-in providers go on.
    NotMine,
}

const SPIN: [&str; 4] = ["◐", "◓", "◑", "◒"];

impl App {
    pub fn new(
        cfg: Config,
        look: Look,
        left: PathBuf,
        right: PathBuf,
        notes: Vec<String>,
        actions: Vec<crate::plugins::Action>,
        previewers: Vec<crate::plugins::Previewer>,
        plugin_defs: Vec<crate::plugins::Plugin>,
    ) -> App {
        let (wake_tx, wake_rx) = channel();
        let (act_tx, act_rx) = channel();
        let mut notes = notes;
        let mut host = lvim_fuzzy_plugin::Host::new(plugin_defs, wake_tx.clone());
        // every plugin's settings: its defaults, the shared file, then lvc's own [plugins.<name>]
        for e in host.configure_all(&crate::config::plugins_dir(), &cfg.plugins) {
            notes.push(e);
        }
        let plug = {
            let p = lvim_fuzzy_plugin::Previewer::load(&crate::config::plugin_roots(), "lvim-fuzzy-commander", &cfg.plugins, wake_tx.clone());
            (!p.is_empty()).then_some(p)
        };
        let hidden = cfg.hidden;
        let mut app = App {
            loader: Loader::new(wake_tx.clone()),
            panels: [Panel::new(left, hidden), Panel::new(right, hidden)],
            _server: None,
            status: Default::default(),
            tabs: vec![None],
            tab_names: vec![None],
            saved_sig: String::new(),
            tab: 0,
            tab_btns: Default::default(),
            active: 0,
            // the quick view as it was left (remembered across runs)
            quick: crate::config::load_state().quick,
            cols: crate::config::load_state().cols,
            dir_counts: Default::default(),
            dialog: None,
            job: None,
            toast: None,
            help: false,
            help_sel: 0,
            help_off: 0,
            term_rows: 24,
            nvim: None,
            nvim_failed: cfg.preview != PreviewKind::Nvim,
            pv_path: None,
            drawer: lvim_fuzzy_ui::provided::Drawer::new(cfg.image_transfer_file),
            plug_skip: 0,
            opener: None,
            watcher: if cfg.watch { crate::watch::Watcher::new() } else { None },
            pv_path_sent: None,
            pv_off: 0,
            wake_tx,
            wake_rx,
            list_rects: [Rect { x: 0, y: 0, w: 0, h: 0 }; 2],
            last_click: None,
            quit: false,
            actions,
            previewers,
            act_tx,
            act_rx,
            host,
            plug,
            plug_wait: None,
            plug_answers: Default::default(),
            pv_out: Default::default(),
            searching: false,
            chord: false,
            fkeys: crate::config::FKEY_ACTIONS
                .iter()
                .map(|(a, def)| {
                    let canon = crate::plugins::KeySpec::parse(def).expect("default key");
                    let key = cfg.keys.iter().find(|(n, _)| n == a).map(|(_, k)| k.clone()).unwrap_or_else(|| canon.clone());
                    (*a, key, KeyEvent::new(canon.code, canon.mods))
                })
                .collect(),
            cmdkeys: crate::config::COMMANDS
                .iter()
                .map(|(n, def)| {
                    let canon = crate::plugins::KeySpec::parse(def).expect("default key");
                    let key = cfg.keys.iter().find(|(k, _)| k == n).map(|(_, k)| k.clone()).unwrap_or_else(|| canon.clone());
                    (*n, key, canon)
                })
                .collect(),
            leader_label: key_label(&cfg.leader.text),
            leader_name: key_label(&cfg.leader.text).replace("C-", "Ctrl-").replace("A-", "Alt-"),
            leader: cfg.leader.clone(),
            synthetic: false,
            footer_focus: None,
            pending_g: false,
            footer_btns: Vec::new(),
            footer_y: 0,
            chord_hints: cfg.key_hints,
            undo: Vec::new(),
            recent: Vec::new(),
            cfg,
            look,
        };
        if !notes.is_empty() {
            app.dialog = Some(Dialog::Message {
                title: "Config".into(),
                lines: notes,
            });
        }
        app
    }

    fn p(&self) -> &Panel {
        &self.panels[self.active]
    }

    fn pm(&mut self) -> &mut Panel {
        &mut self.panels[self.active]
    }

    fn other(&self) -> &Panel {
        &self.panels[1 - self.active]
    }

    fn toast(&mut self, text: impl Into<String>, error: bool) {
        self.toast = Some(Toast {
            text: text.into(),
            error,
            until: Instant::now() + Duration::from_secs(if error { 6 } else { 3 }),
        });
    }

    // ── the loop ────────────────────────────────────────────────────────────────────────────────────

    pub fn run(mut self, term: &mut Term) -> std::io::Result<Outcome> {
        // listen for the other instances: their lines arrive as a control action's
        let (tx, wake) = (self.act_tx.clone(), self.wake_tx.clone());
        self._server = crate::instances::serve(self.status.clone(), move |lines| {
            let _ = tx.send(ActionDone {
                title: "lvim-fuzzy-commander".into(),
                lines,
                control: true,
                reload: false,
                ok: true,
                err: String::new(),
            });
            let _ = wake.send(());
        });
        // keys and background work arrive on one channel: the loop sleeps until either, and wakes at once
        let (ev_tx, ev_rx) = channel::<Event>();
        crate::term::spawn_input(ev_tx, self.wake_tx.clone());
        if let Some(w) = &self.watcher {
            w.notify_on(self.wake_tx.clone());
        }
        // the directories watched from the start, not after the first wait
        self.pump_watch();
        loop {
            {
                let dir = |p: &Panel| p.dir().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
                *self.status.lock().unwrap() =
                    format!("{}\t{}\t{}\t{}", std::process::id(), dir(&self.panels[0]), dir(&self.panels[1]), self.active);
            }
            self.pump_job();
            let cv = self.draw(term.w, term.h);
            term.draw(cv)?;
            if self.quit {
                // the way it is left is the way it opens next time
                self.save_state();
                break;
            }
            self.save_state_if_changed();
            // sleep until a key or a wake-up (a preview made, an archive listed, Neovim painted, a change
            // on disk, an action done): both come on the wake channel, so nothing waits for a tick. The
            // timeouts are only for what has no wake-up of its own: the 30 ms before a picture goes out,
            // a job's progress, a message's expiry.
            let timeout = if self.drawer.take_wait() {
                Duration::from_millis(10)
            } else if self.job.is_some() {
                Duration::from_millis(80)
            } else {
                Duration::from_millis(500)
            };
            let _ = self.wake_rx.recv_timeout(timeout);
            while self.wake_rx.try_recv().is_ok() {}
            while let Ok(ev) = ev_rx.try_recv() {
                self.handle(ev, term)?;
            }
            self.pump_watch();
            while let Ok(done) = self.act_rx.try_recv() {
                self.action_done(done);
            }
            self.pump_procs();
            self.note_recent();
            if let Some(t) = &self.toast
                && Instant::now() > t.until
            {
                self.toast = None;
            }
        }
        if let Some(n) = self.nvim.take() {
            n.quit();
        }
        self.stop_procs();
        Ok(Outcome {
            dir: self.p().dir().map(Path::to_path_buf),
        })
    }

    /// What other programs changed: watch the directories shown now (the panels', the quick view's, the
    /// trash's for its view) and reload a panel whose directory changed. A panel whose directory is gone
    /// goes to the nearest one that still exists.
    fn pump_watch(&mut self) {
        if self.watcher.is_none() {
            return;
        }
        let mut dirs: Vec<PathBuf> = Vec::new();
        for p in &self.panels {
            match p.dir() {
                Some(d) => dirs.push(d.to_path_buf()),
                None if p.is_trash() => {
                    // each trash's info/ changes with every item trashed, restored or purged
                    for (t, _) in crate::trash::trash_dirs() {
                        dirs.push(t.join("info"));
                    }
                }
                None => {}
            }
        }
        if self.quick
            && let Some(cur) = self.p().current_path()
            && cur.is_dir()
        {
            dirs.push(cur);
        }
        dirs.sort();
        dirs.dedup();
        let Some(w) = self.watcher.as_mut() else {
            return;
        };
        let changed = w.changed();
        w.set(&dirs);
        if changed.is_empty() {
            return;
        }
        for p in &mut self.panels {
            let hit = match p.dir() {
                Some(d) => changed.iter().any(|c| c == d),
                None => p.is_trash(),
            };
            if !hit {
                continue;
            }
            if let Some(d) = p.dir().map(Path::to_path_buf)
                && !d.is_dir()
            {
                let mut up = d.as_path();
                while let Some(parent) = up.parent() {
                    up = parent;
                    if up.is_dir() {
                        break;
                    }
                }
                p.go(Place::Dir(up.to_path_buf()));
            } else {
                p.reload();
            }
        }
    }

    /// Remember the panels' directories (newest first, at most 30) for Alt-b.
    fn note_recent(&mut self) {
        for i in 0..2 {
            if let Some(d) = self.panels[i].dir().map(Path::to_path_buf) {
                if self.recent.first() != Some(&d) {
                    self.recent.retain(|p| p != &d);
                    self.recent.insert(0, d);
                    self.recent.truncate(30);
                }
            }
        }
    }

    fn handle(&mut self, ev: Event, term: &mut Term) -> std::io::Result<()> {
        match ev {
            Event::Resize(w, h) => {
                term.resize(w, h);
                // a font change resizes the cells: the images are drawn again at the new size
                lvim_fuzzy_ui::iconimg::resized();
            }
            Event::Key(k) if k.kind != KeyEventKind::Release => {
                // LVC_KEYLOG=FILE: every key as it arrives (for finding out why a key does nothing)
                if let Ok(path) = std::env::var("LVC_KEYLOG")
                    && let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path)
                {
                    use std::io::Write;
                    let _ = writeln!(f, "{:?} {:?}", k.code, k.modifiers);
                }
                self.key(k, term)?;
            }
            Event::Mouse(m) => self.mouse(m, term)?,
            _ => {}
        }
        Ok(())
    }

    // ── jobs ────────────────────────────────────────────────────────────────────────────────────────

    fn start_job(&mut self, kind: JobKind, sources: Vec<PathBuf>, dest: Option<PathBuf>) {
        if sources.is_empty() {
            return;
        }
        if self.job.is_some() {
            self.toast("another operation is running", true);
            return;
        }
        self.job = Some(RunningJob {
            job: wake_on(ops::start(kind, sources, dest), self.wake_tx.clone()),
            done: 0,
            total: 0,
            file: String::new(),
            started: Instant::now(),
        });
    }

    fn pump_job(&mut self) {
        let Some(rj) = self.job.as_mut() else { return };
        loop {
            match rj.job.rx.try_recv() {
                Ok(Report::Progress { done, total, file }) => {
                    rj.done = done;
                    rj.total = total;
                    rj.file = file;
                }
                Ok(Report::Ask { target, answer }) => {
                    self.dialog = Some(Dialog::Conflict { target, answer });
                    return;
                }
                Ok(Report::Done { count, errors, undo }) => {
                    let kind = rj.job.kind;
                    self.job = None;
                    if !undo.is_empty() {
                        self.undo.push(undo);
                    }
                    for p in &mut self.panels {
                        p.marked.clear();
                        p.reload();
                    }
                    if errors.is_empty() {
                        self.toast(format!("{}: {count} done", kind.label()), false);
                    } else {
                        self.dialog = Some(Dialog::Message {
                            title: format!("{}: {count} done, {} failed", kind.label(), errors.len()),
                            lines: errors,
                        });
                    }
                    return;
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    self.job = None;
                    return;
                }
            }
        }
    }

    // ── keys ────────────────────────────────────────────────────────────────────────────────────────

    fn key(&mut self, mut k: KeyEvent, term: &mut Term) -> std::io::Result<()> {
        // the quick view's directory listing is read again after any key (an operation may change it)
        // Shift + a letter as some terminals and multiplexers send it (zellij, the kitty protocol): the
        // letter in lower case with SHIFT. As the upper-case letter, so `H`, `S`, `O`, `G`, `J` work everywhere.
        if let KeyCode::Char(c) = k.code
            && k.modifiers.contains(KeyModifiers::SHIFT)
            && c.is_ascii_lowercase()
        {
            k.code = KeyCode::Char(c.to_ascii_uppercase());
        }
        if self.dialog.is_some() {
            self.dialog_key(k, term)?;
            return Ok(());
        }
        if self.footer_focus.is_some() && !self.help {
            return self.footer_key(k, term);
        }
        // Ctrl-j: down onto the button bar; Ctrl-k from the panels: up, out of lvc (Zellij)
        if k.code == KeyCode::Char('j') && k.modifiers.contains(KeyModifiers::CONTROL) && !self.help {
            self.footer_focus = Some(0);
            self.chord = false;
            return Ok(());
        }
        if k.code == KeyCode::Char('k') && k.modifiers.contains(KeyModifiers::CONTROL) && !self.help {
            self.chord = false;
            crate::mux::move_focus("up");
            return Ok(());
        }
        if self.help {
            // the help is a list: j / k (arrows, PgUp / PgDn, g / G) move, Esc / q / F1 / Enter close
            let n = self.help_items().len();
            let page = self.term_rows.saturating_sub(6).max(1);
            match k.code {
                KeyCode::Char('j') | KeyCode::Down => self.help_sel = (self.help_sel + 1).min(n.saturating_sub(1)),
                KeyCode::Char('k') | KeyCode::Up => self.help_sel = self.help_sel.saturating_sub(1),
                KeyCode::PageDown | KeyCode::Char('d') => self.help_sel = (self.help_sel + page).min(n.saturating_sub(1)),
                KeyCode::PageUp | KeyCode::Char('u') => self.help_sel = self.help_sel.saturating_sub(page),
                KeyCode::Char('g') | KeyCode::Home => self.help_sel = 0,
                KeyCode::Char('G') | KeyCode::End => self.help_sel = n.saturating_sub(1),
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::F(1) | KeyCode::Enter => self.help = false,
                _ => {}
            }
            return Ok(());
        }
        let page = self.list_rects[self.active].h.max(1) as isize;
        // an operation runs: Esc cancels it
        if let Some(rj) = &self.job
            && k.code == KeyCode::Esc
        {
            rj.job.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            return Ok(());
        }
        // Ctrl-c starts a command: the next key says which (Esc / Ctrl-c again: none)
        if self.leader.matches(&k, false) && !self.chord {
            self.chord = true;
            return Ok(());
        }
        let chord = std::mem::take(&mut self.chord);
        if chord && (k.code == KeyCode::Esc || self.leader.matches(&k, false)) {
            return Ok(());
        }
        // [keys]: a key given to an F-key action becomes that action's own F-key; an F-key whose action
        // moved elsewhere does nothing (a press of a bar button is already the action's own key)
        let mut k = k;
        let mut chord = chord;
        let was_synthetic = std::mem::take(&mut self.synthetic);
        if !was_synthetic {
            if let Some((_, _, canon)) = self.fkeys.iter().find(|(_, ks, _)| ks.matches(&k, chord)) {
                k = *canon;
                chord = false;
            } else if !chord
                && matches!(k.code, KeyCode::F(1..=10))
                && self.fkeys.iter().any(|(_, ks, canon)| {
                    // this F-key (Shift or not) belongs to an action that now sits on another key
                    canon.code == k.code
                        && canon.modifiers == (k.modifiers & KeyModifiers::SHIFT)
                        && (ks.chord || ks.code != canon.code || ks.mods != canon.modifiers)
                })
            {
                return Ok(());
            }
        }
        // [keys] for the other commands: a key given to one becomes the command's own key (the leader chord
        // or a plain key); a default key whose commands all moved does nothing. Letters typed into the
        // query field are not commands.
        let typing = self.searching && !chord && !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) && matches!(k.code, KeyCode::Char(_));
        if !typing && !was_synthetic {
            if let Some((_, _, canon)) = self.cmdkeys.iter().find(|(_, u, c)| u != c && u.matches(&k, chord)) {
                k = KeyEvent::new(canon.code, canon.mods);
                chord = canon.chord;
            } else {
                let on: Vec<_> = self.cmdkeys.iter().filter(|(_, _, c)| c.matches(&k, chord)).collect();
                if !on.is_empty() && on.iter().all(|(_, u, c)| u != c) {
                    if chord {
                        self.toast(format!("{}: that command is on another key (F1)", self.leader_name), true);
                    }
                    return Ok(());
                }
            }
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT) && !chord;
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        // Ctrl-c <digit>: the F-key, for a terminal (or multiplexer) that keeps the F-keys
        let fkey = |n: u8| {
            k.code == KeyCode::F(n) || (chord && k.code == KeyCode::Char(if n == 10 { '0' } else { (b'0' + n) as char }))
        };
        // Ctrl-h / Ctrl-l: the left / right panel (Tab: the other one)
        // past the edge (Ctrl-h on the left panel, Ctrl-l on the right): the neighbouring Zellij pane
        if ctrl && !chord && matches!(k.code, KeyCode::Char('h') | KeyCode::Char('l')) {
            let want = if k.code == KeyCode::Char('h') { 0 } else { 1 };
            if self.active == want && crate::mux::move_focus(if want == 0 { "left" } else { "right" }) {
                return Ok(());
            }
            self.active = want;
            self.pv_off = 0;
            return Ok(());
        }
        // Ctrl-f: into (or out of) the query field
        if ctrl && k.code == KeyCode::Char('f') {
            self.searching = !self.searching;
            return Ok(());
        }
        // in the query field: typing filters; Enter / Esc go back to the list (Esc first clears)
        if self.searching && !chord {
            match k.code {
                KeyCode::Char(c) if !ctrl && !alt => {
                    let p = self.pm();
                    p.query.push(c);
                    p.refilter();
                    p.cursor = 0;
                    self.pv_follow();
                    return Ok(());
                }
                KeyCode::Backspace => {
                    let p = self.pm();
                    p.query.pop();
                    p.refilter();
                    self.pv_follow();
                    return Ok(());
                }
                KeyCode::Enter => {
                    self.searching = false;
                    return Ok(());
                }
                KeyCode::Esc => {
                    let p = self.pm();
                    if p.query.is_empty() {
                        self.searching = false;
                    } else {
                        p.query.clear();
                        p.refilter();
                    }
                    return Ok(());
                }
                KeyCode::Tab => self.searching = false,
                _ => {}
            }
        }
        // a user action's key (config [actions] / a plugin): before the built-ins, so it may replace one
        if let Some(i) = self
            .actions
            .iter()
            .position(|a| a.key.as_ref().is_some_and(|ks| ks.matches(&k, chord)))
        {
            self.run_action(i, term)?;
            return Ok(());
        }
        // in the list: vi letters move (j / k / g / G, h / l out of / into a directory), Space marks
        let gg = std::mem::take(&mut self.pending_g);
        if !self.searching && !ctrl && !alt && !chord {
            let handled = match k.code {
                KeyCode::Char('j') => {
                    self.pm().move_by(1);
                    true
                }
                KeyCode::Char('k') => {
                    self.pm().move_by(-1);
                    true
                }
                // gg: the top of this panel (one g waits for the second)
                KeyCode::Char('g') if gg => {
                    self.pm().cursor = 0;
                    true
                }
                // gt / gT: the next / previous tab; g1 … g9: that tab
                KeyCode::Char('t') if gg => {
                    let n = self.tabs.len();
                    self.go_tab((self.tab + 1) % n);
                    true
                }
                KeyCode::Char('T') if gg => {
                    let n = self.tabs.len();
                    self.go_tab((self.tab + n - 1) % n);
                    true
                }
                KeyCode::Char(c @ '1'..='9') if gg => {
                    self.go_tab(c as usize - '1' as usize);
                    true
                }
                KeyCode::Char('g') => {
                    self.pending_g = true;
                    true
                }
                KeyCode::Char('G') => {
                    let n = self.p().shown.len();
                    self.pm().cursor = n.saturating_sub(1);
                    true
                }
                KeyCode::Char('h') => {
                    self.pm().up();
                    true
                }
                KeyCode::Char('l') => {
                    self.pm().enter();
                    true
                }
                KeyCode::Char(' ') => {
                    self.pm().toggle_mark();
                    self.pm().move_by(1);
                    true
                }
                KeyCode::Char('/') => {
                    self.searching = true;
                    true
                }
                KeyCode::Char('.') => {
                    let p = self.pm();
                    p.hidden = !p.hidden;
                    p.reload();
                    true
                }
                // the quick view scrolls
                // J / K: in a media preview the next / previous page or point of a video, else scroll
                KeyCode::Char('J') => {
                    if !self.seek(1) {
                        self.scroll_preview(3, false);
                    }
                    true
                }
                KeyCode::Char('K') => {
                    if !self.seek(-1) {
                        self.scroll_preview(-3, false);
                    }
                    true
                }
                _ => false,
            };
            if handled {
                self.pv_follow();
                return Ok(());
            }
        }
        match k.code {
            KeyCode::Tab | KeyCode::BackTab => {
                self.active = 1 - self.active;
                self.pv_off = 0;
            }
            KeyCode::Up => self.pm().move_by(-1),
            KeyCode::Down => self.pm().move_by(1),
            // Ctrl-d / Ctrl-u: half a page — of the quick view when it is open, else of the list
            // Ctrl-e / Ctrl-y: the quick view one line, as in Neovim
            KeyCode::Char('e') if ctrl && self.quick => self.scroll_preview(1, false),
            KeyCode::Char('y') if ctrl && self.quick => self.scroll_preview(-1, false),
            KeyCode::Char('d') if ctrl && self.quick => self.scroll_preview(1, true),
            KeyCode::Char('u') if ctrl && self.quick => self.scroll_preview(-1, true),
            KeyCode::Char('d') if ctrl => self.pm().move_by((page / 2).max(1)),
            KeyCode::Char('u') if ctrl => self.pm().move_by(-(page / 2).max(1)),
            KeyCode::PageUp => self.pm().move_by(-page),
            KeyCode::PageDown => self.pm().move_by(page),
            KeyCode::Home => self.pm().cursor = 0,
            KeyCode::End => {
                let n = self.p().shown.len();
                self.pm().cursor = n.saturating_sub(1);
            }
            KeyCode::Enter => self.open_current(term)?,
            KeyCode::Left | KeyCode::Char('[') if chord => self.pm().back_in_history(),
            KeyCode::Right | KeyCode::Char(']') if chord => self.pm().forward_in_history(),
            KeyCode::Right => {
                self.pm().enter();
            }
            KeyCode::Left => self.pm().up(),
            KeyCode::Backspace => self.pm().up(),
            KeyCode::Esc => {
                let p = self.pm();
                if !p.query.is_empty() {
                    p.query.clear();
                    p.refilter();
                } else {
                    p.marked.clear();
                }
            }
            KeyCode::Insert => {
                self.pm().toggle_mark();
                self.pm().move_by(1);
            }
            KeyCode::Char('a') if chord => {
                let names: Vec<String> = {
                    let p = self.p();
                    p.shown
                        .iter()
                        .map(|&i| &p.entries[i])
                        .filter(|e| !e.is_parent())
                        .map(|e| e.name.clone())
                        .collect()
                };
                self.pm().marked.extend(names);
            }
            KeyCode::Char('+') if chord => self.input("Mark (glob)", "*", InputAction::Mark(true)),
            KeyCode::Char('-') if chord => self.input("Unmark (glob)", "*", InputAction::Mark(false)),
            KeyCode::Char('*') if chord => self.pm().invert_marks(),
            KeyCode::Char('h') if chord => {
                let p = self.pm();
                p.hidden = !p.hidden;
                p.reload();
            }
            KeyCode::Char('l') if chord => self.toggle_long(),
            KeyCode::Char('s') if chord => {
                let p = self.pm();
                p.sort = p.sort.next();
                p.reload();
            }
            KeyCode::Char('S') if chord => {
                let p = self.pm();
                p.reverse = !p.reverse;
                p.reload();
            }
            KeyCode::Char('H') if chord => {
                self.chord_hints = !self.chord_hints;
                let n = self.leader_name.clone();
                self.toast(if self.chord_hints { format!("{n} hints on") } else { format!("{n} hints off ({n} H)") }, false);
            }
            KeyCode::Char('v') if chord => self.dialog = Some(Dialog::Columns { sel: 0 }),
            KeyCode::Char('n') if chord => self.new_tab(),
            KeyCode::Char('I') if chord => self.instances_menu(),
            KeyCode::Char('w') if chord => self.close_tab(),
            KeyCode::Char(',') if chord => self.rename_tab(),
            KeyCode::Char('<') if chord => self.move_tab(-1),
            KeyCode::Char('>') if chord => self.move_tab(1),
            KeyCode::Char('y') if chord => self.copy_paths(term, PathPart::Full)?,
            KeyCode::Char('Y') if chord => self.copy_paths(term, PathPart::Name)?,
            KeyCode::Char('D') if chord => self.copy_paths(term, PathPart::Dir)?,
            KeyCode::Char('L') if chord => self.symlink(),
            KeyCode::Char('p') if chord => {
                self.panels.swap(0, 1);
            }
            KeyCode::Char('o') if chord => {
                // the directory under the cursor (or this one) in the other panel
                let target = match self.p().current() {
                    Some(e) if e.is_dir() && !e.is_parent() => self.p().current_path(),
                    _ => self.p().dir().map(Path::to_path_buf),
                };
                if let Some(t) = target {
                    let o = 1 - self.active;
                    self.panels[o].go(Place::Dir(t));
                }
            }
            KeyCode::Char('i') if chord => {
                if let Some(d) = self.p().dir().map(Path::to_path_buf) {
                    let o = 1 - self.active;
                    self.panels[o].go(Place::Dir(d));
                }
            }
            KeyCode::Char('r') if chord && self.p().is_trash() => self.restore(),
            KeyCode::Char('e') if chord && self.p().is_trash() => {
                self.dialog = Some(Dialog::Confirm {
                    title: "Empty the trash".into(),
                    lines: vec![format!(
                        "Delete all {} item(s) in the trash for good?",
                        crate::trash::list().len()
                    )],
                    action: ConfirmAction::EmptyTrash,
                });
            }
            KeyCode::Char('r') if chord => {
                for p in &mut self.panels {
                    p.reload();
                }
            }
            KeyCode::Char('t') if chord => {
                if self.p().is_trash() {
                    self.pm().back_in_history();
                } else {
                    self.pm().go(Place::Trash);
                }
            }
            KeyCode::Char('f') if chord => self.find(term, false)?,
            KeyCode::Char('g') if chord => self.find(term, true)?,
            KeyCode::Char('c') if chord => {
                let d = self.p().dir().map(fsx::tilde).unwrap_or_default();
                self.input("Go to directory", &d, InputAction::Goto)
            }
            KeyCode::Char('x') if chord => self.input("Shell command", "", InputAction::Shell),
            KeyCode::Char('u') if chord => self.undo_last(),
            KeyCode::Char('m') if chord => self.chmod(),
            KeyCode::Char('O') if chord => self.chown(),
            KeyCode::Char('b') if chord => self.bookmarks(),
            KeyCode::Char('e') if chord => self.extract(),
            KeyCode::Char('z') if chord => self.compress(),
            KeyCode::Char('q') if chord || ctrl => self.quit = true,
            KeyCode::F(10) => self.quit = true,
            KeyCode::F(1) => {
                self.help = true;
                self.help_sel = 0;
                self.help_off = 0;
            }
            KeyCode::Char('?') if chord => {
                self.help = true;
                self.help_sel = 0;
                self.help_off = 0;
            }
            _ if fkey(9) => self.action_menu(),
            _ if fkey(2) => {
                self.toggle_quick();
            }
            _ if fkey(6) => self.edit_current(term)?,
            _ if fkey(3) => self.copy_move(false),
            // in the trash F5 (move) moves back: restore
            _ if fkey(5) && self.p().is_trash() => self.restore(),
            _ if fkey(5) => self.copy_move(true),
            _ if fkey(4) => {
                if self.p().marked.len() > 1 {
                    self.bulk_rename(term)?;
                } else {
                    self.rename();
                }
            }
            _ if fkey(7) => self.new_file(),
            // F8 / Delete: as the config says (the trash); with Shift: for good
            _ if fkey(8) || k.code == KeyCode::Delete => {
                let direct = (shift && k.code == KeyCode::F(8)) || self.cfg.delete == Delete::Direct;
                self.delete(direct);
            }
            _ if chord => {
                let n = self.leader_name.clone();
                self.toast(format!("{n}: no such command (F1 lists them)"), true)
            }
            _ => {}
        }
        self.pv_follow();
        Ok(())
    }

    /// Everything lvc remembers between runs: the quick view, the columns, and the tabs as they are — their order and
    /// names, both directories of each with the entry under the cursor, the active panel, the quick view.
    fn state(&self) -> crate::config::State {
        use crate::config::{PanelSave, TabSave};
        let panel = |p: &Panel| PanelSave {
            // a panel in the trash has no directory: home takes its place
            dir: p.dir().map(Path::to_path_buf).unwrap_or_else(fsx::home),
            cursor: p.current().map(|e| e.name.clone()).filter(|n| n != "..").unwrap_or_default(),
        };
        let tabs = (0..self.tabs.len())
            .map(|i| {
                let (panels, active, quick) = match &self.tabs[i] {
                    Some(t) => (&t.panels, t.active, t.quick),
                    None => (&self.panels, self.active, self.quick),
                };
                TabSave { name: self.tab_names[i].clone(), panels: [panel(&panels[0]), panel(&panels[1])], active, quick }
            })
            .collect();
        crate::config::State { quick: self.quick, cols: self.cols, tabs, tab: self.tab }
    }

    fn save_state(&mut self) {
        let st = self.state();
        self.saved_sig = Self::sig(&st);
        crate::config::save_state(&st);
    }

    /// What decides whether the state is saved again: everything but the cursors (those are written with the rest).
    fn sig(st: &crate::config::State) -> String {
        let mut s = format!("{}|{}|{:?}", st.quick, st.tab, st.cols);
        for t in &st.tabs {
            s += &format!("|{:?}/{}/{}/{}/{}", t.name, t.panels[0].dir.display(), t.panels[1].dir.display(), t.active, t.quick);
        }
        s
    }

    /// Save when the tabs, a directory or the current tab changed since the last save.
    fn save_state_if_changed(&mut self) {
        if Self::sig(&self.state()) != self.saved_sig {
            self.save_state();
        }
    }

    /// Open the tabs as they were left (started with no directory given): each with its two directories and the
    /// entry the cursor was on, its name, its quick view, the active panel; the tab that was current is current.
    pub fn restore_tabs(&mut self, saved: &crate::config::State) {
        if saved.tabs.is_empty() {
            return;
        }
        let hidden = self.p().hidden;
        let make = |ps: &crate::config::PanelSave| {
            let dir = if ps.dir.is_dir() { ps.dir.clone() } else { fsx::home() };
            let mut p = Panel::new(dir, hidden);
            if !ps.cursor.is_empty() {
                p.select_name(&ps.cursor);
            }
            p
        };
        let current = saved.tab.min(saved.tabs.len() - 1);
        let (mut slots, mut names) = (Vec::new(), Vec::new());
        for (i, t) in saved.tabs.iter().enumerate() {
            let panels = [make(&t.panels[0]), make(&t.panels[1])];
            names.push(t.name.clone());
            if i == current {
                self.panels = panels;
                self.active = t.active;
                self.quick = t.quick;
                slots.push(None);
            } else {
                slots.push(Some(TabState { panels, active: t.active, quick: t.quick }));
            }
        }
        self.tabs = slots;
        self.tab_names = names;
        self.tab = current;
    }

    /// Ctrl-c l: permissions, owner and group together on / off (the "long" listing).
    fn toggle_long(&mut self) {
        let on = !(self.cols.mode && self.cols.owner && self.cols.group);
        self.cols.mode = on;
        self.cols.owner = on;
        self.cols.group = on;
        self.save_state();
    }

    /// Ctrl-c y / Y / D: the paths of the marked items (or the one under the cursor), their names, or the
    /// panel's directory — into the clipboard: OSC 52 (kitty, zellij, over ssh) and wl-copy / xclip.
    fn copy_paths(&mut self, term: &mut Term, part: PathPart) -> std::io::Result<()> {
        let text = match part {
            PathPart::Dir => self.p().dir().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default(),
            _ => {
                let t = self.p().targets();
                t.iter()
                    .map(|p| match part {
                        PathPart::Name => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                        _ => p.to_string_lossy().into_owned(),
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        };
        if text.is_empty() {
            return Ok(());
        }
        term.write_raw(format!("\x1b]52;c;{}\x07", lvim_fuzzy_ui::util::base64(text.as_bytes())).as_bytes())?;
        clipboard_tools(&text);
        let n = text.lines().count();
        let shown = if n == 1 { text.clone() } else { format!("{n} paths") };
        self.toast(format!("copied: {shown}"), false);
        Ok(())
    }

    /// Ctrl-c L: symbolic links to the marked items (or the one under the cursor) in the other panel's
    /// directory; Tab in the dialog: absolute / relative.
    fn symlink(&mut self) {
        if self.p().is_trash() {
            return;
        }
        let targets = self.p().targets();
        let Some(dir) = self.other().dir().map(Path::to_path_buf) else {
            return;
        };
        if targets.is_empty() {
            return;
        }
        let name = if targets.len() == 1 {
            targets[0].file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
        } else {
            String::new()
        };
        let title = symlink_title(targets.len(), false);
        self.input(&title, &name, InputAction::Symlink { targets, dir, relative: false });
    }

    /// Go to tab `i` (the current one is put away in its slot).
    fn go_tab(&mut self, i: usize) {
        if i == self.tab || i >= self.tabs.len() {
            return;
        }
        let Some(t) = self.tabs[i].take() else { return };
        let old = TabState {
            panels: std::mem::replace(&mut self.panels, t.panels),
            active: std::mem::replace(&mut self.active, t.active),
            quick: std::mem::replace(&mut self.quick, t.quick),
        };
        self.tabs[self.tab] = Some(old);
        self.tab = i;
        self.pv_off = 0;
        for p in &mut self.panels {
            p.reload();
        }
    }

    /// Ctrl-c n: a new tab, right after this one, on the same two directories.
    fn new_tab(&mut self) {
        let hidden = self.p().hidden;
        let dir = |p: &Panel| p.dir().map(Path::to_path_buf).unwrap_or_else(fsx::home);
        let t = TabState {
            panels: [Panel::new(dir(&self.panels[0]), hidden), Panel::new(dir(&self.panels[1]), hidden)],
            active: self.active,
            quick: self.quick,
        };
        self.tabs.insert(self.tab + 1, Some(t));
        self.tab_names.insert(self.tab + 1, None);
        self.go_tab(self.tab + 1);
    }

    /// Ctrl-c w: close this tab (the last one stays).
    fn close_tab(&mut self) {
        if self.tabs.len() < 2 {
            self.toast("the last tab stays (F10 quits)", false);
            return;
        }
        let closing = self.tab;
        let next = if closing + 1 < self.tabs.len() { closing + 1 } else { closing - 1 };
        self.go_tab(next);
        // the closed tab is now put away in its slot: drop it; the tabs after it move one left
        self.tabs.remove(closing);
        self.tab_names.remove(closing);
        if closing < self.tab {
            self.tab -= 1;
        }
    }

    /// Close tab `i` (a middle click on its name): the current one as Ctrl-c w does, another one where it is — the
    /// current tab stays the current tab.
    fn close_tab_at(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        if i == self.tab {
            return self.close_tab();
        }
        if self.tabs.len() < 2 {
            return;
        }
        self.tabs.remove(i);
        self.tab_names.remove(i);
        if i < self.tab {
            self.tab -= 1;
        }
    }

    /// Ctrl-c ,: give this tab a name of its own (empty: its directory's name again).
    fn rename_tab(&mut self) {
        let now = self.tab_names[self.tab].clone().unwrap_or_default();
        self.input("Rename the tab (empty: the directory's name)", &now, InputAction::TabName);
    }

    /// Ctrl-c < / >: move this tab one place left / right (the other tab changes places with it).
    fn move_tab(&mut self, d: isize) {
        let to = self.tab as isize + d;
        if to < 0 || to >= self.tabs.len() as isize {
            return;
        }
        let to = to as usize;
        // the current tab lives in `panels` and its slot holds None: the slots swap, the tab number follows
        self.tabs.swap(self.tab, to);
        self.tab_names.swap(self.tab, to);
        self.tab = to;
    }

    /// F2: the quick view on / off; remembered for the next run.
    fn toggle_quick(&mut self) {
        self.quick = !self.quick;
        self.pv_off = 0;
        self.save_state();
    }

    /// J / K on a media preview with positions (PDF / document pages, a video's points): move among them.
    fn seek(&mut self, d: i32) -> bool {
        if !self.quick {
            return false;
        }
        let Some(path) = self.p().current_path() else { return false };
        let positions = match self.plug_answers.get(&(path, self.plug_skip)) {
            Some(lvim_fuzzy_ui::provided::Answer::Preview(p)) => p.positions,
            _ => 1,
        };
        if positions <= 1 {
            return false;
        }
        let to = (self.plug_skip as i32 + d).clamp(0, positions as i32 - 1) as u32;
        if to != self.plug_skip {
            self.plug_skip = to;
            self.drawer.moved();
        }
        true
    }

    /// Scroll the quick view: `n` lines, or (half) a half page in the direction of `n`. A Neovim preview
    /// gets the keys (as lfz does), the built-in one moves its offset.
    fn scroll_preview(&mut self, n: isize, half: bool) {
        if !self.quick {
            return;
        }
        let cur = self.p().current_path().map(|p| p.to_string_lossy().into_owned());
        if let Some(nv) = &self.nvim {
            let shown = nv.grid.lock().unwrap().shown.clone();
            if shown.is_some() && shown == cur && cur.as_deref().is_some_and(|c| Path::new(c).is_file()) {
                let keys = match (half, n > 0) {
                    (true, true) => "<C-d>".to_string(),
                    (true, false) => "<C-u>".to_string(),
                    (false, true) => format!("{n}<C-e>"),
                    (false, false) => format!("{}<C-y>", -n),
                };
                nv.input(&keys);
                return;
            }
        }
        let step = if half { (self.term_rows / 2).max(1) as isize * n.signum() } else { n };
        self.pv_off = (self.pv_off as isize + step).max(0) as usize;
    }

    /// A different entry under the cursor: the preview starts at its top.
    fn pv_follow(&mut self) {
        let cur = self.p().current_path().map(|p| p.to_string_lossy().into_owned());
        if cur != self.pv_path {
            self.pv_off = 0;
            // another file: its first page / point, its picture waits a moment
            self.plug_skip = 0;
            self.drawer.moved();
        }
    }

    fn input(&mut self, title: &str, value: &str, action: InputAction) {
        self.dialog = Some(Dialog::Input {
            title: title.into(),
            value: value.into(),
            cur: value.chars().count(),
            action,
        });
    }

    fn dialog_key(&mut self, k: KeyEvent, term: &mut Term) -> std::io::Result<()> {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let Some(d) = self.dialog.take() else {
            return Ok(());
        };
        match d {
            Dialog::Message { .. } => {} // any key closes it
            Dialog::Menu { title, items, mut sel } => match k.code {
                KeyCode::Esc => {}
                KeyCode::Enter => {
                    if let Some((_, it)) = items.get(sel) {
                        let it = it.clone();
                        self.menu_pick(it, term)?;
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    sel = sel.saturating_sub(1);
                    self.dialog = Some(Dialog::Menu { title, items, sel });
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    sel = (sel + 1).min(items.len().saturating_sub(1));
                    self.dialog = Some(Dialog::Menu { title, items, sel });
                }
                KeyCode::Home | KeyCode::Char('g') => {
                    self.dialog = Some(Dialog::Menu { title, items, sel: 0 });
                }
                KeyCode::End | KeyCode::Char('G') => {
                    let last = items.len().saturating_sub(1);
                    self.dialog = Some(Dialog::Menu { title, items, sel: last });
                }
                KeyCode::Char('q') => {}
                _ => self.dialog = Some(Dialog::Menu { title, items, sel }),
            },
            Dialog::Conflict { target, answer } => {
                let pick = match k.code {
                    KeyCode::Char('o') => Some((Conflict::Overwrite, false)),
                    KeyCode::Char('O') => Some((Conflict::Overwrite, true)),
                    KeyCode::Char('s') => Some((Conflict::Skip, false)),
                    KeyCode::Char('S') => Some((Conflict::Skip, true)),
                    KeyCode::Char('r') => Some((Conflict::Rename, false)),
                    KeyCode::Char('R') => Some((Conflict::Rename, true)),
                    KeyCode::Char('c') | KeyCode::Esc => Some((Conflict::Cancel, true)),
                    _ => None,
                };
                match pick {
                    Some(a) => {
                        let _ = answer.send(a);
                    }
                    None => self.dialog = Some(Dialog::Conflict { target, answer }),
                }
            }
            Dialog::Confirm { title, lines, action } => match k.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => self.confirmed(action),
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {}
                _ => self.dialog = Some(Dialog::Confirm { title, lines, action }),
            },
            Dialog::Columns { mut sel } => {
                let n = crate::config::Cols::ALL.len();
                match k.code {
                    KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => return Ok(()),
                    KeyCode::Char('j') | KeyCode::Down | KeyCode::Tab => sel = (sel + 1) % n,
                    KeyCode::Char('k') | KeyCode::Up | KeyCode::BackTab => sel = (sel + n - 1) % n,
                    KeyCode::Char(' ') | KeyCode::Char('x') => {
                        let key = crate::config::Cols::ALL[sel].0;
                        let on = !self.cols.get(key);
                        self.cols.set(key, on);
                        self.save_state();
                    }
                    _ => {}
                }
                self.dialog = Some(Dialog::Columns { sel });
            }
            Dialog::Owner { paths, users, groups, mut user, mut group, mut focus, mut open } => {
                let list = |f: usize| if f == 0 { &users } else { &groups };
                let matches = |f: usize, q: &str| -> Vec<usize> {
                    let q = q.to_lowercase();
                    list(f).iter().enumerate().filter(|(_, n)| n.to_lowercase().contains(&q)).map(|(i, _)| i).collect()
                };
                if let Some((mut q, mut sel)) = open.take() {
                    // a drop-down is open: j / k (↑ ↓, Ctrl-n / Ctrl-p) move, typing filters, Enter picks
                    let m = matches(focus, &q);
                    match k.code {
                        KeyCode::Esc => {}
                        KeyCode::Enter => {
                            if let Some(&i) = m.get(sel) {
                                if focus == 0 {
                                    user = i;
                                } else {
                                    group = i;
                                }
                            }
                        }
                        KeyCode::Down | KeyCode::Tab => {
                            sel = (sel + 1).min(m.len().saturating_sub(1));
                            open = Some((q, sel));
                        }
                        KeyCode::Char('n') | KeyCode::Char('j') if ctrl => {
                            sel = (sel + 1).min(m.len().saturating_sub(1));
                            open = Some((q, sel));
                        }
                        KeyCode::Up | KeyCode::BackTab => {
                            sel = sel.saturating_sub(1);
                            open = Some((q, sel));
                        }
                        KeyCode::Char('p') | KeyCode::Char('k') if ctrl => {
                            sel = sel.saturating_sub(1);
                            open = Some((q, sel));
                        }
                        KeyCode::Backspace => {
                            q.pop();
                            open = Some((q, 0));
                        }
                        KeyCode::Char(c) if !ctrl => {
                            q.push(c);
                            open = Some((q, 0));
                        }
                        _ => open = Some((q, sel)),
                    }
                } else {
                    match k.code {
                        KeyCode::Esc => return Ok(()),
                        KeyCode::Tab | KeyCode::Down | KeyCode::Char('j') => focus = (focus + 1) % 4,
                        KeyCode::BackTab | KeyCode::Up | KeyCode::Char('k') => focus = (focus + 3) % 4,
                        KeyCode::Right | KeyCode::Char('l') if focus == 2 => focus = 3,
                        KeyCode::Left | KeyCode::Char('h') if focus == 3 => focus = 2,
                        KeyCode::Enter | KeyCode::Char(' ') if focus < 2 => {
                            // open on the current choice
                            let cur = if focus == 0 { user } else { group };
                            open = Some((String::new(), cur));
                        }
                        KeyCode::Enter | KeyCode::Char(' ') if focus == 2 => {
                            let spec = format!("{}:{}", users[user], groups[group]);
                            return self.submit(InputAction::Chown(paths), spec, term);
                        }
                        KeyCode::Enter | KeyCode::Char(' ') => return Ok(()),
                        _ => {}
                    }
                }
                self.dialog = Some(Dialog::Owner { paths, users, groups, user, group, focus, open });
            }
            Dialog::Mode {
                paths,
                mut value,
                mut cur,
                base,
                is_dir,
                mut bits,
                mut focus,
            } => {
                let chars: Vec<char> = value.chars().collect();
                let mut edited = false;
                match k.code {
                    KeyCode::Esc => return Ok(()),
                    KeyCode::Enter => return self.submit(InputAction::Chmod(paths), value, term),
                    KeyCode::Tab => focus = (focus + 1) % 13,
                    KeyCode::BackTab => focus = (focus + 12) % 13,
                    // the ticks: a 3 × 3 grid (owner / group / others × r w x), then the special row
                    KeyCode::Down | KeyCode::Char('j') if focus > 0 || k.code == KeyCode::Down => {
                        focus = match focus {
                            0 => 1,
                            1..=6 => focus + 3,
                            7..=9 => focus + 3,
                            f => f,
                        }
                    }
                    KeyCode::Up | KeyCode::Char('k') if focus > 0 => {
                        focus = match focus {
                            1..=3 => 0,
                            f => f - 3,
                        }
                    }
                    KeyCode::Right | KeyCode::Char('l') if focus > 0 => {
                        if focus % 3 != 0 {
                            focus += 1;
                        }
                    }
                    KeyCode::Left | KeyCode::Char('h') if focus > 0 => {
                        if focus % 3 != 1 {
                            focus -= 1;
                        }
                    }
                    KeyCode::Char(' ') | KeyCode::Char('x') if focus > 0 => {
                        bits ^= MODE_BITS[focus - 1];
                        value = format!("{bits:o}");
                        cur = value.chars().count();
                    }
                    // the field: typing (the ticks follow when it reads as a mode)
                    KeyCode::Char('u') if ctrl && focus == 0 => {
                        value.clear();
                        cur = 0;
                        edited = true;
                    }
                    KeyCode::Backspace if focus == 0 && cur > 0 => {
                        let mut c = chars.clone();
                        c.remove(cur - 1);
                        cur -= 1;
                        value = c.into_iter().collect();
                        edited = true;
                    }
                    KeyCode::Delete if focus == 0 && cur < chars.len() => {
                        let mut c = chars.clone();
                        c.remove(cur);
                        value = c.into_iter().collect();
                        edited = true;
                    }
                    KeyCode::Left if focus == 0 => cur = cur.saturating_sub(1),
                    KeyCode::Right if focus == 0 => cur = (cur + 1).min(chars.len()),
                    KeyCode::Home if focus == 0 => cur = 0,
                    KeyCode::End if focus == 0 => cur = chars.len(),
                    KeyCode::Char(c) if focus == 0 && !ctrl => {
                        let mut v = chars.clone();
                        v.insert(cur, c);
                        cur += 1;
                        value = v.into_iter().collect();
                        edited = true;
                    }
                    _ => {}
                }
                if edited && let Ok(m) = fsx::parse_mode(&value, base, is_dir) {
                    bits = m;
                }
                self.dialog = Some(Dialog::Mode { paths, value, cur, base, is_dir, bits, focus });
            }
            Dialog::Input {
                title,
                mut value,
                mut cur,
                action,
            } => {
                let chars: Vec<char> = value.chars().collect();
                let mut action = action;
                let mut title = title;
                match k.code {
                    KeyCode::Esc => return Ok(()),
                    KeyCode::Enter => return self.submit(action, value, term),
                    KeyCode::Tab if matches!(action, InputAction::Symlink { .. }) => {
                        if let InputAction::Symlink { relative, targets, .. } = &mut action {
                            *relative = !*relative;
                            title = symlink_title(targets.len(), *relative);
                        }
                    }
                    KeyCode::Char('u') if ctrl => {
                        value.clear();
                        cur = 0;
                    }
                    KeyCode::Char('w') if ctrl => {
                        let before: String = chars[..cur].iter().collect();
                        let t = before.trim_end();
                        let keep = t.rfind(['/', ' ']).map(|i| i + 1).unwrap_or(0);
                        let kept: String = t[..keep].to_string();
                        let after: String = chars[cur..].iter().collect();
                        cur = kept.chars().count();
                        value = kept + &after;
                    }
                    KeyCode::Backspace if cur > 0 => {
                        let mut c = chars.clone();
                        c.remove(cur - 1);
                        cur -= 1;
                        value = c.into_iter().collect();
                    }
                    KeyCode::Delete if cur < chars.len() => {
                        let mut c = chars.clone();
                        c.remove(cur);
                        value = c.into_iter().collect();
                    }
                    KeyCode::Left => cur = cur.saturating_sub(1),
                    KeyCode::Right => cur = (cur + 1).min(chars.len()),
                    KeyCode::Home => cur = 0,
                    KeyCode::Char('a') if ctrl => cur = 0,
                    KeyCode::End => cur = chars.len(),
                    KeyCode::Char('e') if ctrl => cur = chars.len(),
                    KeyCode::Char(c) if !ctrl => {
                        let mut v = chars.clone();
                        v.insert(cur, c);
                        cur += 1;
                        value = v.into_iter().collect();
                    }
                    _ => {}
                }
                self.dialog = Some(Dialog::Input {
                    title,
                    value,
                    cur,
                    action,
                });
            }
        }
        Ok(())
    }

    /// `~` and relative paths (to the active panel's directory) resolved.
    fn resolve(&self, s: &str) -> PathBuf {
        let s = s.trim();
        let p = if s == "~" {
            fsx::home()
        } else if let Some(r) = s.strip_prefix("~/") {
            fsx::home().join(r)
        } else {
            PathBuf::from(s)
        };
        if p.is_absolute() {
            p
        } else {
            self.p().dir().unwrap_or(Path::new("/")).join(p)
        }
    }

    fn submit(&mut self, action: InputAction, value: String, term: &mut Term) -> std::io::Result<()> {
        match action {
            InputAction::Plugin(c) => self.plugin_call(&c, Some(&value)),
            InputAction::Mkdir => {
                if value.trim().is_empty() {
                    return Ok(());
                }
                let p = self.resolve(&value);
                let existed = p.exists();
                match std::fs::create_dir_all(&p) {
                    Ok(()) => {
                        if !existed {
                            self.undo.push(vec![ops::Undo::Created(p.clone())]);
                        }
                        self.pm().reload();
                        if let Some(n) = p.file_name() {
                            let n = n.to_string_lossy().into_owned();
                            self.pm().select_name(&n);
                        }
                    }
                    Err(e) => self.toast(format!("{}: {e}", p.display()), true),
                }
            }
            InputAction::NewFile if value.trim().ends_with('/') => {
                return self.submit(InputAction::Mkdir, value.trim().trim_end_matches('/').to_string() + "/", term);
            }
            InputAction::NewFile => {
                if value.trim().is_empty() {
                    return Ok(());
                }
                let p = self.resolve(&value);
                if p.symlink_metadata().is_ok() {
                    self.toast(format!("{} exists", p.display()), true);
                    return Ok(());
                }
                if let Some(parent) = p.parent()
                    && let Err(e) = std::fs::create_dir_all(parent)
                {
                    self.toast(format!("{}: {e}", parent.display()), true);
                    return Ok(());
                }
                match std::fs::OpenOptions::new().write(true).create_new(true).open(&p) {
                    Ok(_) => {
                        self.undo.push(vec![ops::Undo::Created(p.clone())]);
                        // a name with a directory part: the panel goes there, on the new file
                        if let Some(parent) = p.parent()
                            && self.p().dir() != Some(parent)
                        {
                            self.pm().go(Place::Dir(parent.to_path_buf()));
                        } else {
                            self.pm().reload();
                        }
                        if let Some(n) = p.file_name() {
                            let n = n.to_string_lossy().into_owned();
                            self.pm().select_name(&n);
                        }
                    }
                    Err(e) => self.toast(format!("{}: {e}", p.display()), true),
                }
            }
            InputAction::Rename(from) => {
                let to = from.with_file_name(value.trim());
                if to == from || value.trim().is_empty() {
                    return Ok(());
                }
                if to.symlink_metadata().is_ok() {
                    self.toast(format!("{} exists", to.display()), true);
                    return Ok(());
                }
                match std::fs::rename(&from, &to) {
                    Ok(()) => {
                        self.undo.push(vec![ops::Undo::Moved {
                            from: from.clone(),
                            to: to.clone(),
                        }]);
                        self.pm().reload();
                        self.pm().select_name(value.trim());
                    }
                    Err(e) => self.toast(format!("{e}"), true),
                }
            }
            InputAction::Chmod(paths) => {
                // octal, or symbolic (u+x, g-w,o-rwx, a=r, +X), applied to each item's own mode
                if let Err(e) = fsx::parse_mode(value.trim(), 0o644, false) {
                    self.toast(e, true);
                    return Ok(());
                }
                use std::os::unix::fs::{MetadataExt, PermissionsExt};
                let mut errs = Vec::new();
                for p in &paths {
                    let res = std::fs::metadata(p).map_err(|e| e.to_string()).and_then(|m| {
                        let mode = fsx::parse_mode(value.trim(), m.mode(), m.is_dir())?;
                        std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).map_err(|e| e.to_string())
                    });
                    if let Err(e) = res {
                        errs.push(format!("{}: {e}", p.display()));
                    }
                }
                self.after_trash_op(paths.len() - errs.len(), errs, &format!("mode {}", value.trim()));
            }
            InputAction::Symlink { targets, dir, relative } => {
                let mut made = Vec::new();
                let mut errs = Vec::new();
                for t in &targets {
                    // one target: the name typed; several: each link named as its target
                    let name = if targets.len() == 1 && !value.trim().is_empty() {
                        value.trim().to_string()
                    } else {
                        t.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
                    };
                    let link = if Path::new(&name).is_absolute() { PathBuf::from(&name) } else { dir.join(&name) };
                    let pointee = if relative {
                        relative_path(link.parent().unwrap_or(&dir), t)
                    } else {
                        t.clone()
                    };
                    match std::os::unix::fs::symlink(&pointee, &link) {
                        Ok(()) => made.push(link),
                        Err(e) => errs.push(format!("{}: {e}", link.display())),
                    }
                }
                if !made.is_empty() {
                    self.undo.push(made.iter().cloned().map(ops::Undo::Created).collect());
                }
                for p in &mut self.panels {
                    p.reload();
                }
                if errs.is_empty() {
                    self.toast(format!("{} link(s) made", made.len()), false);
                } else {
                    self.toast(errs.join("; "), true);
                }
            }
            InputAction::Chown(paths) => {
                if value.trim().is_empty() {
                    return Ok(());
                }
                let mut errs = Vec::new();
                for p in &paths {
                    if let Err(e) = fsx::chown(p, &value) {
                        errs.push(format!("{}: {e}", p.display()));
                    }
                }
                self.after_trash_op(paths.len() - errs.len(), errs, &format!("owner {}", value.trim()));
            }
            InputAction::Compress(dir, names) => {
                let out = self.resolve(&value);
                match crate::archive::compress_cmd(&dir, &names, &out) {
                    Ok(cmd) => self.start_command(JobKind::Compress, cmd, value.trim().to_string(), Some(out)),
                    Err(e) => self.toast(e, true),
                }
            }
            InputAction::CopyTo(src) => {
                let dest = self.resolve(&value);
                self.start_job(JobKind::Copy, src, Some(dest));
            }
            InputAction::MoveTo(src) => {
                let dest = self.resolve(&value);
                self.start_job(JobKind::Move, src, Some(dest));
            }
            InputAction::Mark(on) => {
                let n = self.pm().mark_glob(value.trim(), on);
                self.toast(format!("{} {n}", if on { "marked" } else { "unmarked" }), false);
            }
            InputAction::TabName => {
                let name = value.trim();
                let at = self.tab;
                self.tab_names[at] = (!name.is_empty()).then(|| name.to_string());
            }
            InputAction::Goto => {
                let p = self.resolve(&value);
                if p.is_dir() {
                    self.pm().go(Place::Dir(p));
                } else {
                    self.toast(format!("not a directory: {}", p.display()), true);
                }
            }
            InputAction::Shell => {
                if value.trim().is_empty() {
                    return Ok(());
                }
                let dir = self.p().dir().map(Path::to_path_buf).unwrap_or_else(fsx::home);
                term.suspend()?;
                let st = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(&value)
                    .current_dir(&dir)
                    .status();
                if let Err(e) = st {
                    eprintln!("lvc: {e}");
                }
                eprint!("\n[lvc] press a key to return ");
                crossterm::terminal::enable_raw_mode()?;
                loop {
                    if let Event::Key(_) = event::read()? {
                        break;
                    }
                }
                term.resume()?;
                for p in &mut self.panels {
                    p.reload();
                }
            }
        }
        Ok(())
    }

    fn confirmed(&mut self, action: ConfirmAction) {
        match action {
            ConfirmAction::Plugin(c) => self.plugin_call(&c, None),
            ConfirmAction::Delete(paths, direct) => {
                let kind = if direct { JobKind::Delete } else { JobKind::Trash };
                self.start_job(kind, paths, None);
            }
            ConfirmAction::Purge(items) => {
                let mut errs = Vec::new();
                for it in &items {
                    if let Err(e) = crate::trash::purge(it) {
                        errs.push(format!("{}: {e}", it.original.display()));
                    }
                }
                self.after_trash_op(items.len(), errs, "deleted for good");
            }
            ConfirmAction::EmptyTrash => {
                let items = crate::trash::list();
                let mut errs = Vec::new();
                for it in &items {
                    if let Err(e) = crate::trash::purge(it) {
                        errs.push(format!("{}: {e}", it.original.display()));
                    }
                }
                self.after_trash_op(items.len(), errs, "deleted for good");
            }
        }
    }

    fn after_trash_op(&mut self, n: usize, errs: Vec<String>, what: &str) {
        for p in &mut self.panels {
            p.marked.clear();
            p.reload();
        }
        if errs.is_empty() {
            self.toast(format!("{} {what}", n), false);
        } else {
            self.dialog = Some(Dialog::Message {
                title: format!("{} failed", errs.len()),
                lines: errs,
            });
        }
    }

    // ── operations ──────────────────────────────────────────────────────────────────────────────────

    fn copy_move(&mut self, mv: bool) {
        if self.p().is_trash() {
            if mv {
                self.restore();
            }
            return;
        }
        let src = self.p().targets();
        if src.is_empty() {
            return;
        }
        let dest = match self.other().dir() {
            Some(d) => fsx::tilde(d),
            None => String::new(),
        };
        let what = if src.len() == 1 {
            src[0]
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else {
            format!("{} items", src.len())
        };
        let title = format!("{} {what} to", if mv { "Move" } else { "Copy" });
        let act = if mv {
            InputAction::MoveTo(src)
        } else {
            InputAction::CopyTo(src)
        };
        self.input(&title, &dest, act);
    }

    fn rename(&mut self) {
        if self.p().is_trash() {
            return;
        }
        let Some(e) = self.p().current().cloned() else { return };
        if e.is_parent() {
            return;
        }
        let Some(path) = self.p().current_path() else { return };
        self.input("Rename", &e.name, InputAction::Rename(path));
    }

    fn delete(&mut self, direct: bool) {
        if self.p().is_trash() {
            let items = self.p().trash_targets();
            if items.is_empty() {
                return;
            }
            self.dialog = Some(Dialog::Confirm {
                title: "Delete for good".into(),
                lines: vec![format!("Delete {} item(s) from the trash for good?", items.len())],
                action: ConfirmAction::Purge(items),
            });
            return;
        }
        let t = self.p().targets();
        if t.is_empty() {
            return;
        }
        let what = if t.len() == 1 {
            fsx::tilde(&t[0])
        } else {
            format!("{} items", t.len())
        };
        self.dialog = Some(Dialog::Confirm {
            title: if direct { "Delete for good".into() } else { "Move to the trash".into() },
            lines: vec![if direct {
                format!("Delete {what} for good? This cannot be undone.")
            } else {
                format!("Move {what} to the trash?")
            }],
            action: ConfirmAction::Delete(t, direct),
        });
    }

    fn restore(&mut self) {
        let items = self.p().trash_targets();
        let mut errs = Vec::new();
        for it in &items {
            if let Err(e) = crate::trash::restore(it, None) {
                errs.push(format!("{}: {e}", it.original.display()));
            }
        }
        self.after_trash_op(items.len() - errs.len(), errs, "restored");
    }

    fn start_command(&mut self, kind: JobKind, cmd: std::process::Command, label: String, made: Option<PathBuf>) {
        if self.job.is_some() {
            self.toast("another operation is running", true);
            return;
        }
        self.job = Some(RunningJob {
            job: wake_on(ops::start_command(kind, cmd, label, made), self.wake_tx.clone()),
            done: 0,
            total: 0,
            file: String::new(),
            started: Instant::now(),
        });
    }

    fn menu_pick(&mut self, it: MenuItem, term: &mut Term) -> std::io::Result<()> {
        match it {
            MenuItem::Plugin(c, arg) => self.plugin_call(&c, arg.as_deref()),
            MenuItem::Dir(p) => self.pm().go(Place::Dir(p)),
            MenuItem::Action(i) => self.run_action(i, term)?,
            MenuItem::Builtin(b) => self.builtin(b, term)?,
            MenuItem::Peer(peer) => {
                let what = |d: PeerDo, t: &str| (t.to_string(), MenuItem::PeerDo(peer.clone(), d));
                let items = vec![
                    what(PeerDo::GoThere, &format!("Go to its directory ({})", peer.dir())),
                    what(PeerDo::BringHere, "Bring it to this directory"),
                    what(PeerDo::Copy, "Copy the marked into its directory"),
                    what(PeerDo::Move, "Move the marked into its directory"),
                ];
                self.dialog = Some(Dialog::Menu { title: format!("lvc {}", peer.pid), items, sel: 0 });
            }
            MenuItem::PeerDo(peer, d) => match d {
                PeerDo::GoThere => self.pm().go(Place::Dir(PathBuf::from(peer.dir()))),
                PeerDo::BringHere => {
                    if let Some(here) = self.p().dir().map(|d| d.to_string_lossy().into_owned())
                        && let Err(e) = crate::instances::send(peer.pid, &[format!("cd {here}")])
                    {
                        self.toast(e, true);
                    }
                }
                PeerDo::Copy | PeerDo::Move => {
                    let src = self.p().targets();
                    if !src.is_empty() {
                        let mv = matches!(d, PeerDo::Move);
                        let act = if mv { InputAction::MoveTo(src) } else { InputAction::CopyTo(src) };
                        let title = format!("{} to lvc {}", if mv { "Move" } else { "Copy" }, peer.pid);
                        self.input(&title, peer.dir(), act);
                    }
                }
            },
        }
        Ok(())
    }

    /// Ctrl-c I: the other running instances.
    fn instances_menu(&mut self) {
        let peers = crate::instances::peers();
        if peers.is_empty() {
            self.toast("no other lvc runs", false);
            return;
        }
        let items = peers
            .into_iter()
            .map(|p| (format!("{}  {}  |  {}", p.pid, fsx::tilde(Path::new(&p.left)), fsx::tilde(Path::new(&p.right))), MenuItem::Peer(p)))
            .collect();
        self.dialog = Some(Dialog::Menu { title: "Other lvc".into(), items, sel: 0 });
    }

    fn builtin(&mut self, b: Builtin, term: &mut Term) -> std::io::Result<()> {
        match b {
            Builtin::Copy => self.copy_move(false),
            Builtin::Move => self.copy_move(true),
            Builtin::Rename => {
                if self.p().marked.len() > 1 {
                    self.bulk_rename(term)?;
                } else {
                    self.rename();
                }
            }
            Builtin::NewFile => self.new_file(),
            Builtin::Trash => self.delete(self.cfg.delete == Delete::Direct),
            Builtin::Delete => self.delete(true),
            Builtin::Chmod => self.chmod(),
            Builtin::Chown => self.chown(),
            Builtin::Compress => self.compress(),
            Builtin::Extract => self.extract(),
            Builtin::Edit => self.edit_current(term)?,
            Builtin::View => {
                self.toggle_quick();
            }
            Builtin::Find => self.find(term, false)?,
            Builtin::Grep => self.find(term, true)?,
            Builtin::Goto => {
                let d = self.p().dir().map(fsx::tilde).unwrap_or_default();
                self.input("Go to directory", &d, InputAction::Goto)
            }
            Builtin::Bookmarks => self.bookmarks(),
            Builtin::Shell => self.input("Shell command", "", InputAction::Shell),
            Builtin::TrashView => {
                if self.p().is_trash() {
                    self.pm().back_in_history();
                } else {
                    self.pm().go(Place::Trash);
                }
            }
            Builtin::Hidden => {
                let p = self.pm();
                p.hidden = !p.hidden;
                p.reload();
            }
            Builtin::Long => self.toggle_long(),
            Builtin::Sort => {
                let p = self.pm();
                p.sort = p.sort.next();
                p.reload();
            }
            Builtin::Swap => self.panels.swap(0, 1),
            Builtin::Undo => self.undo_last(),
            Builtin::Reload => {
                for p in &mut self.panels {
                    p.reload();
                }
            }
            Builtin::Help => {
                self.help = true;
                self.help_sel = 0;
                self.help_off = 0;
            }
        }
        Ok(())
    }

    /// F9: the built-in operations, then every user action (config and plugins) with its key and where
    /// it comes from.
    fn action_menu(&mut self) {
        let kw = BUILTINS.iter().map(|b| b.2.len()).max().unwrap_or(4);
        let mut items: Vec<(String, MenuItem)> = BUILTINS
            .iter()
            .map(|(b, title, key)| (format!("{:<kw$}  {title}", self.show_key(&key.replace("C-c ", "C-c "))), MenuItem::Builtin(*b)))
            .collect();
        let user: Vec<(String, MenuItem)> = self
            .actions
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let key = a.key.as_ref().map(|k| format!("  [{}]", self.show_key(&k.text))).unwrap_or_default();
                let src = if a.source == "config" { String::new() } else { format!("  ({})", a.source) };
                (format!("{:<kw$}  {}{key}{src}", "", a.title), MenuItem::Action(i))
            })
            .collect();
        items.extend(user);
        self.dialog = Some(Dialog::Menu {
            title: "Menu".into(),
            items,
            sel: 0,
        });
    }

    fn action_ctx(&self) -> crate::plugins::Ctx {
        let p = self.p();
        crate::plugins::Ctx {
            file: p.current().filter(|e| !e.is_parent()).map(|e| p.path_of(e)),
            files: p.targets(),
            dir: p.dir().map(Path::to_path_buf).unwrap_or_else(fsx::home),
            other: self.other().dir().map(Path::to_path_buf),
        }
    }

    /// Run user action `i` in its mode (see plugins::Mode).
    fn run_action(&mut self, i: usize, term: &mut Term) -> std::io::Result<()> {
        use crate::plugins::Mode;
        let a = self.actions[i].clone();
        if let Some(call) = &a.call {
            let c = PluginCall { plugin: a.source.clone(), call: call.clone() };
            self.plugin_call(&c, None);
            return Ok(());
        }
        let ctx = self.action_ctx();
        let mut cmd = crate::plugins::command(&a.command, &ctx, a.dir.as_deref());
        match a.mode {
            Mode::Wait | Mode::Tty => {
                term.suspend()?;
                if let Err(e) = cmd.status() {
                    eprintln!("lvc: {}: {e}", a.title);
                }
                if a.mode == Mode::Wait {
                    eprint!("\n[lvc] {} — press a key to return ", a.title);
                    crossterm::terminal::enable_raw_mode()?;
                    loop {
                        if let Event::Key(_) = event::read()? {
                            break;
                        }
                    }
                }
                term.resume()?;
                if a.reload || a.mode == Mode::Tty {
                    for p in &mut self.panels {
                        p.reload();
                    }
                }
            }
            Mode::Silent | Mode::Control => {
                let tx = self.act_tx.clone();
                let wake = self.wake_tx.clone();
                let control = a.mode == Mode::Control;
                cmd.stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped());
                std::thread::spawn(move || {
                    let done = match cmd.output() {
                        Ok(o) => ActionDone {
                            title: a.title.clone(),
                            lines: String::from_utf8_lossy(&o.stdout).lines().map(str::to_string).collect(),
                            control,
                            reload: a.reload,
                            ok: o.status.success(),
                            err: String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("").to_string(),
                        },
                        Err(e) => ActionDone {
                            title: a.title.clone(),
                            lines: Vec::new(),
                            control,
                            reload: false,
                            ok: false,
                            err: e.to_string(),
                        },
                    };
                    let _ = tx.send(done);
                    let _ = wake.send(());
                });
            }
        }
        Ok(())
    }

    fn action_done(&mut self, d: ActionDone) {
        use crate::plugins::parse_instr;
        if !d.ok {
            self.toast(format!("{}: {}", d.title, if d.err.is_empty() { "failed" } else { &d.err }), true);
        } else if !d.control {
            self.toast(format!("{}: done", d.title), false);
        }
        if d.reload {
            for p in &mut self.panels {
                p.reload();
            }
        }
        if !d.control {
            return;
        }
        for line in &d.lines {
            let Some(ins) = parse_instr(line) else { continue };
            self.apply_instr(ins, None);
        }
    }

    /// Call a plugin function: the host starts the plugin if it is not running and sends the context; the
    /// answer comes out of `pump_procs`.
    fn plugin_call(&mut self, c: &PluginCall, arg: Option<&str>) {
        let cx = self.action_ctx();
        let dirs: Vec<PathBuf> = std::iter::once(cx.dir.clone()).chain(cx.other.clone()).collect();
        let ctx = cx.to_json(&c.call, arg, self.active);
        if let Err(e) = self.host.call(&c.plugin, &c.call, ctx, &dirs) {
            self.toast(e, true);
        }
    }

    /// What the plugins answered (or failed to) since the last look; then the events they asked for.
    fn pump_procs(&mut self) {
        use lvim_fuzzy_plugin::Out;
        for o in self.host.poll() {
            match o {
                Out::Answer { plugin, call, text, .. } => {
                    let from = PluginCall { plugin, call };
                    self.apply_responses(Some(&from), &text);
                }
                Out::Failed { text, .. } => self.toast(text, true),
            }
        }
        self.pump_previews();
        self.pump_events();
    }

    /// The preview plugins' answers, kept per file; the next draw uses them.
    fn pump_previews(&mut self) {
        let Some(pl) = self.plug.as_mut() else { return };
        for r in pl.poll() {
            if self.plug_wait.as_ref().is_some_and(|(t, ..)| *t == r.ticket) {
                self.plug_wait = None;
            }
            let skip = r.extra["skip"].as_u64().unwrap_or(0) as u32;
            let answer = r
                .answer
                .and_then(|text| lvim_fuzzy_ui::provided::parse(&text))
                .unwrap_or(lvim_fuzzy_ui::provided::Answer::Pass);
            if self.plug_answers.len() > 256 {
                self.plug_answers.clear();
            }
            self.plug_answers.insert((r.path, skip), answer);
        }
    }

    /// The preview a plugin gives `path`: Some(true) = drawn (or being asked: "…"), Some(false) = a plugin says
    /// Neovim shows it (`nvim`) / the built-in highlighter does (`builtin`) — see `PluginShow` — None = no plugin
    /// has anything to say, the built-in providers go on.
    fn draw_plugin_preview(&mut self, cv: &mut Canvas, l: &Look, inner: Rect, path: &Path) -> PluginShow {
        if self.plug.is_none() {
            return PluginShow::NotMine;
        }
        let skip = self.plug_skip;
        match self.plug_answers.get(&(path.to_path_buf(), skip)).cloned() {
            Some(lvim_fuzzy_ui::provided::Answer::Pass) => PluginShow::NotMine,
            Some(lvim_fuzzy_ui::provided::Answer::Preview(p)) => {
                if p.missing.is_empty() {
                    match p.renderer.as_deref() {
                        Some("nvim") => return PluginShow::Nvim,
                        Some("builtin") => return PluginShow::Builtin,
                        _ => {}
                    }
                }
                let key = format!("{}|{skip}", path.display());
                self.drawer.draw(cv, l, inner, &p, &key, &mut self.pv_off);
                PluginShow::Drawn
            }
            None => {
                let asked = self.plug_wait.as_ref().is_some_and(|(_, p, k)| p == path && *k == skip);
                if !asked {
                    let (pw, ph) = lvim_fuzzy_ui::provided::wanted_px(inner);
                    let extra = serde_json::json!({"cols": inner.w, "rows": inner.h, "host": "lvc", "skip": skip, "max_px": [pw, ph]});
                    match self.plug.as_mut().and_then(|pl| pl.request(path, extra)) {
                        Some(t) => self.plug_wait = Some((t, path.to_path_buf(), skip)),
                        None => {
                            self.plug_answers.insert((path.to_path_buf(), skip), lvim_fuzzy_ui::provided::Answer::Pass);
                            return PluginShow::NotMine;
                        }
                    }
                }
                cv.put(inner.x + 1, inner.y, "…", l.counter, inner.x + inner.w);
                PluginShow::Drawn
            }
        }
    }

    /// The directory, the file under the cursor and the marks (as the host compares them for the events).
    fn event_state(&self) -> (String, String, String) {
        let ctx = self.action_ctx();
        // the marks only (`files` also holds the entry under the cursor when nothing is marked)
        let mut m: Vec<&String> = self.p().marked.iter().collect();
        m.sort();
        (
            ctx.dir.to_string_lossy().into_owned(),
            ctx.file.as_ref().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(),
            m.into_iter().cloned().collect::<Vec<_>>().join("\n"),
        )
    }

    /// The JSON an event carries: the context, and the event's name.
    fn event_params(&self, event: &str) -> serde_json::Value {
        let mut p = self.action_ctx().to_json("", None, self.active);
        p["event"] = serde_json::Value::from(event);
        p
    }

    fn pump_events(&mut self) {
        if self.host.defs().iter().all(|d| d.events.is_empty()) {
            return;
        }
        let now = self.event_state();
        let params: Vec<(&str, serde_json::Value)> =
            ["cd", "cursor", "selection"].into_iter().map(|e| (e, self.event_params(e))).collect();
        self.host.pump_events(&now, |ev| params.iter().find(|(n, _)| *n == ev).map(|(_, v)| v.clone()).unwrap_or_default());
    }

    /// At the end: `quit` to the processes that asked for it, then the plugins are closed.
    fn stop_procs(&mut self) {
        let quit = self.event_params("quit");
        self.host.stop(|_| quit.clone());
    }

    /// Apply a plugin's answer; a bad one is shown as an error.
    fn apply_responses(&mut self, from: Option<&PluginCall>, answer: &str) {
        match crate::plugins::parse_response(answer) {
            Ok(list) => {
                for ins in list {
                    self.apply_instr(ins, from);
                }
            }
            Err(e) => self.toast(format!("plugin answer: {e}"), true),
        }
    }

    /// One instruction. `from` is the plugin the answer came from — the interactive ones (`input`,
    /// `confirm`, `menu`) call back into it.
    fn apply_instr(&mut self, ins: crate::plugins::Instr, from: Option<&PluginCall>) {
        use crate::plugins::Instr;
        let back = |then: &str| from.map(|f| PluginCall { call: then.to_string(), ..f.clone() });
        match ins {
            Instr::Cd(p) => {
                let p = self.resolve(&p.to_string_lossy());
                if p.is_dir() {
                    self.pm().go(Place::Dir(p));
                }
            }
            Instr::OtherCd(p) => {
                let p = self.resolve(&p.to_string_lossy());
                if p.is_dir() {
                    let o = 1 - self.active;
                    self.panels[o].go(Place::Dir(p));
                }
            }
            Instr::Select(n) => self.pm().select_name(&n),
            Instr::Mark(n) => {
                if self.p().entries.iter().any(|e| e.name == n) {
                    self.pm().marked.insert(n);
                }
            }
            Instr::Reload => {
                for p in &mut self.panels {
                    p.reload();
                }
            }
            Instr::Message(m) => self.toast(m, false),
            Instr::Error(m) => self.toast(m, true),
            Instr::Input { prompt, value, then } => match back(&then) {
                Some(c) => self.input(&prompt, &value, InputAction::Plugin(c)),
                None => self.toast("`input` needs a plugin with a runtime", true),
            },
            Instr::Confirm { text, then } => match back(&then) {
                Some(c) => {
                    self.dialog = Some(Dialog::Confirm {
                        title: "Plugin".into(),
                        lines: text.lines().map(str::to_string).collect(),
                        action: ConfirmAction::Plugin(c),
                    })
                }
                None => self.toast("`confirm` needs a plugin with a runtime", true),
            },
            Instr::Menu { title, items } => {
                let Some(f) = from else {
                    self.toast("`menu` needs a plugin with a runtime", true);
                    return;
                };
                if items.is_empty() {
                    return;
                }
                let items = items
                    .into_iter()
                    .map(|m| (m.label, MenuItem::Plugin(PluginCall { call: m.call, ..f.clone() }, m.arg)))
                    .collect();
                self.dialog = Some(Dialog::Menu { title, items, sel: 0 });
            }
            Instr::Preview(t) => {
                self.dialog = Some(Dialog::Message {
                    title: "Preview".into(),
                    lines: t.lines().map(str::to_string).collect(),
                });
            }
        }
    }

    /// Ctrl-z: take the last operation back (a move / rename moves back, a trashing is restored, a copy or
    /// a new directory goes to the trash).
    fn undo_last(&mut self) {
        let Some(ops_) = self.undo.pop() else {
            self.toast("nothing to undo", false);
            return;
        };
        let mut errs = Vec::new();
        let n = ops_.len();
        for u in ops_.into_iter().rev() {
            let r = match &u {
                ops::Undo::Moved { from, to } => {
                    if from.symlink_metadata().is_ok() {
                        Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, format!("{} exists", from.display())))
                    } else {
                        ops::move_path(to, from, &mut |_| true)
                    }
                }
                ops::Undo::Trashed(it) => crate::trash::restore(it, None).map(|_| ()),
                ops::Undo::Created(p) => crate::trash::trash(p).map(|_| ()),
            };
            if let Err(e) = r {
                errs.push(format!("{e}"));
            }
        }
        self.after_trash_op(n - errs.len(), errs, "undone");
    }

    fn new_file(&mut self) {
        if !self.p().is_trash() {
            self.input("New — a name ending in / is a directory", "", InputAction::NewFile)
        }
    }

    fn chmod(&mut self) {
        if self.p().is_trash() {
            return;
        }
        let t = self.p().targets();
        if t.is_empty() {
            return;
        }
        let (base, is_dir) = self
            .p()
            .current()
            .filter(|e| !e.is_parent())
            .map(|e| (e.mode & 0o7777, e.is_dir()))
            .unwrap_or((0o644, false));
        let value = format!("{base:o}");
        self.dialog = Some(Dialog::Mode {
            paths: t,
            cur: value.chars().count(),
            value,
            base,
            is_dir,
            bits: base,
            focus: 0,
        });
    }

    /// Ctrl-c O: the owner and group of the marked items (or the one under the cursor), chosen from the
    /// real users and groups.
    fn chown(&mut self) {
        if self.p().is_trash() {
            return;
        }
        let t = self.p().targets();
        if t.is_empty() {
            return;
        }
        let (cu, cg) = self
            .p()
            .current()
            .filter(|e| !e.is_parent())
            .map(|e| (fsx::user_name(e.uid), fsx::group_name(e.gid)))
            .unwrap_or_else(|| (fsx::user_name(unsafe { libc::geteuid() }), fsx::group_name(unsafe { libc::getegid() })));
        let mut users = fsx::real_users();
        let mut groups = fsx::real_groups();
        // the current ones are always there (a service account owning the file, say)
        if !users.contains(&cu) {
            users.insert(0, cu.clone());
        }
        if !groups.contains(&cg) {
            groups.insert(0, cg.clone());
        }
        let user = users.iter().position(|u| u == &cu).unwrap_or(0);
        let group = groups.iter().position(|g| g == &cg).unwrap_or(0);
        self.dialog = Some(Dialog::Owner { paths: t, users, groups, user, group, focus: 0, open: None });
    }

    /// Alt-b: the bookmarks of the config, then the directories visited.
    fn bookmarks(&mut self) {
        let mut items: Vec<(String, MenuItem)> = Vec::new();
        let mut bm: Vec<(&String, &String)> = self.cfg.bookmarks.iter().collect();
        bm.sort();
        for (name, path) in bm {
            let p = self.resolve(path);
            items.push((format!("{name}  {}", fsx::tilde(&p)), MenuItem::Dir(p)));
        }
        for p in &self.recent {
            if !items.iter().any(|(_, q)| matches!(q, MenuItem::Dir(d) if d == p)) {
                items.push((fsx::tilde(p), MenuItem::Dir(p.clone())));
            }
        }
        if items.is_empty() {
            self.toast("no bookmarks ([bookmarks] in the config) and no recent directories", false);
            return;
        }
        self.dialog = Some(Dialog::Menu {
            title: "Bookmarks · recent".into(),
            items,
            sel: 0,
        });
    }

    /// Alt-a: extract the archive under the cursor into `<other panel>/<archive stem>/`.
    fn extract(&mut self) {
        let Some(e) = self.p().current().cloned() else { return };
        if self.p().is_trash() || crate::archive::format(&e.name).is_none() {
            self.toast("not an archive", false);
            return;
        }
        let Some(src) = self.p().current_path() else { return };
        let Some(base) = self.other().dir().or(self.p().dir()).map(Path::to_path_buf) else {
            return;
        };
        let mut dest = base.join(crate::archive::stem(&e.name));
        if dest.symlink_metadata().is_ok() {
            dest = ops::free_name(&dest);
        }
        if let Err(err) = std::fs::create_dir_all(&dest) {
            self.toast(format!("{}: {err}", dest.display()), true);
            return;
        }
        match crate::archive::extract_cmd(&src, &dest) {
            Some(cmd) => self.start_command(JobKind::Extract, cmd, e.name.clone(), Some(dest)),
            None => self.toast("not an archive", false),
        }
    }

    /// Alt-z: pack the marked entries (or the one under the cursor) into an archive here.
    fn compress(&mut self) {
        if self.p().is_trash() {
            return;
        }
        let Some(dir) = self.p().dir().map(Path::to_path_buf) else { return };
        let names: Vec<String> = self
            .p()
            .targets()
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();
        if names.is_empty() {
            return;
        }
        let default = if names.len() == 1 {
            format!("{}.tar.gz", names[0])
        } else {
            format!(
                "{}.tar.gz",
                dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "archive".into())
            )
        };
        self.input("Archive name (.tar.gz .tar.xz .tar.zst .zip .7z ...)", &default, InputAction::Compress(dir, names));
    }

    /// F2 with several marked: edit their names in the editor, one per line; the changed ones are renamed
    /// (through temporary names, so swapping two names works).
    fn bulk_rename(&mut self, term: &mut Term) -> std::io::Result<()> {
        let Some(dir) = self.p().dir().map(Path::to_path_buf) else {
            return Ok(());
        };
        let names: Vec<String> = self
            .p()
            .entries
            .iter()
            .filter(|e| self.p().marked.contains(&e.name))
            .map(|e| e.name.clone())
            .collect();
        if names.iter().any(|n| n.contains('\n')) {
            self.toast("a name with a newline cannot be edited as a line", true);
            return Ok(());
        }
        let tmp = std::env::temp_dir().join(format!("lvc-rename-{}.txt", std::process::id()));
        std::fs::write(&tmp, names.join("\n") + "\n")?;
        self.edit(term, &tmp)?;
        let text = std::fs::read_to_string(&tmp).unwrap_or_default();
        let _ = std::fs::remove_file(&tmp);
        let new: Vec<String> = text.lines().map(str::to_string).collect();
        if new.len() != names.len() {
            self.toast(format!("{} names for {} entries: nothing renamed", new.len(), names.len()), true);
            return Ok(());
        }
        let pairs: Vec<(String, String)> = names
            .into_iter()
            .zip(new)
            .filter(|(a, b)| a != b && !b.trim().is_empty())
            .collect();
        // a target that exists and is not itself being renamed away is a conflict: stop before anything
        let leaving: std::collections::HashSet<&String> = pairs.iter().map(|(a, _)| a).collect();
        let clash: Vec<String> = pairs
            .iter()
            .filter(|(_, b)| dir.join(b).symlink_metadata().is_ok() && !leaving.contains(b))
            .map(|(_, b)| format!("{b} exists"))
            .collect();
        if !clash.is_empty() {
            self.dialog = Some(Dialog::Message {
                title: "Nothing renamed".into(),
                lines: clash,
            });
            return Ok(());
        }
        let mut undo = Vec::new();
        let mut errs = Vec::new();
        let mut staged = Vec::new();
        for (i, (a, b)) in pairs.iter().enumerate() {
            let t = dir.join(format!(".lvc-rename-{}-{i}", std::process::id()));
            match std::fs::rename(dir.join(a), &t) {
                Ok(()) => staged.push((t, a, b)),
                Err(e) => errs.push(format!("{a}: {e}")),
            }
        }
        for (t, a, b) in staged {
            match std::fs::rename(&t, dir.join(b)) {
                Ok(()) => undo.push(ops::Undo::Moved {
                    from: dir.join(a),
                    to: dir.join(b),
                }),
                Err(e) => {
                    let _ = std::fs::rename(&t, dir.join(a));
                    errs.push(format!("{a} → {b}: {e}"));
                }
            }
        }
        if !undo.is_empty() {
            self.undo.push(undo);
        }
        self.after_trash_op(pairs.len() - errs.len(), errs, "renamed");
        Ok(())
    }

    /// Enter on an entry: a directory is entered; a file opens (the `[open]` command for its extension,
    /// the editor for text, else xdg-open).
    fn open_current(&mut self, term: &mut Term) -> std::io::Result<()> {
        if self.p().is_trash() {
            return Ok(());
        }
        let Some(e) = self.p().current().cloned() else {
            return Ok(());
        };
        if e.is_dir() {
            self.pm().enter();
            return Ok(());
        }
        let Some(path) = self.p().current_path() else {
            return Ok(());
        };
        // what the open plugin says (the one rule lfz and the Neovim picker use too): a program, run detached, or
        // the editor; a program that is not installed is said and nothing is opened
        let opener = self.opener.get_or_insert_with(|| {
            let (wake, _rx) = channel();
            lvim_fuzzy_plugin::Previewer::load(&crate::config::plugin_roots(), "lvim-fuzzy-commander", &self.cfg.plugins, wake)
        });
        let answer = opener
            .ask("open", &path, serde_json::json!({"host": "lvim-fuzzy-commander"}), Duration::from_secs(5))
            .and_then(|t| lvim_fuzzy_ui::open::parse(&t));
        match answer {
            Ok(lvim_fuzzy_ui::open::Open::Exec(line)) => self.run_detached(&line),
            Ok(lvim_fuzzy_ui::open::Open::Editor) => self.edit(term, &path)?,
            Ok(lvim_fuzzy_ui::open::Open::Missing(m)) => self.toast(m.join("; "), true),
            Ok(lvim_fuzzy_ui::open::Open::Pass) => self.toast(format!("no open plugin has an answer for {}", path.display()), true),
            Err(e) => self.toast(e, true),
        }
        Ok(())
    }

    fn edit_current(&mut self, term: &mut Term) -> std::io::Result<()> {
        // a file, or a directory (the editor opens it: nvim shows its file browser); `..` = this directory
        if self.p().is_trash() {
            return Ok(());
        }
        let path = match self.p().current() {
            Some(e) if e.is_parent() => self.p().dir().map(Path::to_path_buf),
            Some(_) => self.p().current_path(),
            None => self.p().dir().map(Path::to_path_buf),
        };
        if let Some(p) = path {
            self.edit(term, &p)?;
        }
        Ok(())
    }

    fn edit(&mut self, term: &mut Term, path: &Path) -> std::io::Result<()> {
        let words = lvim_fuzzy_ui::util::shell_words(&self.cfg.editor).unwrap_or_else(|_| vec!["nvim".into()]);
        let Some((prog, args)) = words.split_first() else {
            return Ok(());
        };
        term.suspend()?;
        let r = std::process::Command::new(prog)
            .args(args)
            .arg(path)
            .current_dir(if path.is_dir() { path } else { path.parent().unwrap_or(Path::new("/")) })
            .status();
        term.resume()?;
        if let Err(e) = r {
            self.toast(format!("{prog}: {e}"), true);
        }
        for p in &mut self.panels {
            p.reload();
        }
        Ok(())
    }

    /// A GUI program (an image viewer, xdg-open): detached from the terminal and from lvc.
    fn run_detached(&mut self, line: &str) {
        if let Err(e) = lvim_fuzzy_ui::open::spawn_detached(line) {
            self.toast(format!("cannot open: {e}"), true);
        }
    }

    /// Alt-f / Alt-g: the lvim-fuzzy finder (files / live grep) from this directory; the panel goes to
    /// the chosen result, the cursor on it.
    fn find(&mut self, term: &mut Term, grep: bool) -> std::io::Result<()> {
        let Some(dir) = self.p().dir().map(Path::to_path_buf) else {
            return Ok(());
        };
        // the finder draws over this very screen (it is told to keep it): nothing flashes, the icons stay
        term.suspend_in_place()?;
        let out = std::process::Command::new("lvim-fuzzy-cli")
            .arg(if grep { "grep" } else { "files" })
            .current_dir(&dir)
            .env("LVIM_FUZZY_KEEP_SCREEN", "1")
            .stdin(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .output();
        term.resume_in_place()?;
        let out = match out {
            Ok(o) => o,
            Err(e) => {
                self.toast(format!("lvim-fuzzy-cli: {e}"), true);
                return Ok(());
            }
        };
        let text = String::from_utf8_lossy(&out.stdout);
        let Some(line) = text.lines().next().filter(|l| !l.is_empty()) else {
            return Ok(());
        };
        // a grep result is `path:line:col:text`; a file is the path
        let raw = if grep { line.split(':').next().unwrap_or(line) } else { line };
        let p = {
            let r = PathBuf::from(raw);
            if r.is_absolute() { r } else { dir.join(r) }
        };
        let (target_dir, name) = if p.is_dir() {
            (p, None)
        } else {
            (
                p.parent().map(Path::to_path_buf).unwrap_or(dir),
                p.file_name().map(|n| n.to_string_lossy().into_owned()),
            )
        };
        self.pm().go(Place::Dir(target_dir));
        if let Some(n) = name {
            self.pm().select_name(&n);
        }
        Ok(())
    }

    // ── mouse ───────────────────────────────────────────────────────────────────────────────────────

    fn mouse(&mut self, m: crossterm::event::MouseEvent, term: &mut Term) -> std::io::Result<()> {
        if self.dialog.is_some() {
            return Ok(());
        }
        // on a tab's name on the title bar: a click goes to it, a middle click closes it, a right click renames it
        if m.row == 0 && matches!(m.kind, MouseEventKind::Down(MouseButton::Left | MouseButton::Middle | MouseButton::Right)) {
            let hit = self.tab_btns.borrow().iter().find(|(a, b, _)| m.column >= *a && m.column < *b).map(|t| t.2);
            if let Some(i) = hit {
                match m.kind {
                    MouseEventKind::Down(MouseButton::Middle) => self.close_tab_at(i),
                    MouseEventKind::Down(MouseButton::Right) => {
                        self.go_tab(i);
                        self.rename_tab();
                    }
                    _ => self.go_tab(i),
                }
            }
            return Ok(());
        }
        // a click on a button of the bottom bar presses it
        if matches!(m.kind, MouseEventKind::Down(MouseButton::Left))
            && m.row == self.footer_y
            && let Some(&(_, _, n)) = self.footer_btns.iter().find(|(a, b, _)| m.column >= *a && m.column < *b)
        {
            self.footer_focus = None;
            self.synthetic = true;
            return self.key(KeyEvent::new(KeyCode::F(n), KeyModifiers::NONE), term);
        }
        let hit = |r: &Rect| m.column >= r.x && m.column < r.x + r.w && m.row >= r.y && m.row < r.y + r.h;
        let Some(i) = (0..2).find(|&i| hit(&self.list_rects[i])) else {
            return Ok(());
        };
        match m.kind {
            MouseEventKind::ScrollDown => self.panels[i].move_by(3),
            MouseEventKind::ScrollUp => self.panels[i].move_by(-3),
            MouseEventKind::Down(MouseButton::Left) => {
                self.active = i;
                let row = self.panels[i].offset + (m.row - self.list_rects[i].y) as usize;
                if row < self.panels[i].shown.len() {
                    let double = self
                        .last_click
                        .is_some_and(|(t, pi, r)| pi == i && r == row && t.elapsed() < Duration::from_millis(400));
                    self.panels[i].cursor = row;
                    self.last_click = Some((Instant::now(), i, row));
                    if double {
                        self.pm().enter();
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    // ── drawing ─────────────────────────────────────────────────────────────────────────────────────

    fn draw(&mut self, w: u16, h: u16) -> Canvas {
        self.term_rows = h as usize;
        let l = self.look.clone();
        let mut cv = Canvas::new(w, h);
        cv.base_bg = l.panel.bg;
        cv.base_fg = l.panel.fg;
        cv.clear();
        if w < 30 || h < 8 {
            cv.put(0, 0, "lvc: the terminal is too small", l.panel, w);
            return cv;
        }
        // the lvim-fuzzy frame: a title bar, the content (inset by a blank ring), the button bar
        self.draw_title(&mut cv, &l, w);
        let area = Rect { x: 0, y: 1, w, h: h - 2 };
        let mid = w / 2;
        // the separator between the two halves, as lfz draws it between its list and preview
        for y in area.y..area.y + area.h {
            cv.put(mid, y, "│", l.border, mid + 1);
        }
        let halves = [
            Rect { x: 1, y: area.y + 1, w: mid.saturating_sub(2), h: area.h.saturating_sub(2) },
            Rect { x: mid + 2, y: area.y + 1, w: w.saturating_sub(mid + 3), h: area.h.saturating_sub(2) },
        ];
        for (i, &r) in halves.iter().enumerate() {
            if self.quick && i != self.active {
                self.list_rects[i] = Rect { x: 0, y: 0, w: 0, h: 0 };
                self.draw_preview(&mut cv, &l, r);
            } else {
                self.draw_panel(&mut cv, &l, i, r);
            }
        }
        self.draw_footer(&mut cv, &l, h - 1, w);
        if self.chord && self.chord_hints {
            self.draw_chords(&mut cv, &l, h - 1, w);
        }
        if let Some(t) = &self.toast {
            let st = if t.error { l.marker } else { l.count };
            let text = format!(" {} ", t.text);
            let tw = str_width(&text).min(w as usize - 4) as u16;
            let x = (w - tw) / 2;
            let y = h - 3;
            cv.fill(Rect { x: x - 1, y, w: tw + 2, h: 1 }, l.popup);
            cv.put(x, y, &truncate(&text, tw as usize), Style { bg: l.popup.bg, ..st }, x + tw);
        }
        if self.help {
            self.draw_help(&mut cv, &l, w, h);
        }
        if self.dialog.is_some() {
            self.draw_dialog(&mut cv, &l, w, h);
        }
        cv
    }

    /// The title bar: what the active panel is (upper case, as lfz titles its finder), and on the right
    /// its counter — or the running operation's progress.
    fn draw_title(&self, cv: &mut Canvas, l: &Look, w: u16) {
        cv.fill(Rect { x: 0, y: 0, w, h: 1 }, l.title);
        let p = self.p();
        let what = if p.is_trash() { "TRASH".to_string() } else { "FILES".to_string() };
        let title = if p.query.is_empty() {
            format!(" {what} ")
        } else {
            format!(" {what}: {} ", p.query.to_uppercase())
        };
        let right = match &self.job {
            Some(rj) => {
                let pct = if rj.total > 0 { rj.done * 100 / rj.total.max(1) } else { 0 };
                let spin = SPIN[(rj.started.elapsed().as_millis() / 120) as usize % SPIN.len()];
                let size = if matches!(rj.job.kind, JobKind::Copy | JobKind::Move) {
                    format!("{} / {}", fsx::human_size(rj.done), fsx::human_size(rj.total))
                } else {
                    format!("{} / {}", rj.done, rj.total)
                };
                format!(" {spin} {} {pct}% · {size} · {} · Esc cancels ", rj.job.kind.label(), rj.file)
            }
            None => {
                let mut s = if p.query.is_empty() {
                    format!(" {} ", p.entries.iter().filter(|e| !e.is_parent()).count())
                } else {
                    format!(" {}/{} ", p.shown.len(), p.entries.iter().filter(|e| !e.is_parent()).count())
                };
                if !p.marked.is_empty() {
                    s = format!("{}· {} marked, {} ", s, p.marked.len(), fsx::human_size(p.marked_size()));
                }
                s
            }
        };
        let rw = str_width(&right) as u16;
        let mut x = cv.put(0, 0, &truncate(&title, w.saturating_sub(rw + 1) as usize), l.title, w);
        // the tabs, when there are several: number and the active panel's directory, the current one lit
        let mut btns = self.tab_btns.borrow_mut();
        btns.clear();
        if self.tabs.len() > 1 {
            let lim = w.saturating_sub(rw + 1);
            for i in 0..self.tabs.len() {
                let (panels, active) = match &self.tabs[i] {
                    Some(t) => (&t.panels, t.active),
                    None => (&self.panels, self.active),
                };
                let name = match (&self.tab_names[i], panels[active].dir()) {
                    (Some(own), _) => own.clone(),
                    (None, Some(d)) => d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "/".into()),
                    (None, None) => "trash".into(),
                };
                let text = format!(" {} {} ", i + 1, truncate(&name, 16));
                let st = if i == self.tab { l.footer_key } else { l.footer_label };
                x = cv.put(x, 0, " ", l.title, lim);
                let x0 = x;
                x = cv.put(x, 0, &text, st, lim);
                btns.push((x0, x, i));
            }
        }
        cv.put(w.saturating_sub(rw), 0, &right, l.counter, w);
    }

    /// The button bar, centred: " key " + " label ", one space apart (lfz / lvim-picker).
    /// After Ctrl-c: the commands it leads to, as Neovim's key helper — buttons in rows over the bottom,
    /// ending on the footer's row.
    fn draw_chords(&self, cv: &mut Canvas, l: &Look, bottom: u16, w: u16) {
        let place = if self.p().is_trash() { 2 } else { 1 };
        let mut items: Vec<(String, String)> = CHORDS
            .iter()
            .filter(|(_, _, p, _)| *p == 0 || *p == place)
            .map(|(k, lab, _, name)| {
                // `[keys]` may have moved the command
                let moved = self.cmdkeys.iter().find(|(n, u, c)| n == name && u != c).map(|(_, u, _)| key_label(&u.text));
                (moved.map(|m| m.trim_start_matches("C-c ").to_string()).unwrap_or_else(|| k.to_string()), lab.to_string())
            })
            .collect();
        for a in &self.actions {
            if let Some(k) = &a.key
                && k.chord
            {
                items.push((k.text.trim_start_matches("C-c ").to_string(), a.title.clone()));
            }
        }
        // a grid of equal cells, as many columns as fit
        let cell = items.iter().map(|(k, lab)| str_width(k) + str_width(lab) + 5).max().unwrap_or(10) as u16;
        let cols = (w.saturating_sub(4) / (cell + 1)).max(1);
        let rows = (items.len() as u16).div_ceil(cols);
        // the title, a blank row, the keys, a blank row, Esc on the footer's row
        let top = bottom.saturating_sub(rows + 3);
        cv.fill(Rect { x: 0, y: top, w, h: rows + 4 }, l.popup);
        let title = format!(" {} ", self.leader_name);
        let title = title.as_str();
        cv.put((w.saturating_sub(str_width(title) as u16)) / 2, top, title, l.title, w);
        let grid = cols * (cell + 1);
        let x0 = w.saturating_sub(grid) / 2;
        for (i, (k, lab)) in items.iter().enumerate() {
            let (r, c) = (i as u16 / cols, i as u16 % cols);
            let mut x = x0 + c * (cell + 1);
            let y = top + 2 + r;
            let end = x + cell;
            x = cv.put(x, y, &format!(" {k} "), l.footer_key, end);
            // the label's background runs to the end of the cell: every button as wide as the widest
            cv.fill(Rect { x, y, w: end.saturating_sub(x), h: 1 }, l.footer_label);
            cv.put(x, y, &format!(" {lab} "), l.footer_label, end);
        }
        let esc = " Esc  cancel ";
        cv.put((w.saturating_sub(str_width(esc) as u16)) / 2, bottom, esc, l.count, w);
    }

    /// The buttons of the bottom bar: (F-key, label).
    /// The buttons of the bottom bar: (the key now — `[keys]` may have moved it, the label, the action's own
    /// F-key number).
    fn footer_keys(&self) -> Vec<(String, &'static str, u8)> {
        let key = |a: &str| -> (String, u8) {
            let (_, ks, canon) = self.fkeys.iter().find(|(n, _, _)| *n == a).expect("an F-key action");
            let n = if let KeyCode::F(n) = canon.code { n } else { 0 };
            (key_label(&ks.text), n)
        };
        let list: Vec<(&str, &'static str)> = if self.p().is_trash() {
            vec![("help", "help"), ("view", "view"), ("move", "restore"), ("trash", "delete"), ("menu", "menu"), ("quit", "quit")]
        } else {
            vec![
                ("help", "help"),
                ("view", "view"),
                ("copy", "copy"),
                ("rename", "rename"),
                ("move", "move"),
                ("edit", "edit"),
                ("new", "new"),
                ("trash", if self.cfg.delete == Delete::Trash { "trash" } else { "delete" }),
                ("menu", "menu"),
                ("quit", "quit"),
            ]
        };
        list.into_iter()
            .map(|(a, lab)| {
                let (k, n) = key(a);
                (k, lab, n)
            })
            .collect()
    }

    /// The bottom bar: the F-keys as buttons. With the focus on it (Ctrl-j) the chosen one is lit, as the
    /// buttons of the lvim Lua plugins; the mouse presses them too.
    fn draw_footer(&mut self, cv: &mut Canvas, l: &Look, y: u16, w: u16) {
        cv.fill(Rect { x: 0, y, w, h: 1 }, l.panel);
        let keys = self.footer_keys();
        if let Some(f) = self.footer_focus {
            self.footer_focus = Some(f.min(keys.len().saturating_sub(1)));
        }
        let lit_key = Style { fg: l.footer_key.bg, bg: l.footer_key.fg, attrs: l.footer_key.attrs, ..l.footer_key };
        let lit_label = Style { fg: l.sel[0].fg, bg: l.sel[0].bg, attrs: l.sel[0].attrs, ..l.footer_label };
        let total: usize = keys.iter().map(|(k, lab, _)| str_width(k) + str_width(lab) + 4).sum::<usize>()
            + keys.len().saturating_sub(1);
        let mut x = if (total as u16) < w { (w - total as u16) / 2 } else { 0 };
        self.footer_btns.clear();
        self.footer_y = y;
        for (i, (k, lab, n)) in keys.iter().enumerate() {
            if i > 0 {
                x = cv.put(x, y, " ", l.panel, w);
            }
            let bw = (str_width(k) + str_width(lab) + 4) as u16;
            if x + bw > w.saturating_sub(2) && total as u16 >= w {
                cv.put(w.saturating_sub(2), y, " ❯", l.chevron, w);
                break;
            }
            let lit = self.footer_focus == Some(i);
            let x0 = x;
            x = cv.put(x, y, &format!(" {k} "), if lit { lit_key } else { l.footer_key }, w);
            x = cv.put(x, y, &format!(" {lab} "), if lit { lit_label } else { l.footer_label }, w);
            self.footer_btns.push((x0, x, *n));
        }
    }

    /// Ctrl-j on the bar: h / l (← →, Tab) choose, Enter / Space press, Esc / Ctrl-k / k back to the list.
    fn footer_key(&mut self, k: KeyEvent, term: &mut Term) -> std::io::Result<()> {
        let n = self.footer_keys().len();
        let Some(f) = self.footer_focus else {
            return Ok(());
        };
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Tab if !ctrl => self.footer_focus = Some((f + 1) % n.max(1)),
            KeyCode::Char('h') | KeyCode::Left | KeyCode::BackTab if !ctrl => {
                self.footer_focus = Some((f + n.max(1) - 1) % n.max(1))
            }
            KeyCode::Char('0') | KeyCode::Char('^') | KeyCode::Home => self.footer_focus = Some(0),
            KeyCode::Char('$') | KeyCode::End => self.footer_focus = Some(n.saturating_sub(1)),
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.footer_focus = None;
                if let Some(&(_, _, num)) = self.footer_keys().get(f) {
                    self.synthetic = true;
                    self.key(KeyEvent::new(KeyCode::F(num), KeyModifiers::NONE), term)?;
                }
            }
            // Ctrl-j on the bar: down, out of lvc (Zellij)
            KeyCode::Char('j') if ctrl => {
                crate::mux::move_focus("down");
            }
            KeyCode::Esc | KeyCode::Char('k') | KeyCode::Up => self.footer_focus = None,
            KeyCode::Char('q') if ctrl => self.quit = true,
            // an F-key itself still works
            KeyCode::F(_) => {
                self.footer_focus = None;
                self.key(k, term)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// The bar over a panel: the directory, drawn as lfz draws the file over its preview (icon, name,
    /// the parent dimmer).
    fn draw_path_bar(&self, cv: &mut Canvas, l: &Look, r: Rect, path: &Path, icon_override: Option<&str>, reserve: u16) {
        cv.fill(Rect { x: r.x, y: r.y, w: r.w, h: 1 }, l.file_bar);
        let limit = (r.x + r.w).saturating_sub(reserve);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let image = match icon_override {
            Some(_) => None,
            None => lvim_fuzzy_ui::iconimg::image_id(&path.to_string_lossy(), path.is_dir()),
        };
        let mut x = if let Some(id) = image {
            let x = cv.put(r.x, r.y, " ", l.file_name, limit);
            cv.put_image_icon(x, r.y, id, l.file_name.bg, limit)
        } else {
            let (ic, accent) = match icon_override {
                Some(i) => (i, "blue"),
                None => lvim_fuzzy_ui::lvim::icon(&path.to_string_lossy(), path.is_dir()),
            };
            let icon_st = Style { fg: l.theme.color(accent).or(l.file_icon.fg), bg: l.file_name.bg.or(l.file_icon.bg), ..l.file_icon };
            cv.put(r.x, r.y, &format!(" {ic}"), icon_st, limit)
        };
        x = cv.put(x, r.y, &format!(" {name} "), l.file_name, limit);
        if let Some(parent) = path.parent().filter(|_| path.file_name().is_some()) {
            let d = fsx::tilde(parent);
            let d = if d.ends_with('/') { d } else { format!("{d}/") };
            cv.put(x, r.y, &format!(" {d} "), l.file_dir, limit);
        }
    }

    /// One panel: its path bar, its query row (the lfz prompt), the rows, and the line about the entry
    /// under the cursor.
    fn draw_panel(&mut self, cv: &mut Canvas, l: &Look, i: usize, r: Rect) {
        if r.h < 4 || r.w < 10 {
            return;
        }
        let active = i == self.active;
        // the path bar
        match self.panels[i].place.clone() {
            Place::Dir(d) => {
                let p = &self.panels[i];
                let fw = 2 + p.sort.label().len() as u16 + if p.reverse { 2 } else { 0 } + if p.hidden { 9 } else { 0 };
                self.draw_path_bar(cv, l, r, &d, None, fw + 1)
            }
            Place::Trash => {
                cv.fill(Rect { x: r.x, y: r.y, w: r.w, h: 1 }, l.file_bar);
                let st = Style { fg: l.theme.color("red").or(l.file_icon.fg), ..l.file_icon };
                let x = cv.put(r.x, r.y, " 󰩹", st, r.x + r.w);
                cv.put(x, r.y, " Trash ", l.file_name, r.x + r.w);
            }
        }
        // the order and the hidden-files flag, at the right end of the bar
        {
            let p = &self.panels[i];
            let flags = format!(
                " {}{}{} ",
                p.sort.label(),
                if p.reverse { " ↑" } else { "" },
                if p.hidden { " · hidden" } else { "" }
            );
            let fw = str_width(&flags) as u16;
            if r.w > fw + 20 {
                cv.put(r.x + r.w - fw, r.y, &flags, l.file_dir, r.x + r.w);
            }
        }
        // the prompt: the badge (bright on the active panel), then the field
        let py = r.y + 1;
        let p = &self.panels[i];
        let searching = active && self.searching;
        let badge_st = if searching { l.prompt } else { Style { bg: l.input.bg, ..l.count } };
        let mut x = cv.put(r.x, py, " Search ", badge_st, r.x + r.w);
        x = cv.put(x, py, " ", l.panel, r.x + r.w);
        let field = Rect { x, y: py, w: (r.x + r.w).saturating_sub(x), h: 1 };
        cv.fill(field, l.input);
        let qx = if p.query.is_empty() {
            let ghost = if searching {
                "type to filter…"
            } else if active {
                "C-f or / to filter"
            } else {
                ""
            };
            if searching {
                field.x + 1
            } else {
                cv.put(field.x + 1, py, ghost, Style { bg: l.input.bg, ..l.count }, field.x + field.w);
                0
            }
        } else {
            cv.put(field.x + 1, py, &p.query, l.input, field.x + field.w)
        };
        if searching && qx > 0 {
            // the caret: a reversed cell after the query (the ghost text follows it when empty)
            let caret = Style { attrs: crossterm::style::Attributes::from(crossterm::style::Attribute::Reverse), ..l.input };
            cv.put(qx, py, " ", caret, field.x + field.w);
            if p.query.is_empty() {
                cv.put(qx + 1, py, "type to filter…", Style { bg: l.input.bg, ..l.count }, field.x + field.w);
            }
        }
        // the rows, between the prompt and the info line
        let list = Rect { x: r.x, y: r.y + 2, w: r.w, h: r.h.saturating_sub(3) };
        self.list_rects[i] = list;
        let p = &mut self.panels[i];
        p.scroll(list.h as usize);
        if let Some(e) = &p.error {
            cv.put(list.x + 1, list.y, &truncate(e, list.w as usize - 1), l.marker, list.x + list.w);
        } else if p.shown.is_empty() {
            let msg = if p.query.is_empty() { "[empty]" } else { "[no matches]" };
            cv.put(list.x + 1, list.y, msg, l.count, list.x + list.w);
        }
        let role = |name: &str, fallback: &str| -> Style {
            l.theme
                .roles
                .get(name)
                .copied()
                .unwrap_or(Style { fg: l.theme.color(fallback), ..Style::default() })
        };
        let (dir_st, exec_st, link_st, broken_st) =
            (role("dir", "blue"), role("exec", "green"), role("link", "cyan"), role("broken", "red"));
        // the size column's text: a file's size, a directory's count of entries
        let counts = &self.dir_counts;
        let size_of = |p: &Panel, e: &fsx::Entry| -> (String, &'static str) {
            if e.is_dir() {
                let path = p.dir().map(|d| d.join(&e.name));
                match path.and_then(|d| dir_count(counts, &d, e.mtime)) {
                    Some(n) => (format!("({n})"), "blue"),
                    None => ("(?)".to_string(), "comment"),
                }
            } else {
                let h = fsx::human_size(e.size);
                let colour = match h.chars().last() {
                    Some('K') => "green",
                    Some('M') => "yellow",
                    Some('G') | Some('T') => "red",
                    _ => "comment",
                };
                (h, colour)
            }
        };
        // each column as wide as its widest value on screen: no padding past the longest name or size
        let (mut owner_w, mut group_w, mut size_w) = (0usize, 0usize, 0usize);
        for row in 0..list.h as usize {
            let Some(&ei) = p.shown.get(p.offset + row) else { break };
            let e = &p.entries[ei];
            if e.is_parent() {
                continue;
            }
            if self.cols.owner {
                owner_w = owner_w.max(str_width(&fsx::user_name(e.uid)).min(16));
            }
            if self.cols.group {
                group_w = group_w.max(str_width(&fsx::group_name(e.gid)).min(16));
            }
            if self.cols.size {
                size_w = size_w.max(str_width(&size_of(p, e).0));
            }
        }
        for row in 0..list.h as usize {
            let idx = p.offset + row;
            let Some(&ei) = p.shown.get(idx) else { break };
            let e = &p.entries[ei];
            let y = list.y + row as u16;
            let stripe = idx % 2;
            let is_cur = idx == p.cursor;
            let base = if is_cur && active {
                l.sel[stripe]
            } else if is_cur {
                Style { bg: l.cursor_line.bg.or(l.rows[stripe].bg), ..l.rows[stripe] }
            } else {
                l.rows[stripe]
            };
            cv.fill(Rect { x: list.x, y, w: list.w, h: 1 }, base);
            let lim = list.x + list.w;
            let marked = p.marked.contains(&e.name);
            let mut x = cv.put(list.x, y, if marked { "➤" } else { " " }, Style { bg: base.bg, ..l.marker }, lim);
            // the columns right of the name (Ctrl-c v), each in its own colour; the narrowest panel drops
            // them from the least needed (group, owner, permissions, type, date, size) so the name keeps room
            let mut cells: Vec<(&str, String, Style)> = Vec::new();
            if !e.is_parent() {
                let c = self.cols;
                let tb = base.bg;
                if c.kind && !p.is_trash() {
                    let (t, colour) = if e.broken {
                        ("BRK", "red")
                    } else if matches!(e.kind, Kind::Link | Kind::LinkDir) {
                        ("LNK", "cyan")
                    } else if e.is_dir() {
                        ("DIR", "blue")
                    } else if e.is_exec() {
                        ("EXE", "green")
                    } else {
                        ("FILE", "comment")
                    };
                    cells.push(("kind", format!("{t:<4}"), tint(l, colour, base, tb)));
                }
                if c.mode && !p.is_trash() {
                    cells.push(("mode", fsx::mode_string(e), base));
                }
                if c.owner && !p.is_trash() {
                    let u = fsx::user_name(e.uid);
                    let st = tint(l, if e.uid == 0 { "red" } else { "orange" }, base, tb);
                    cells.push(("owner", format!("{:<owner_w$}", truncate(&u, 16)), st));
                }
                if c.group && !p.is_trash() {
                    cells.push((
                        "group",
                        format!("{:<group_w$}", truncate(&fsx::group_name(e.gid), 16)),
                        tint(l, "teal", base, tb),
                    ));
                }
                if c.size {
                    // a directory: how many entries it holds, in parentheses (a size it has not)
                    let (txt, colour) = size_of(p, e);
                    cells.push(("size", format!("{txt:>size_w$}"), tint(l, colour, base, tb)));
                }
                if c.date {
                    let (txt, t) = if p.is_trash() {
                        let it = p.trash_items.get(ei);
                        (it.map(|t| t.deleted.replace('T', " ").chars().take(16).collect()).unwrap_or_default(), e.mtime)
                    } else {
                        (fsx::date_string(e.mtime), e.mtime)
                    };
                    cells.push(("date", txt, tint(l, age_colour(t), base, tb)));
                }
            }
            let width = |cells: &[(&str, String, Style)]| -> u16 {
                cells.iter().map(|c| str_width(&c.1) as u16 + 2).sum::<u16>() + if cells.is_empty() { 0 } else { 1 }
            };
            for drop in ["group", "owner", "mode", "kind", "date", "size"] {
                if lim.saturating_sub(x) >= width(&cells) + 14 {
                    break;
                }
                cells.retain(|c| c.0 != drop);
            }
            let rw = width(&cells);
            let name_lim = lim.saturating_sub(rw).max(x);
            // the icon: a Material image (iconimg) where the terminal takes them, else the Nerd glyph
            if let Some(id) = lvim_fuzzy_ui::iconimg::image_id(&e.name, e.is_dir()) {
                x = cv.put_image_icon(x, y, id, base.bg, name_lim);
            } else {
                let (glyph, accent) = lvim_fuzzy_ui::lvim::icon(&e.name, e.is_dir());
                let ifg = l.theme.color(accent).or(base.fg);
                x = cv.put(x, y, glyph, Style { fg: ifg, bg: base.bg, ..base }, name_lim);
            }
            x = cv.put(x, y, " ", base, name_lim);
            // the name in its file-listing role (dir / exec / link / broken), the matches marked
            let kind_st = if e.broken {
                Some(broken_st)
            } else if matches!(e.kind, Kind::Link | Kind::LinkDir) {
                Some(link_st)
            } else if e.is_dir() {
                Some(dir_st)
            } else if e.is_exec() {
                Some(exec_st)
            } else {
                None
            };
            let ns = match kind_st {
                Some(k) => Style {
                    fg: k.fg.or(base.fg),
                    bg: base.bg,
                    attrs: k.attrs,
                    ..base
                },
                None => base,
            };
            let hl = p.hl.get(idx).cloned().unwrap_or_default();
            let mut name = e.name.clone();
            if e.is_dir() && !e.is_parent() {
                name.push('/');
            }
            for (ci, ch) in name.chars().enumerate() {
                if x >= name_lim {
                    break;
                }
                let st = if hl.contains(&ci) {
                    Style { fg: l.matched.fg, bg: base.bg, attrs: l.matched.attrs, ..ns }
                } else {
                    ns
                };
                let mut b = [0u8; 4];
                x = cv.put(x, y, ch.encode_utf8(&mut b), st, name_lim);
            }
            if matches!(e.kind, Kind::Link | Kind::LinkDir)
                && let Some(t) = &e.target
            {
                cv.put(x, y, &format!(" → {t}"), tint(l, "cyan", base, base.bg), name_lim);
            }
            let mut cx = lim - rw;
            for (key, txt, st) in &cells {
                cx = cv.put(cx, y, "  ", base, lim);
                cx = if *key == "mode" { put_mode(cv, l, cx, y, txt, base, lim) } else { cv.put(cx, y, txt, *st, lim) };
            }
            if !cells.is_empty() {
                cv.put(cx, y, " ", base, lim);
            }
        }
        // the line about the entry under the cursor (dim, as lfz's counters)
        let iy = r.y + r.h - 1;
        let info = match p.current() {
            Some(e) if p.is_trash() => {
                let it = p.current_trash_item();
                format!(" {}  deleted {}", fsx::human_size(e.size), it.map(|t| t.deleted.as_str()).unwrap_or(""))
            }
            Some(e) if !e.is_parent() => {
                let e = e.clone();
                put_facts(cv, l, r.x, iy, &e, l.count, r.x + r.w, true);
                return;
            }
            _ => String::new(),
        };
        cv.put(r.x, iy, &truncate(&info, r.w as usize), l.count, r.x + r.w);
    }

    /// The quick view: as lfz's preview — the file bar, then the body.
    fn draw_preview(&mut self, cv: &mut Canvas, l: &Look, r: Rect) {
        let path = self.p().current_path();
        let Some(path) = path.filter(|p| !p.as_os_str().is_empty()) else {
            return;
        };
        if r.h < 3 || r.w < 8 {
            return;
        }
        self.draw_path_bar(cv, l, r, &path, None, 0);
        let inner = Rect { x: r.x, y: r.y + 1, w: r.w, h: r.h - 1 };
        let ps = path.to_string_lossy().into_owned();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| ps.clone());
        if self.pv_path.as_deref() != Some(ps.as_str()) {
            self.pv_path = Some(ps.clone());
            self.pv_off = 0;
        }
        let is_file = path.is_file();
        // the preview plugins first: they choose the engine from their configuration for this file's type
        let mut via_nvim = false;
        let shown = self.draw_plugin_preview(cv, l, inner, &path);
        if !matches!(shown, PluginShow::Drawn) {
            // a picture put on the terminal earlier is taken off when something else takes the place
            self.drawer.forget();
        }
        match shown {
            PluginShow::Drawn => return,
            PluginShow::Builtin => {
                self.draw_file_preview(cv, l, inner, &ps);
                return;
            }
            PluginShow::Nvim => via_nvim = true,
            PluginShow::NotMine => {}
        }
        // a user previewer for this file (config [previewers] / a plugin): its output, ANSI colours kept
        if is_file && !via_nvim && let Some(pi) = self.previewers.iter().position(|p| p.applies(&name)) {
            self.draw_previewer(cv, l, inner, pi, &path);
            return;
        }
        // a real Neovim for files (when configured and it starts); the built-in preview until it has
        // painted this file, and for directories
        if is_file && !self.nvim_failed {
            if self.nvim.is_none() {
                match Nvim::start(&self.cfg.nvim, None, self.cfg.pool, self.wake_tx.clone()) {
                    Ok(n) => self.nvim = Some(n),
                    Err(_) => self.nvim_failed = true,
                }
            }
            if let Some(n) = self.nvim.as_mut() {
                n.size(inner.w as usize, inner.h as usize);
                let shown = n.grid.lock().unwrap().shown.clone();
                if shown.as_deref() != Some(ps.as_str()) {
                    let pending = self.pv_path_sent.as_deref() != Some(ps.as_str());
                    if pending {
                        n.open(&ps, 0, 0, "", "");
                        self.pv_path_sent = Some(ps.clone());
                    }
                } else {
                    let g = n.grid.lock().unwrap();
                    if g.exited {
                        drop(g);
                        self.nvim = None;
                        self.nvim_failed = true;
                    } else if g.ready {
                        let (gw, gh) = (g.w.min(inner.w as usize), g.h.min(inner.h as usize));
                        for row in 0..gh {
                            for col in 0..gw {
                                let cell = &g.cells[row * g.w + col];
                                if cell.text.is_empty() {
                                    continue;
                                }
                                let st = g.style(cell.hl);
                                let x = inner.x + col as u16;
                                let y = inner.y + row as u16;
                                cv.fill(Rect { x, y, w: 1, h: 1 }, Style { bg: st.bg, ..Style::default() });
                                cv.put(x, y, &cell.text, st, inner.x + inner.w);
                            }
                        }
                        return;
                    }
                }
            }
        }
        self.draw_file_preview(cv, l, inner, &ps);
    }

    fn draw_file_preview(&mut self, cv: &mut Canvas, l: &Look, inner: Rect, path: &str) {
        let Some(view) = self.loader.view(path) else {
            return;
        };
        if let Some(note) = &view.note {
            cv.put(inner.x + 1, inner.y, note, l.counter, inner.x + inner.w);
            return;
        }
        let total = view.lines.len();
        let h = inner.h as usize;
        self.pv_off = self.pv_off.min(total.saturating_sub(h));
        let nr_w = if view.is_dir { 0 } else { total.max(1).to_string().len() as u16 + 1 };
        for (i, line) in view.lines.iter().skip(self.pv_off).take(h).enumerate() {
            let y = inner.y + i as u16;
            let lnum = self.pv_off + i + 1;
            let mut x = inner.x;
            if nr_w > 0 {
                x = cv.put(x, y, &format!("{lnum:>w$} ", w = nr_w as usize - 1), l.line_nr, inner.x + inner.w);
            }
            let spans = view.spans.get(self.pv_off + i);
            for (ci, ch) in line.chars().enumerate() {
                if x >= inner.x + inner.w {
                    break;
                }
                let ch = if ch == '\t' { ' ' } else { ch };
                let mut st = l.panel;
                if let Some(sp) = spans
                    && let Some(&(_, _, tok)) = sp.iter().find(|s| s.0 as usize <= ci && ci < s.1 as usize)
                {
                    let (a, italic, bold) = lvim_fuzzy_ui::syntax::accent(tok);
                    st.fg = l.theme.color(a).or(st.fg);
                    if italic {
                        st.attrs.set(Attribute::Italic);
                    }
                    if bold {
                        st.attrs.set(Attribute::Bold);
                    }
                }
                let mut b = [0u8; 4];
                x = cv.put(x, y, ch.encode_utf8(&mut b), st, inner.x + inner.w);
            }
        }
    }

    fn draw_previewer(&mut self, cv: &mut Canvas, l: &Look, inner: Rect, pi: usize, path: &Path) {
        let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        let key = format!("{}\0{}\0{:?}\0{}", self.previewers[pi].name, path.display(), mtime, inner.w);
        let have = self.pv_out.lock().unwrap().get(&key).cloned();
        let Some(lines) = have else {
            let map = self.pv_out.clone();
            let wake = self.wake_tx.clone();
            let pv = self.previewers[pi].clone();
            let ctx = crate::plugins::Ctx {
                file: Some(path.to_path_buf()),
                files: vec![path.to_path_buf()],
                dir: path.parent().map(Path::to_path_buf).unwrap_or_else(fsx::home),
                other: self.other().dir().map(Path::to_path_buf),
            };
            let cols = inner.w;
            map.lock().unwrap().insert(key.clone(), vec!["…".into()]);
            std::thread::spawn(move || {
                let mut cmd = crate::plugins::command(&pv.command, &ctx, pv.dir.as_deref());
                // the preview's size, as fzf gives its previewers
                cmd.env("LVC_PREVIEW_COLUMNS", cols.to_string())
                    .env("FZF_PREVIEW_COLUMNS", cols.to_string())
                    .stdin(std::process::Stdio::null());
                let lines = match cmd.output() {
                    Ok(o) => {
                        let mut v: Vec<String> = String::from_utf8_lossy(&o.stdout).lines().map(str::to_string).collect();
                        if v.is_empty() {
                            v = String::from_utf8_lossy(&o.stderr).lines().map(str::to_string).collect();
                        }
                        v
                    }
                    Err(e) => vec![format!("{}: {e}", pv.name)],
                };
                map.lock().unwrap().insert(key, lines);
                let _ = wake.send(());
            });
            return;
        };
        let h = inner.h as usize;
        self.pv_off = self.pv_off.min(lines.len().saturating_sub(h));
        let mut carry = Style::default();
        for (i, line) in lines.iter().enumerate() {
            let (text, runs, end) = lvim_fuzzy_ui::ansi::parse(line, carry);
            carry = end;
            if i < self.pv_off {
                continue;
            }
            let row = i - self.pv_off;
            if row >= h {
                break;
            }
            let y = inner.y + row as u16;
            let mut x = inner.x;
            for (bi, ch) in text.char_indices() {
                if x >= inner.x + inner.w {
                    break;
                }
                if ch.is_control() && ch != '\t' {
                    continue;
                }
                let st = lvim_fuzzy_ui::ansi::style_at(&runs, bi);
                let st = Style { fg: st.fg.or(l.panel.fg), bg: st.bg.or(l.panel.bg), ..st };
                let ch = if ch == '\t' { ' ' } else { ch };
                let mut b = [0u8; 4];
                x = cv.put(x, y, ch.encode_utf8(&mut b), st, inner.x + inner.w);
            }
        }
    }

    /// A popup as lfz draws them: a block in the popup colours, its frame of spaces in the border colour,
    /// the title on the frame.
    fn popup(&self, cv: &mut Canvas, l: &Look, (w, h): (u16, u16), (pw, ph): (u16, u16), title: &str) -> Rect {
        let pw = pw.min(w.saturating_sub(4)).max(10);
        // one blank row between the title edge and the content, as the content ends with one before the
        // bottom edge (the hints row is followed by the frame): the same air above and below
        let ph = (ph + 1).min(h.saturating_sub(2)).max(4);
        let r = Rect { x: (w - pw) / 2, y: (h - ph) / 2, w: pw, h: ph };
        cv.fill(r, l.popup_border);
        cv.fill(Rect { x: r.x + 1, y: r.y + 1, w: r.w - 2, h: r.h - 2 }, l.popup);
        let t = format!(" {title} ");
        let tw = str_width(&t) as u16;
        cv.put(r.x + (r.w.saturating_sub(tw)) / 2, r.y, &t, l.title, r.x + r.w);
        Rect { x: r.x + 2, y: r.y + 2, w: r.w.saturating_sub(4), h: r.h.saturating_sub(3) }
    }

    fn draw_dialog(&self, cv: &mut Canvas, l: &Look, w: u16, h: u16) {
        let Some(d) = &self.dialog else { return };
        let text = Style { bg: l.popup.bg, ..l.popup };
        match d {
            Dialog::Columns { sel } => {
                let all = crate::config::Cols::ALL;
                let lw = all.iter().map(|(_, lab)| str_width(lab)).max().unwrap_or(10) as u16;
                let r = self.popup(cv, l, (w, h), (lw + 12, all.len() as u16 + 4), "Columns");
                let lim = r.x + r.w;
                let on = Style { fg: l.theme.color("green").or(text.fg), ..text };
                let dim = Style { bg: l.popup.bg, ..l.count };
                for (i, (key, lab)) in all.iter().enumerate() {
                    let y = r.y + i as u16;
                    let set = self.cols.get(key);
                    let row = if i == *sel { l.sel[0] } else { text };
                    cv.fill(Rect { x: r.x, y, w: r.w, h: 1 }, row);
                    let tst = if i == *sel { row } else if set { on } else { dim };
                    let x = cv.put(r.x, y, if set { "[x] " } else { "[ ] " }, tst, lim);
                    let colour = match *key {
                        "kind" => "blue",
                        "mode" => "yellow",
                        "owner" => "orange",
                        "group" => "teal",
                        "size" => "cyan",
                        _ => "purple",
                    };
                    let lst = if i == *sel { row } else { tint(l, colour, text, text.bg) };
                    cv.put(x, y, lab, lst, lim);
                }
                hints(cv, l, r.x, r.y + all.len() as u16 + 1, lim, &[("Space", "tick"), ("j/k", "move"), ("Esc", "close")]);
            }
            Dialog::Owner { paths, users, groups, user, group, focus, open } => {
                let title = format!("Owner · {} item(s)", paths.len());
                let r = self.popup(cv, l, (w, h), (48, 7), &title);
                let lim = r.x + r.w;
                let fx = r.x + 8;
                let fw = lim.saturating_sub(fx);
                for (row, (lab, name)) in [("User", &users[*user]), ("Group", &groups[*group])].iter().enumerate() {
                    let y = r.y + row as u16 * 2;
                    cv.put(r.x, y, lab, text, lim);
                    let st = if *focus == row { l.sel[0] } else { l.input };
                    cv.fill(Rect { x: fx, y, w: fw, h: 1 }, st);
                    cv.put(fx + 1, y, name, st, lim.saturating_sub(2));
                    cv.put(lim.saturating_sub(2), y, "▾", st, lim);
                }
                // OK / Cancel as buttons, the focused one lit
                let lit_key = Style { fg: l.footer_key.bg, bg: l.footer_key.fg, ..l.footer_key };
                let ok = if *focus == 2 { lit_key } else { l.footer_key };
                let cancel = if *focus == 3 { lit_key } else { l.footer_key };
                let by = r.y + 4;
                let bx = r.x + (r.w.saturating_sub(18)) / 2;
                let x = cv.put(bx, by, "   OK   ", ok, lim);
                let x = cv.put(x, by, "  ", text, lim);
                cv.put(x, by, " Cancel ", cancel, lim);
                // the open drop-down, over what is below its field
                if let Some((q, sel)) = open {
                    let names = if *focus == 0 { users } else { groups };
                    let ql = q.to_lowercase();
                    let m: Vec<&String> = names.iter().filter(|n| n.to_lowercase().contains(&ql)).collect();
                    let y0 = r.y + *focus as u16 * 2 + 1;
                    let rows = (m.len() as u16).clamp(1, 10);
                    let bx = Rect { x: fx, y: y0, w: fw, h: rows + 1 };
                    cv.fill(bx, l.popup_border);
                    let qline = if q.is_empty() { "type to filter".to_string() } else { format!("› {q}") };
                    cv.put(fx + 1, y0, &qline, Style { bg: l.popup_border.bg, ..l.count }, lim);
                    let start = sel.saturating_sub(rows as usize - 1);
                    for (i, n) in m.iter().enumerate().skip(start).take(rows as usize) {
                        let y = y0 + 1 + (i - start) as u16;
                        let st = if i == *sel { l.sel[0] } else { l.rows[i % 2] };
                        cv.fill(Rect { x: fx, y, w: fw, h: 1 }, st);
                        cv.put(fx + 1, y, n, st, lim);
                    }
                    if m.is_empty() {
                        cv.put(fx + 1, y0 + 1, "(no match)", Style { bg: l.popup_border.bg, ..l.count }, lim);
                    }
                }
            }
            Dialog::Mode { paths, value, cur, base, is_dir, bits, focus } => {
                let title = format!("Permissions · {} item(s)", paths.len());
                let r = self.popup(cv, l, (w, h), (56, 11), &title);
                let lim = r.x + r.w;
                let valid = fsx::parse_mode(value, *base, *is_dir).is_ok();
                // the field (octal, or symbolic: u+x,g-w,o=r)
                let x = cv.put(r.x, r.y, "Mode  ", text, lim);
                // the field, then the mode it reads as, in the listing's colours
                let field = Rect { x, y: r.y, w: lim.saturating_sub(x).saturating_sub(13), h: 1 };
                {
                    let kind = if *is_dir { 'd' } else { '-' };
                    let m = *bits;
                    let mut ms = String::from(kind);
                    for (i, c) in "rwxrwxrwx".chars().enumerate() {
                        let bit = 0o400 >> i;
                        let mut ch = if m & bit != 0 { c } else { '-' };
                        if i == 2 && m & 0o4000 != 0 { ch = if m & 0o100 != 0 { 's' } else { 'S' }; }
                        if i == 5 && m & 0o2000 != 0 { ch = if m & 0o010 != 0 { 's' } else { 'S' }; }
                        if i == 8 && m & 0o1000 != 0 { ch = if m & 0o001 != 0 { 't' } else { 'T' }; }
                        ms.push(ch);
                    }
                    put_mode(cv, l, lim.saturating_sub(10), r.y, &ms, text, lim);
                }
                let fst = if valid { l.input } else { Style { fg: l.theme.color("red").or(l.input.fg), ..l.input } };
                cv.fill(field, fst);
                let mut fx = field.x;
                let chars: Vec<char> = value.chars().collect();
                for (i, ch) in chars.iter().enumerate() {
                    let st = if *focus == 0 && i == *cur { Style { attrs: crossterm::style::Attributes::from(Attribute::Reverse), ..fst } } else { fst };
                    let mut b = [0u8; 4];
                    fx = cv.put(fx, field.y, ch.encode_utf8(&mut b), st, lim);
                }
                if *focus == 0 && *cur >= chars.len() {
                    cv.put(fx, field.y, " ", Style { attrs: crossterm::style::Attributes::from(Attribute::Reverse), ..fst }, lim);
                }
                // the ticks
                // the ticks, coloured as `rwx` is everywhere: read yellow, write red, exec green; the rows
                // as the owner (orange), group (teal) and others (purple) columns; the special bits magenta
                let dim = Style { bg: l.popup.bg, ..l.count };
                let col = |c: u16| r.x + 10 + c * 12;
                let colours = ["yellow", "red", "green"];
                for (c, lab) in ["read", "write", "exec"].iter().enumerate() {
                    cv.put(col(c as u16), r.y + 2, lab, tint(l, colours[c], text, text.bg), lim);
                }
                let tick = |cv: &mut Canvas, x: u16, y: u16, i: usize, colour: &str| {
                    let set = bits & MODE_BITS[i] != 0;
                    let st = if *focus == i + 1 {
                        l.sel[0]
                    } else if set {
                        tint(l, colour, text, text.bg)
                    } else {
                        dim
                    };
                    cv.put(x, y, if set { "[x]" } else { "[ ]" }, st, lim);
                };
                for (row, (who, wc)) in [("owner", "orange"), ("group", "teal"), ("others", "purple")].iter().enumerate() {
                    let y = r.y + 3 + row as u16;
                    cv.put(r.x, y, who, tint(l, wc, text, text.bg), lim);
                    for (c, colour) in colours.iter().enumerate() {
                        tick(cv, col(c as u16), y, row * 3 + c, colour);
                    }
                }
                let y = r.y + 6;
                cv.put(r.x, y, "special", tint(l, "magenta", text, text.bg), lim);
                for (c, lab) in ["setuid", "setgid", "sticky"].iter().enumerate() {
                    let x = col(c as u16);
                    tick(cv, x, y, 9 + c, "magenta");
                    cv.put(x + 4, y, lab, dim, lim);
                }
                hints(cv, l, r.x, r.y + 8, lim, &[("Enter", "ok"), ("Tab", "switch"), ("Space", "tick"), ("Esc", "cancel")]);
            }
            Dialog::Input { title, value, cur, .. } => {
                let pw = (w * 2 / 3).max(40);
                let r = self.popup(cv, l, (w, h), (pw, 5), title);
                let field = Rect { x: r.x, y: r.y, w: r.w, h: 1 };
                cv.fill(field, l.input);
                // the end of a long value stays visible
                let chars: Vec<char> = value.chars().collect();
                let start = cur.saturating_sub(field.w as usize - 1);
                let mut x = field.x;
                for (i, ch) in chars.iter().enumerate().skip(start) {
                    let st = if i == *cur { Style { attrs: crossterm::style::Attributes::from(Attribute::Reverse), ..l.input } } else { l.input };
                    let mut b = [0u8; 4];
                    x = cv.put(x, field.y, ch.encode_utf8(&mut b), st, field.x + field.w);
                }
                if *cur >= chars.len() {
                    cv.put(x, field.y, " ", Style { attrs: crossterm::style::Attributes::from(Attribute::Reverse), ..l.input }, field.x + field.w);
                }
                hints(cv, l, r.x, r.y + 2, r.x + r.w, &[("Enter", "ok"), ("Esc", "cancel"), ("C-u", "clear")]);
            }
            Dialog::Confirm { title, lines, .. } => {
                let pw = lines.iter().map(|s| str_width(s) as u16).max().unwrap_or(20).max(30) + 6;
                let r = self.popup(cv, l, (w, h), (pw, lines.len() as u16 + 4), title);
                for (i, s) in lines.iter().enumerate() {
                    cv.put(r.x, r.y + i as u16, s, text, r.x + r.w);
                }
                hints(cv, l, r.x, r.y + lines.len() as u16 + 1, r.x + r.w, &[("y", "yes"), ("n", "no")]);
            }
            Dialog::Conflict { target, .. } => {
                let msg = format!("{} exists", fsx::tilde(target));
                let pw = (str_width(&msg) as u16 + 6).max(56);
                let r = self.popup(cv, l, (w, h), (pw, 6), "The target exists");
                cv.put(r.x, r.y, &msg, text, r.x + r.w);
                cv.put(r.x, r.y + 1, "upper case (O S R): the same for all the rest", Style { bg: l.popup.bg, ..l.count }, r.x + r.w);
                hints(cv, l, r.x, r.y + 3, r.x + r.w, &[("o", "overwrite"), ("s", "skip"), ("r", "keep both"), ("c", "cancel")]);
            }
            Dialog::Menu { title, items, sel } => {
                let pw = items.iter().map(|(s, _)| str_width(s) as u16).max().unwrap_or(20).max(38) + 10;
                let n = items.len().min(h as usize - 7);
                let r = self.popup(cv, l, (w, h), (pw, n as u16 + 4), title);
                let start = sel.saturating_sub(n.saturating_sub(1));
                for (i, (label, _)) in items.iter().enumerate().skip(start).take(n) {
                    let y = r.y + (i - start) as u16;
                    let st = if i == *sel { l.sel[0] } else { l.rows[i % 2] };
                    cv.fill(Rect { x: r.x, y, w: r.w, h: 1 }, st);
                    cv.put(r.x, y, label, st, r.x + r.w);
                }
                hints(cv, l, r.x, r.y + n as u16 + 1, r.x + r.w, &[("j/k", "move"), ("Enter", "pick"), ("Esc", "close")]);
            }
            Dialog::Message { title, lines } => {
                let pw = lines.iter().map(|s| str_width(s) as u16).max().unwrap_or(20).max(30) + 6;
                let n = lines.len().min(h as usize - 7);
                let r = self.popup(cv, l, (w, h), (pw, n as u16 + 4), title);
                for (i, s) in lines.iter().take(n).enumerate() {
                    cv.put(r.x, r.y + i as u16, s, text, r.x + r.w);
                }
                hints(cv, l, r.x, r.y + n as u16 + 1, r.x + r.w, &[("any key", "close")]);
            }
        }
    }

    /// A key as shown: the leader chord names the real leader (`C-c x` → `C-a x`).
    fn show_key(&self, text: &str) -> String {
        match text.strip_prefix("C-c ") {
            Some(rest) => format!("{} {rest}", self.leader_label),
            None => key_label(text),
        }
    }

    /// The help's rows: the built-in keys, then the user actions that have a key.
    fn help_items(&self) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = HELP_KEYS
            .iter()
            .map(|(k, d)| (k.replace("Ctrl-c", &self.leader_name), d.replace("Ctrl-c", &self.leader_name)))
            .collect();
        // the commands `[keys]` moved
        for (n, u, c) in &self.cmdkeys {
            if u != c {
                v.push((self.show_key(&u.text), format!("{n} (moved from {})", self.show_key(&c.text))));
            }
        }
        for a in &self.actions {
            if let Some(k) = &a.key {
                let src = if a.source == "config" { String::new() } else { format!(" ({})", a.source) };
                v.push((self.show_key(&k.text), format!("{}{src}", a.title)));
            }
        }
        v
    }

    /// The help, as the Lua plugins draw theirs: every row in its stripe's background across the whole
    /// width, the selected one brighter; j / k scroll it.
    fn draw_help(&mut self, cv: &mut Canvas, l: &Look, w: u16, h: u16) {
        let items = self.help_items();
        let kw = items.iter().map(|(k, _)| str_width(k)).max().unwrap_or(10) as u16;
        let dw = items.iter().map(|(_, d)| str_width(d)).max().unwrap_or(10) as u16;
        let r = self.popup(cv, l, (w, h), (kw + dw + 9, items.len() as u16 + 3), "Keys");
        let rows = r.h as usize;
        self.help_sel = self.help_sel.min(items.len().saturating_sub(1));
        if self.help_sel < self.help_off {
            self.help_off = self.help_sel;
        } else if self.help_sel >= self.help_off + rows {
            self.help_off = self.help_sel + 1 - rows;
        }
        for (row, (i, (k, d))) in items.iter().enumerate().skip(self.help_off).take(rows).enumerate() {
            let y = r.y + row as u16;
            let stripe = i % 2;
            let (ks, ds) = if i == self.help_sel {
                (l.sel[stripe], l.sel[stripe])
            } else {
                (l.help_key[stripe], l.help_desc[stripe])
            };
            // the whole row, padding included, in the row's background
            cv.fill(Rect { x: r.x - 1, y, w: r.w + 2, h: 1 }, ds);
            cv.fill(Rect { x: r.x - 1, y, w: kw + 2, h: 1 }, ks);
            cv.put(r.x, y, k, ks, r.x + kw);
            cv.put(r.x + kw + 2, y, d, ds, r.x + r.w);
        }
        // where in the list, on the bottom edge
        let pos = format!(" {}/{} ", self.help_sel + 1, items.len());
        let pw = str_width(&pos) as u16;
        cv.put(r.x + r.w + 1 - pw, r.y + r.h, &pos, l.counter, r.x + r.w + 1);
    }
}

const HELP_KEYS: &[(&str, &str)] = &[
        ("Tab · Ctrl-h · Ctrl-l", "the other panel · the left · the right"),
        ("j k · gg G · ↑ ↓ PgUp PgDn Home End", "move · top / bottom"),
        ("l · Enter · →", "enter a directory · open a file"),
        ("h · Backspace · ←", "up one directory"),
        ("Ctrl-f · /", "input mode: typing filters (fuzzy); Enter / Esc back"),
        ("Space · Insert", "mark and move down"),
        (".", "hidden files"),
        ("J · K", "the quick view: next / previous page (PDF, documents) or point of a video; else scroll 3 lines"),
        ("Ctrl-e · Ctrl-y", "scroll the quick view one line"),
        ("Esc", "clear the query, then the marks"),
        ("F1", "this help"),
        ("F2", "quick view"),
        ("F3", "copy (to the other panel)"),
        ("F4", "rename (several marked: all of them in the editor)"),
        ("F5", "move (to the other panel)"),
        ("F6", "edit (a directory too: the editor opens it)"),
        ("F7", "new: a file, or a directory when the name ends in /"),
        ("F8 · Delete", "to the trash (config: delete = \"direct\")"),
        ("Shift-F8", "delete for good"),
        ("F9", "the menu"),
        ("F10 · Ctrl-q", "quit"),
        ("Ctrl-j", "onto the F-key buttons: h / l choose, Enter presses, Ctrl-k / Esc back"),
        ("Ctrl-c …", "every other command: Ctrl-c, then a key (the bar shows them)"),
        ("Ctrl-c f · g", "find files · grep (lvim-fuzzy)"),
        ("Ctrl-c c · b", "go to a directory · bookmarks and recent"),
        ("Ctrl-c x", "run a shell command here"),
        ("Ctrl-c t", "the trash (there: F5 or r restore, F8 delete, e empty)"),
        ("Ctrl-c m", "permissions: ticks, or 755 / u+x,g-w,o=r in the field (each follows the other)"),
        ("Ctrl-c O", "owner and group: drop-downs of the real users and groups"),
        ("Ctrl-c z · e", "compress the marked · extract"),
        ("Ctrl-c u", "undo the last operation"),
        ("Ctrl-c a · * · + · -", "mark all · invert · mark / unmark by glob"),
        ("Ctrl-c h · l · s · S", "hidden files · long view (perms, owner, group) · sort · reverse"),
        ("Ctrl-c n · w", "a new tab (on the same directories) · close this tab"),
        ("Ctrl-c , · < · >", "rename this tab (empty: its directory's name) · move it left · right"),
        ("gt · gT · g1…g9", "the next / previous tab · that tab (a click on its name: go · middle click: close · right click: rename)"),
        ("Ctrl-c I", "the other running lvc: go to its directory, bring it here, copy / move to it"),
        ("Ctrl-c y · Y · D", "copy the path(s) · the name(s) · the panel's directory (clipboard)"),
        ("Ctrl-c L", "symbolic links to the marked in the other panel (Tab: absolute / relative)"),
        ("Ctrl-c v", "the columns: type, permissions, owner, group, size, date — each on / off"),
        ("Ctrl-c p", "swap the panels"),
        ("Ctrl-c o · i", "the dir under the cursor / this one → other panel"),
        ("Ctrl-c [ · ]", "back / forward in the history"),
        ("Ctrl-d · Ctrl-u", "half a page down / up (the quick view when it is open)"),
        ("Ctrl-c r", "reload"),
        ("Ctrl-c 1 … 0", "F1 … F10 (when the terminal keeps the F-keys)"),
        ("Ctrl-c H", "show / hide the bar of the Ctrl-c commands"),
        ("Ctrl-c q", "quit"),
    ];


#[derive(Clone, Copy)]
enum PathPart {
    Full,
    Name,
    Dir,
}

/// Also into the desktop clipboard where a tool for it is there (OSC 52 does not reach every terminal).
fn clipboard_tools(text: &str) {
    use std::io::Write;
    let cmd: Option<(&str, &[&str])> = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        Some(("wl-copy", &[]))
    } else if std::env::var_os("DISPLAY").is_some() {
        Some(("xclip", &["-selection", "clipboard"]))
    } else {
        None
    };
    if let Some((prog, args)) = cmd
        && let Ok(mut child) = std::process::Command::new(prog)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
    {
        if let Some(mut i) = child.stdin.take() {
            let _ = i.write_all(text.as_bytes());
        }
        // wl-copy / xclip stay to serve the clipboard; not waited for
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

fn symlink_title(n: usize, relative: bool) -> String {
    let kind = if relative { "relative" } else { "absolute" };
    if n == 1 {
        format!("Symlink in the other panel ({kind}; Tab: switch)")
    } else {
        format!("{n} symlinks in the other panel, named as the targets ({kind}; Tab: switch)")
    }
}

/// `to` as a path relative to the directory `from` (`../lib/x` style).
fn relative_path(from: &Path, to: &Path) -> PathBuf {
    let from = std::path::absolute(from).unwrap_or_else(|_| from.to_path_buf());
    let to = std::path::absolute(to).unwrap_or_else(|_| to.to_path_buf());
    let a: Vec<_> = from.components().collect();
    let b: Vec<_> = to.components().collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut out = PathBuf::new();
    for _ in common..a.len() {
        out.push("..");
    }
    for c in &b[common..] {
        out.push(c);
    }
    out
}

/// A tab that is not the current one.
struct TabState {
    panels: [Panel; 2],
    active: usize,
    quick: bool,
}

type DirCounts = std::cell::RefCell<std::collections::HashMap<PathBuf, (i64, usize)>>;

/// The number of entries in directory `d` (hidden ones too), cached until its mtime changes.
fn dir_count(cache: &DirCounts, d: &Path, mtime: i64) -> Option<usize> {
    let mut c = cache.borrow_mut();
    if let Some(&(m, n)) = c.get(d)
        && m == mtime
    {
        return Some(n);
    }
    let n = std::fs::read_dir(d).ok()?.count();
    c.insert(d.to_path_buf(), (mtime, n));
    Some(n)
}

/// A job whose every report (progress, a question, the end) also wakes the UI at once.
fn wake_on(job: ops::Job, wake: Sender<()>) -> ops::Job {
    let ops::Job { kind, rx: from, cancel } = job;
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for r in from {
            if tx.send(r).is_err() {
                return;
            }
            let _ = wake.send(());
        }
    });
    ops::Job { kind, rx, cancel }
}

/// A key as the bar shows it: `f5` → F5, `shift-f8` → S-F8, `ctrl-c x` → C-c x, `alt-x` → A-x.
fn key_label(text: &str) -> String {
    let t = text.trim();
    if let Some(rest) = t.strip_prefix("C-c ") {
        return format!("C-c {rest}");
    }
    let lower = t.to_ascii_lowercase();
    let mut out = String::new();
    let mut rest = lower.as_str();
    loop {
        if let Some(r) = rest.strip_prefix("ctrl-") {
            out.push_str("C-");
            rest = r;
        } else if let Some(r) = rest.strip_prefix("alt-") {
            out.push_str("A-");
            rest = r;
        } else if let Some(r) = rest.strip_prefix("shift-") {
            out.push_str("S-");
            rest = r;
        } else {
            break;
        }
    }
    if rest.starts_with('f') && rest[1..].parse::<u8>().is_ok() {
        out.push_str(&rest.to_ascii_uppercase());
    } else {
        out.push_str(rest);
    }
    out
}

/// How old a time is, as a colour: today green, this week teal, this month blue, this year purple, older
/// dim.
fn age_colour(t: i64) -> &'static str {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    match now - t {
        d if d < 86_400 => "green",
        d if d < 7 * 86_400 => "teal",
        d if d < 31 * 86_400 => "blue",
        d if d < 365 * 86_400 => "purple",
        _ => "comment",
    }
}

/// A palette colour of the theme on `bg` (the theme's `fallback` when it has no such colour).
fn tint(l: &Look, name: &str, fallback: Style, bg: Option<crossterm::style::Color>) -> Style {
    Style { fg: l.theme.color(name).or(fallback.fg), bg, ..fallback }
}

/// `drwxr-xr-x` with a colour per meaning: the type, r, w, x, the special bits, and `-` dim.
fn put_mode(cv: &mut Canvas, l: &Look, x: u16, y: u16, mode: &str, base: Style, limit: u16) -> u16 {
    let bg = base.bg;
    let dim = tint(l, "comment", base, bg);
    let mut x = x;
    for (i, ch) in mode.chars().enumerate() {
        let st = match ch {
            '-' => dim,
            'd' if i == 0 => tint(l, "blue", base, bg),
            'l' if i == 0 => tint(l, "cyan", base, bg),
            'r' => tint(l, "yellow", base, bg),
            'w' => tint(l, "red", base, bg),
            'x' => tint(l, "green", base, bg),
            's' | 'S' | 't' | 'T' => tint(l, "magenta", base, bg),
            _ if i == 0 => tint(l, "orange", base, bg),
            _ => base,
        };
        let mut b = [0u8; 4];
        x = cv.put(x, y, ch.encode_utf8(&mut b), st, limit);
    }
    x
}

/// The facts about an entry, each in its own colour: mode, owner:group, size (or DIR), date, link target.
#[allow(clippy::too_many_arguments)]
fn put_facts(cv: &mut Canvas, l: &Look, x: u16, y: u16, e: &fsx::Entry, base: Style, limit: u16, all: bool) -> u16 {
    let bg = base.bg;
    let mut x = cv.put(x, y, " ", base, limit);
    x = put_mode(cv, l, x, y, &fsx::mode_string(e), base, limit);
    x = cv.put(x, y, "  ", base, limit);
    let user = fsx::user_name(e.uid);
    let ust = if e.uid == 0 { tint(l, "red", base, bg) } else { tint(l, "orange", base, bg) };
    x = cv.put(x, y, &user, ust, limit);
    x = cv.put(x, y, ":", tint(l, "comment", base, bg), limit);
    x = cv.put(x, y, &fsx::group_name(e.gid), tint(l, "teal", base, bg), limit);
    if !all {
        return x;
    }
    x = cv.put(x, y, "  ", base, limit);
    if e.is_dir() {
        x = cv.put(x, y, "DIR", tint(l, "blue", base, bg), limit);
    } else {
        x = cv.put(x, y, &fsx::human_size(e.size), tint(l, "cyan", base, bg), limit);
    }
    x = cv.put(x, y, "  ", base, limit);
    x = cv.put(x, y, &fsx::date_string(e.mtime), tint(l, "purple", base, bg), limit);
    if let Some(t) = &e.target {
        x = cv.put(x, y, "  → ", tint(l, "comment", base, bg), limit);
        x = cv.put(x, y, t, tint(l, "cyan", base, bg), limit);
    }
    x
}

/// Key hints as the button bar draws them: " key " + " label ", one space apart, centred in [x, limit).
fn hints(cv: &mut Canvas, l: &Look, x: u16, y: u16, limit: u16, keys: &[(&str, &str)]) {
    let total: usize = keys
        .iter()
        .map(|(k, lab)| str_width(k) + str_width(lab) + 4)
        .sum::<usize>()
        + keys.len().saturating_sub(1);
    let room = limit.saturating_sub(x) as usize;
    let mut x = x + (room.saturating_sub(total) / 2) as u16;
    for (i, (k, lab)) in keys.iter().enumerate() {
        if i > 0 {
            x = cv.put(x, y, " ", l.popup, limit);
        }
        x = cv.put(x, y, &format!(" {k} "), l.footer_key, limit);
        x = cv.put(x, y, &format!(" {lab} "), l.footer_label, limit);
    }
}

