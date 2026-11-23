//! lvim-fuzzy-commander (lvc): a two-panel file manager for the terminal, in the style of Midnight Commander,
//! with the lvim look (lvim-fuzzy-ui) and the lvim-fuzzy matcher for filtering.

mod app;
pub use lvim_fuzzy_ui::archive;
mod complete;
mod config;
mod fsx;
mod ops;
mod panel;
mod instances;
mod mux;
mod plugins;
mod subshell;
mod term;
mod trash;
mod watch;

use std::path::PathBuf;

const HELP: &str = "\
lvim-fuzzy-commander (lvc) — a two-panel file manager

USAGE
    lvim-fuzzy-commander [OPTIONS] [LEFT_DIR [RIGHT_DIR]]      (`lvc` is a link to it)

OPTIONS
    --print-dir     print the active panel's directory on exit (for `cd`)
    --chooser-file FILE
                    a file chooser: Enter on a file (or on the marked ones) writes the paths to FILE, one a line,
                    and quits; quitting without choosing writes nothing (remove FILE first)
    --zsh, --bash   print a shell function `lvcd` that changes to that directory
    --plugins       list the plugins, actions and previewers, and any problem with them
    --types [open|preview]
                    which program opens (or which engine shows) which type of file, as the plugins are configured
                    for lvc, with a mark for a program that is not installed
    --install-icons put the Material icons into the shared place (~/.local/share/lvim-icons/material)
                    when they are missing there or older (lvim-icons and lvim-fuzzy share it)
    --version
    -h, --help

Keys: F1 inside. Config: ~/.config/lvim-commander/config.toml ($LVIM_COMMANDER_CONFIG).
";

