//! Zellij: the Ctrl-h / j / k / l navigation of lvim-winnav.
//!
//! The lvim-winnav Zellij plugin hands those keys to a pane only when its title is the mark Neovim sets
//! (`@lvim_nvim`); any other pane is moved away from. lvc sets the same mark on its own pane while it runs,
//! so Ctrl-h / Ctrl-l choose its panels and Ctrl-j / Ctrl-k reach its button bar, and at its edges it moves
//! the Zellij focus itself — as Neovim does at the edge of its windows.

use std::process::{Command, Stdio};

const MARK: &str = "@lvim_nvim";

fn pane() -> Option<String> {
    std::env::var("ZELLIJ_PANE_ID").ok().filter(|s| !s.is_empty()).map(|id| format!("terminal_{id}"))
}

fn in_zellij() -> bool {
    std::env::var_os("ZELLIJ").is_some() && std::env::var_os("TMUX").is_none()
}

fn zellij(args: &[&str]) {
    let _ = Command::new("zellij")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Mark (or unmark) our pane, so the plugin forwards the nav keys to lvc.
pub fn mark(on: bool) {
    if !in_zellij() {
        return;
    }
    let target = pane();
    let mut args: Vec<&str> = vec!["action", if on { "rename-pane" } else { "undo-rename-pane" }];
    if let Some(t) = &target {
        args.push("-p");
        args.push(t);
    }
    if on {
        args.push(MARK);
    }
    zellij(&args);
}

/// Past lvc's edge: the neighbouring Zellij pane (a no-op at the outermost one). Returns whether it was
/// tried (outside Zellij there is nothing to move to).
pub fn move_focus(dir: &str) -> bool {
    if !in_zellij() {
        return false;
    }
    zellij(&["action", "move-focus", dir]);
    true
}