fn shell_snippet() -> &'static str {
    // lvc writes the folder it was left in (or Shift-Enter's folder of the choice) to a file the function names: its stdout stays the terminal's,
    // so an editor lvc starts draws on the screen, not into a pipe
    "# lvcd: lvim-fuzzy-commander, then cd to the folder Shift-Enter chose (and, with cd_on_quit, to where it was left)\n\
lvcd() {\n  local f d rc\n  f=\"$(mktemp)\" || return\n  LVC_OUT=\"$f\" command lvim-fuzzy-commander \"$@\"\n  rc=$?\n  IFS= read -r d < \"$f\"\n  command rm -f \"$f\"\n  [ $rc -eq 0 ] && [ -d \"$d\" ] && cd -- \"$d\"\n}\n# `lvc` itself does the same, so there is one name to type\nlvc() { lvcd \"$@\"; }\n"
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // started to BE the preview plugin (a process the plugin host runs): answer as it, nothing else
    // the other instances: list them, or send one (or all) an instruction
    match args.first().map(String::as_str) {
        Some("--list") => {
            let peers = instances::peers();
            if peers.is_empty() {
                println!("no other lvim-fuzzy-commander runs");
            }
            for p in peers {
                let mark = |i: usize| if p.active == i { "*" } else { " " };
                println!("{}  {}{}  {}{}", p.pid, mark(0), p.left, mark(1), p.right);
            }
            return;
        }
        Some("--send") => {
            // lvc --send <pid|all> <instruction…>   (cd PATH, other-cd PATH, select NAME, reload, message TEXT …)
            let (Some(who), true) = (args.get(1), args.len() > 2) else {
                eprintln!("lvim-fuzzy-commander --send <pid|all> <instruction>   (cd PATH · other-cd PATH · select NAME · mark NAME · reload · message TEXT)");
                std::process::exit(2);
            };
            let line = args[2..].join(" ");
            let pids: Vec<u32> = if who == "all" {
                instances::peers().iter().map(|p| p.pid).collect()
            } else {
                who.parse().map(|p| vec![p]).unwrap_or_default()
            };
            let mut bad = false;
            for pid in pids {
                if let Err(e) = instances::send(pid, std::slice::from_ref(&line)) {
                    eprintln!("lvim-fuzzy-commander: {e}");
                    bad = true;
                }
            }
            std::process::exit(i32::from(bad));
        }
        _ => {}
    }
    let mut print_dir = false;
    let mut chooser: Option<PathBuf> = None;
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                return;
            }
            "--version" => {
                println!("lvim-fuzzy-commander {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--zsh" | "--bash" => {
                print!("{}", shell_snippet());
                return;
            }
            "--print-dir" => print_dir = true,
            "--chooser-file" => match it.next() {
                Some(f) => chooser = Some(PathBuf::from(f)),
                None => {
                    eprintln!("lvim-fuzzy-commander: --chooser-file needs a file");
                    std::process::exit(2);
                }
            },
            "--install-icons" => {
                let dir = lvim_fuzzy_ui::iconimg::shared_dir().map(|d| d.display().to_string()).unwrap_or_default();
                match lvim_fuzzy_ui::iconimg::install_shared() {
                    Ok(lvim_fuzzy_ui::iconimg::Installed::Wrote(v)) => println!("icons: Material {v} installed in {dir}"),
                    Ok(lvim_fuzzy_ui::iconimg::Installed::Current(v)) => println!("icons: {dir} already has Material {v}"),
                    Err(e) => {
                        eprintln!("lvim-fuzzy-commander: icons: {e}");
                        std::process::exit(1);
                    }
                }
                return;
            }
            // lvim-fuzzy-commander --types [open|preview]: which program opens / which engine shows which type of file, as
            // the plugins' configuration has it for lvc (their defaults, the shared file, then this config's [plugins.*])
            "--types" => {
                let (cfg, errs) = config::load();
                for e in &errs {
                    eprintln!("lvim-fuzzy-commander: {e}");
                }
                let which = args.iter().skip_while(|x| *x != "--types").nth(1).map(String::as_str).unwrap_or("open");
                let plugin: &str = match which {
                    "open" => "lvim-fuzzy-plugin-open",
                    "preview" => "lvim-fuzzy-plugin-preview",
                    other => {
                        eprintln!("lvim-fuzzy-commander: --types open | preview, not `{other}`");
                        std::process::exit(2);
                    }
                };
                let (wake, _rx) = std::sync::mpsc::channel();
                let pl = lvim_fuzzy_plugin::Previewer::load(&config::plugin_roots(), "lvim-fuzzy-commander", &cfg.plugins, wake);
                if pl.host().def(plugin).is_none() {
                    eprintln!("lvim-fuzzy-commander: the plugin `{plugin}` is not installed (looked in {})", config::plugins_dir().display());
                    std::process::exit(1);
                }
                println!("{plugin} — for lvc");
                let conf = pl.host().config_of(plugin);
                let builtin = lvim_fuzzy_plugin::types::builtin_engines(&conf);
                let builtin: Vec<(&str, Option<&str>)> = builtin.iter().map(|(n, p)| (n.as_str(), p.as_deref())).collect();
                print!("{}", lvim_fuzzy_plugin::types::list(&conf, &builtin, &lvim_fuzzy_plugin::have));
                return;
            }
            "--plugins" => {
                let (cfg, mut errs) = config::load();
                let (plugins, actions, previewers, perrs) = plugins::load_plugins(&config::plugin_roots());
                errs.extend(perrs);
                let mut all: Vec<plugins::Action> = cfg.actions.iter().chain(actions.iter()).cloned().collect();
                plugins::apply_key_overrides(&mut all, &cfg.keys, config::is_command, &mut errs);
                println!("plugins ({}, and the old {}):", config::plugins_dir().display(), config::legacy_plugins_dir().display());
                for p in &plugins {
                    println!("  {}  [{}]  {}  ({})", p.name, p.runtime.name(), p.description, p.dir.display());
                    // the state of the runtime: a library that does not load, a script that does not compile
                    match lvim_fuzzy_plugin::status(p) {
                        Ok(st) if !st.is_empty() => println!("      {st}"),
                        Ok(_) => {}
                        Err(e) => errs.push(e),
                    }
                }
                println!("actions:");
                for a in all.iter() {
                    let key = a.key.as_ref().map(|k| k.text.clone()).unwrap_or_else(|| "-".into());
                    let what = a.call.as_ref().map(|c| format!("call {c}")).unwrap_or_else(|| format!("{:?}", a.mode));
                    println!("  {:<10} {:<16} {:<28} {}  [{}]", key, a.id, a.title, what, a.source);
                }
                println!("previewers:");
                for p in cfg.previewers.iter().chain(previewers.iter()) {
                    println!("  {:<12} {}  {}", p.name, p.extensions.join(","), p.command);
                }
                for e in &errs {
                    eprintln!("lvim-fuzzy-commander: {e}");
                }
                std::process::exit(if errs.is_empty() { 0 } else { 1 });
            }
            s if s.starts_with('-') => {
                eprintln!("lvim-fuzzy-commander: unknown option {s} (--help)");
                std::process::exit(2);
            }
            s => dirs.push(PathBuf::from(s)),
        }
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| fsx::home());
    let abs = |p: PathBuf| {
        let p = if p.is_absolute() { p } else { cwd.join(p) };
        std::fs::canonicalize(&p).unwrap_or(p)
    };
    let explicit_dirs = !dirs.is_empty() || chooser.is_some();
    let left = dirs.first().cloned().map(abs).unwrap_or_else(|| cwd.clone());
    let right = dirs.get(1).cloned().map(abs).unwrap_or_else(|| left.clone());
    for d in [&left, &right] {
        if !d.is_dir() {
            eprintln!("lvim-fuzzy-commander: not a directory: {}", d.display());
            std::process::exit(2);
        }
    }

    // The configuration is checked BEFORE anything is drawn: a problem in the config, a plugin that is broken, a key that two things want —
    // lvc does not start, it says what is wrong (a plugin that takes a key somebody has is an error, not a quiet winner).
    let (cfg, mut errors) = config::load();
    // its own themes (lvim-colorscheme's extras/lvim_commander) first, then lvim-fuzzy's
    let mut dirs = vec![config::themes_dir()];
    dirs.extend(lvim_fuzzy_ui::lvim::theme_dirs());
    let (theme, note) = lvim_fuzzy_ui::lvim::resolve_theme_in(&dirs, cfg.theme.as_deref());
    if let Some(n) = note {
        errors.push(n);
    }
    let look = lvim_fuzzy_ui::lvim::Look::new(theme);
    // a plugin is a program of its own, installed by its own `scripts/install.sh` (or `lvim-fuzzy-plugin install`): lvc carries none of them
    for (name, t) in &cfg.plugins {
        if t.as_table().is_some_and(|t| t.contains_key("install")) {
            errors.push(format!("[plugins.{name}] install is gone: a plugin is a program of its own — install it with its `scripts/install.sh` (it builds the program and writes the plugin into {}), and delete this line", config::plugins_dir().display()));
        }
    }
    let (p_defs, p_actions, p_previewers, perrs) = plugins::load_plugins(&config::plugin_roots());
    errors.extend(perrs);
    // the config's actions first, then the plugins'
    let mut actions = cfg.actions.clone();
    actions.extend(p_actions);
    // the plugins are loaded: the app's `[keys]` may now move their keys
    plugins::apply_key_overrides(&mut actions, &cfg.keys, config::is_command, &mut errors);
    // where every button is: the app's commands and the plugins' buttons in one map, with what the config moved; nobody takes what another has
    let mut moves = cfg.positions.clone();
    moves.extend(plugins::plugin_moves(&cfg.plugins, &p_defs, &config::plugins_dir(), &mut errors));
    let (keymap, kerrs) = app::build_keymap(&actions, &moves, cfg.delete);
    errors.extend(kerrs);
    // the settings of the plugins: their defaults, the shared override file, lvc's own [plugins.<name>]
    {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut host = lvim_fuzzy_plugin::Host::new(p_defs.clone(), tx);
        errors.extend(host.configure_all(&config::plugins_dir(), &cfg.plugins));
    }
    if !errors.is_empty() {
        eprintln!("lvim-fuzzy-commander: not started — {} problem{} in the configuration:", errors.len(), if errors.len() == 1 { "" } else { "s" });
        for (i, e) in errors.iter().enumerate() {
            let mut lines = e.lines();
            eprintln!("  {}. {}", i + 1, lines.next().unwrap_or(""));
            for l in lines {
                eprintln!("     {l}");
            }
        }
        eprintln!("Fix them and start again (`lvc --plugins` lists what is loaded).");
        std::process::exit(2);
    }
    let notes: Vec<String> = Vec::new();
    let mut term = match term::Term::open() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("lvim-fuzzy-commander: cannot open the terminal: {e}");
            std::process::exit(1);
        }
    };
    let mut previewers = cfg.previewers.clone();
    previewers.extend(p_previewers);
    // icons as images: in raw mode, before input is read (it may ask the terminal its cell size)
    lvim_fuzzy_ui::iconimg::init(cfg.icons, cfg.zellij_images);
    // the previews' disk cache kept under 500 MB (off the way: a thread)
    std::thread::spawn(|| lvim_fuzzy_ui::media::prune_cache(500 << 20));
    std::thread::spawn(app::prune_arcs);
    let mut app = app::App::new(cfg, look, left, right, notes, actions, previewers, p_defs);
    app.set_columns(plugins::load_columns(&config::plugin_roots()));
    app.set_keymap(keymap);
    // started with no directory: the tabs open as they were left; with directories given, they are what you asked for
    if !explicit_dirs {
        app.restore_tabs(&config::load_state());
    }
    if let Some(f) = chooser {
        app.set_chooser(f);
    }
    let out = app.run(&mut term);
    drop(term);
    match out {
        Ok(o) => {
            // what cd chose to print; else, with --print-dir, the directory it was left in
            let text = if !o.print.is_empty() {
                o.print.iter().map(|l| format!("{l}\n")).collect::<String>()
            } else if (print_dir || (o.cd_on_quit && std::env::var_os("LVC_OUT").is_some())) && let Some(d) = o.dir {
                format!("{}\n", d.display())
            } else {
                String::new()
            };
            // the shell function (`lvc --zsh`) asks for a file (`LVC_OUT`): its stdout must stay the terminal's, or the programs lvc starts (an editor)
            // would draw into a pipe
            match std::env::var_os("LVC_OUT") {
                Some(f) => {
                    if let Err(e) = std::fs::write(&f, &text) {
                        eprintln!("lvim-fuzzy-commander: {}: {e}", std::path::Path::new(&f).display());
                    }
                }
                None => print!("{text}"),
            }
        }
        Err(e) => {
            eprintln!("lvim-fuzzy-commander: {e}");
            std::process::exit(1);
        }
    }
}
