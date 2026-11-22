//! git in lvc: the keys of group `x`, which of them are possible now, and what each one does. The reading and the doing is the git
//! plugin's library (`lvim-fuzzy-plugin-git`), on this machine or — for a panel on an SSH server — over `ssh`; here are the panels' marks,
//! the targets (the marked entries, or the one under the cursor), the dialogs, and the background threads that keep the interface from waiting.

use super::*;
use lvim_fuzzy_plugin_git::ops::{self, Repo};
use lvim_fuzzy_plugin_git::status::{Kind, Mark};

type Cmd = (&'static str, &'static str, &'static str);

/// A sub-group of group `x`: its key, its label, and its commands (key, name, label — 1–2 words).
pub(super) struct GitSub {
    pub key: char,
    pub label: &'static str,
    pub items: &'static [Cmd],
}

/// Group `x`: the sub-groups (`x a` files, `x c` commit …) …
pub(super) const GITSUBS: &[GitSub] = &[
    GitSub {
        key: 'a',
        label: "files",
        items: &[
            ("a", "git-stage", "add"),
            ("u", "git-unstage", "unstage"),
            ("d", "git-discard", "discard"),
            ("r", "git-remove", "remove"),
            ("i", "git-ignore", "ignore"),
            ("m", "git-rename", "move"),
            ("A", "git-stage-all", "add all"),
            ("U", "git-unstage-all", "unstage all"),
        ],
    },
    GitSub { key: 'c', label: "commit", items: &[("c", "git-commit", "commit"), ("a", "git-amend", "amend")] },
    GitSub {
        key: 'd',
        label: "inspect",
        items: &[("d", "git-diff", "diff"), ("l", "git-log", "log"), ("L", "git-log-all", "repo log"), ("b", "git-blame", "blame")],
    },
    GitSub {
        key: 'b',
        label: "branch",
        items: &[("l", "git-branches", "branches"), ("n", "git-new-branch", "new"), ("m", "git-merge", "merge"), ("r", "git-rebase", "rebase")],
    },
    GitSub { key: 'p', label: "sync", items: &[("f", "git-fetch", "fetch"), ("p", "git-pull", "pull"), ("P", "git-push", "push")] },
    GitSub { key: 's', label: "stash", items: &[("s", "git-stash", "stash"), ("l", "git-stashes", "stashes")] },
    GitSub { key: 't', label: "tags", items: &[("l", "git-tags", "tags"), ("n", "git-new-tag", "new tag")] },
    GitSub { key: 'S', label: "modules", items: &[("l", "git-modules", "submodules"), ("u", "git-modules-update", "update"), ("U", "git-modules-update-all", "update all")] },
    // only while a merge / rebase / cherry-pick / revert is in progress
    GitSub { key: 'm', label: "in progress", items: &[("c", "git-continue", "continue"), ("a", "git-abort", "abort"), ("r", "git-resolve", "resolve")] },
];

/// … and the commands right in the group.
pub(super) const GITDIRECT: &[Cmd] = &[("R", "git-refresh", "refresh")];

/// The label of a git command.
pub(super) fn git_label(name: &str) -> &'static str {
    GITSUBS.iter().flat_map(|s| s.items.iter()).chain(GITDIRECT.iter()).find(|(_, n, _)| *n == name).map_or("", |c| c.2)
}

/// A target of a git command: an entry of the panel with its path from the work tree's top and its mark.
pub(super) struct GitItem {
    pub rel: String,
    pub name: String,
    pub dir: bool,
    pub mark: Option<Mark>,
}

impl GitItem {
    /// Changed in the work tree and not in the index (an untracked file counts), and not merely ignored.
    fn unstaged(&self) -> bool {
        self.mark.is_some_and(|m| m.y != ' ' && m.y != '!')
    }

    fn staged(&self) -> bool {
        self.mark.is_some_and(|m| m.x != ' ')
    }

    fn untracked(&self) -> bool {
        self.mark.is_some_and(|m| m.kind == Kind::Untracked)
    }

    fn conflicted(&self) -> bool {
        self.mark.is_some_and(|m| m.kind == Kind::Conflict)
    }

    /// Known to git (clean, or changed): not untracked, not ignored.
    fn tracked(&self) -> bool {
        self.mark.is_none_or(|m| !matches!(m.kind, Kind::Untracked | Kind::Ignored))
    }
}

/// What a diff on the screen is a diff of (so a hunk of it can be staged, unstaged or given up, and the diff read again).
#[derive(Clone, Debug)]
pub(super) struct DiffView {
    pub repo: Repo,
    pub path: String,
    pub staged: bool,
    pub untracked: bool,
    /// Which hunk is the current one.
    pub hunk: usize,
}

impl App {
    /// Is the panel in a git work tree (and git is on)?
    pub(super) fn git_here(&self) -> bool {
        self.cfg.git && self.p().git.is_some()
    }

    /// The marked entries of the panel, or the one under the cursor.
    pub(super) fn git_items(&self) -> Vec<GitItem> {
        let p = self.p();
        let Some(g) = &p.git else { return Vec::new() };
        let names: Vec<String> = if p.marked.is_empty() {
            p.current().filter(|e| !e.is_parent()).map(|e| vec![e.name.clone()]).unwrap_or_default()
        } else {
            p.marked.iter().cloned().collect()
        };
        names
            .into_iter()
            .filter_map(|n| {
                let e = p.entries.iter().find(|e| e.name == n)?;
                let dir = e.is_dir();
                let rel = if p.git_base.is_empty() { n.clone() } else { format!("{}/{}", p.git_base, n) };
                Some(GitItem { mark: g.mark_of(&rel, dir), rel, name: n, dir })
            })
            .collect()
    }

    /// Does the command make sense now? A button that cannot be used is not shown.
    pub(super) fn git_avail(&self, name: &str) -> bool {
        let Some(g) = self.p().git.clone() else { return false };
        let items = self.git_items();
        let busy = g.state.is_some();
        match name {
            "git-stage" | "git-discard" => items.iter().any(GitItem::unstaged),
            "git-unstage" => items.iter().any(GitItem::staged),
            "git-stage-all" => g.changed + g.untracked + g.conflicted > 0,
            "git-unstage-all" | "git-commit" => g.staged > 0,
            "git-amend" => g.oid != "(initial)",
            "git-diff" => items.iter().any(|i| i.mark.is_some_and(|m| m.kind != Kind::Ignored)),
            "git-blame" => items.len() == 1 && !items[0].dir && items[0].tracked() && g.oid != "(initial)",
            "git-pull" => g.upstream.is_some(),
            "git-stash" => g.dirty(),
            "git-ignore" => items.iter().any(GitItem::untracked),
            "git-remove" => items.iter().any(GitItem::tracked),
            "git-rename" => items.len() == 1 && items[0].tracked(),
            // starting a merge or a rebase while one is in progress makes no sense
            "git-merge" | "git-rebase" => !busy && g.oid != "(initial)",
            "git-continue" | "git-abort" => busy,
            "git-resolve" => items.iter().any(GitItem::conflicted),
            _ => true,
        }
    }

    /// Is the sub-group `key` of group `x` there: has it a button that can be used?
    pub(super) fn git_sub_avail(&self, s: &GitSub) -> bool {
        s.items.iter().any(|(_, n, _)| self.git_avail(n))
    }

    /// The repository of the active panel: where git runs for it.
    fn git_repo(&self) -> Option<Repo> {
        self.p().git.as_ref().and_then(|g| g.repo.clone())
    }

    /// Run `f` in the background; what it says (or why it failed) comes back as a toast, and the panels read git again.
    pub(super) fn git_spawn<F: FnOnce() -> Result<String, String> + Send + 'static>(&mut self, label: &str, f: F) {
        let (tx, wake, label) = (self.git_tx.clone(), self.wake_tx.clone(), label.to_string());
        self.toast(format!("{label}…"), false);
        std::thread::spawn(move || {
            let result = f();
            let _ = tx.send(GitMsg::Done { label, result });
            let _ = wake.send(());
        });
    }

    /// Read a text in the background and show it on the whole screen.
    pub(super) fn git_text<F: FnOnce() -> Result<String, String> + Send + 'static>(&mut self, title: &str, f: F) {
        let (tx, wake, title) = (self.git_tx.clone(), self.wake_tx.clone(), title.to_string());
        std::thread::spawn(move || {
            let result = f();
            let _ = tx.send(GitMsg::Text { title, result });
            let _ = wake.send(());
        });
    }

    /// Read a diff in the background and show it, with its hunks (`keep`: the hunk that is the current one afterwards).
    pub(super) fn git_diff_view(&mut self, d: DiffView, keep: usize) {
        let (tx, wake) = (self.git_tx.clone(), self.wake_tx.clone());
        std::thread::spawn(move || {
            let result = ops::diff(&d.repo, Some(&d.path), d.staged, d.untracked);
            let _ = tx.send(GitMsg::Diff { view: DiffView { hunk: keep, ..d }, result });
            let _ = wake.send(());
        });
    }

    /// Read a list in the background and offer it to pick from.
    fn git_list<F: FnOnce() -> Result<Vec<(String, MenuItem)>, String> + Send + 'static>(&mut self, title: &str, f: F) {
        let (tx, wake, title) = (self.git_tx.clone(), self.wake_tx.clone(), title.to_string());
        std::thread::spawn(move || {
            let result = f();
            let _ = tx.send(GitMsg::List { title, result });
            let _ = wake.send(());
        });
    }

    /// The command of group `x`.
    pub(super) fn git_op(&mut self, name: &str) {
        let Some(repo) = self.git_repo() else {
            self.toast("not in a git work tree", true);
            return;
        };
        let Some(g) = self.p().git.clone() else { return };
        if !self.git_avail(name) {
            self.toast(format!("{}: nothing to do here", git_label(name)), false);
            return;
        }
        let items = self.git_items();
        let rels = |f: &dyn Fn(&GitItem) -> bool| -> Vec<String> { items.iter().filter(|i| f(i)).map(|i| i.rel.clone()).collect() };
        let state = g.state.clone();
        match name {
            "git-stage" => {
                let paths = rels(&GitItem::unstaged);
                self.git_spawn("git add", move || ops::stage(&repo, &paths).map(|_| String::new()));
            }
            "git-unstage" => {
                let paths = rels(&GitItem::staged);
                self.git_spawn("git unstage", move || ops::unstage(&repo, &paths).map(|_| String::new()));
            }
            "git-discard" => {
                let tracked = rels(&|i| i.unstaged() && !i.untracked());
                let untracked = rels(&GitItem::untracked);
                let mut lines = vec!["The changes of these are given up for good:".to_string()];
                lines.extend(tracked.iter().chain(&untracked).take(8).map(|p| format!("  {p}")));
                if tracked.len() + untracked.len() > 8 {
                    lines.push(format!("  … and {} more", tracked.len() + untracked.len() - 8));
                }
                if !untracked.is_empty() {
                    lines.push("Untracked ones are deleted.".into());
                }
                self.dialog = Some(Dialog::Confirm { title: "Discard changes".into(), lines, action: ConfirmAction::GitDiscard { repo, tracked, untracked } });
            }
            "git-stage-all" => self.git_spawn("git add all", move || ops::stage_all(&repo).map(|_| String::new())),
            "git-unstage-all" => self.git_spawn("git unstage all", move || ops::unstage_all(&repo).map(|_| String::new())),
            "git-commit" => self.input("Commit message", "", InputAction::GitCommit { repo, amend: false }),
            "git-amend" => {
                let last = ops::last_message(&repo);
                self.input("Amend the last commit — its message", &last, InputAction::GitCommit { repo, amend: true });
            }
            "git-diff" => {
                let Some(i) = items.first() else { return };
                let (path, untracked, staged) = (i.rel.clone(), i.untracked(), !i.unstaged() && i.staged());
                self.git_diff_view(DiffView { repo, path, staged, untracked, hunk: 0 }, 0);
            }
            "git-blame" => {
                let Some(i) = items.first() else { return };
                let (path, title) = (i.rel.clone(), format!("blame {}", i.rel));
                self.git_text(&title, move || ops::blame(&repo, &path));
            }
            "git-log" | "git-log-all" => {
                let path = (name == "git-log").then(|| items.first().map(|i| i.rel.clone()).unwrap_or_else(|| self.p().git_base.clone())).filter(|p| !p.is_empty());
                let title = match &path {
                    Some(p) => format!("Log · {p}"),
                    None => "Log".to_string(),
                };
                self.git_list(&title, move || {
                    let commits = ops::log(&repo, path.as_deref(), 300)?;
                    Ok(commits
                        .into_iter()
                        .map(|c| (format!("{}  {}  {:<14}  {}", c.short, c.date, truncate(&c.author, 14), c.subject), MenuItem::GitCommit { repo: repo.clone(), hash: c.hash, subject: c.subject }))
                        .collect())
                });
            }
            "git-branches" => {
                self.git_list("Branches · Enter switch · d delete · n new · m merge · r rebase", move || {
                    let branches = ops::branches(&repo)?;
                    let w = branches.iter().map(|b| b.name.chars().count()).max().unwrap_or(0);
                    Ok(branches
                        .into_iter()
                        .map(|b| {
                            let up = b.upstream.as_deref().map(|u| format!("  → {u}")).unwrap_or_default();
                            let label = format!("{} {:<w$}{}  {}", if b.current { "●" } else { " " }, b.name, up, b.subject);
                            (label, MenuItem::GitBranch { repo: repo.clone(), name: b.name, remote: b.remote, current: b.current })
                        })
                        .collect())
                });
            }
            "git-new-branch" => self.input("New branch (switched to)", "", InputAction::GitNewBranch { repo }),
            "git-merge" | "git-rebase" => {
                let merge = name == "git-merge";
                let title = if merge { "Merge into the current branch — pick one" } else { "Rebase the current branch onto — pick one" };
                self.git_list(title, move || {
                    Ok(ops::branches(&repo)?
                        .into_iter()
                        .filter(|b| !b.current)
                        .map(|b| {
                            let item = if merge { MenuItem::GitMerge { repo: repo.clone(), name: b.name.clone() } } else { MenuItem::GitRebase { repo: repo.clone(), name: b.name.clone() } };
                            (format!("{}  {}", b.name, b.subject), item)
                        })
                        .collect())
                });
            }
            "git-continue" => {
                let Some(st) = state else { return };
                let label = format!("git {st} --continue");
                self.git_spawn(&label, move || ops::continue_(&repo, &st));
            }
            "git-abort" => {
                let Some(st) = state else { return };
                let label = format!("git {st} --abort");
                self.dialog = Some(Dialog::Confirm {
                    title: "Give up".into(),
                    lines: vec![format!("Abort the {st} in progress? Its changes are lost.")],
                    action: ConfirmAction::GitAbort { repo, state: st, label },
                });
            }
            "git-resolve" => {
                let paths = rels(&GitItem::conflicted);
                let items = vec![
                    ("Use ours (this branch's side)".to_string(), MenuItem::GitResolve { repo: repo.clone(), paths: paths.clone(), how: 0 }),
                    ("Use theirs (the other side)".to_string(), MenuItem::GitResolve { repo: repo.clone(), paths: paths.clone(), how: 1 }),
                    ("Mark as resolved (the file as it is now)".to_string(), MenuItem::GitResolve { repo, paths, how: 2 }),
                ];
                self.dialog = Some(Dialog::Menu { title: "Resolve the conflict".into(), items, sel: 0 });
            }
            "git-fetch" => self.git_spawn("git fetch", move || ops::fetch(&repo)),
            "git-pull" => self.git_spawn("git pull", move || ops::pull(&repo)),
            "git-push" => self.git_spawn("git push", move || ops::push(&repo)),
            "git-stash" => self.input("Stash the changes — a message (or none)", "", InputAction::GitStash { repo }),
            "git-stashes" => {
                self.git_list("Stashes · Enter pop · a apply · v show · d drop", move || {
                    Ok(ops::stash_list(&repo)?.into_iter().map(|(r, s)| (format!("{r}  {s}"), MenuItem::GitStash { repo: repo.clone(), name: r })).collect())
                });
            }
            "git-tags" => {
                self.git_list("Tags · Enter show · c switch · p push · d delete", move || {
                    Ok(ops::tags(&repo)?.into_iter().map(|t| (format!("{:<20} {}  {}", t.name, t.hash, t.subject), MenuItem::GitTag { repo: repo.clone(), name: t.name })).collect())
                });
            }
            "git-new-tag" => self.input("New tag at the head — a name (and, after a space, a message: an annotated tag)", "", InputAction::GitTag { repo }),
            "git-modules" => {
                self.git_list("Submodules · Enter go there · u update · U update all", move || {
                    Ok(ops::submodules(&repo)?
                        .into_iter()
                        .map(|s| {
                            let mark = match s.state {
                                '-' => "not initialized",
                                '+' => "other commit",
                                'U' => "conflict",
                                _ => "",
                            };
                            (format!("{:<28} {}  {} {}", s.path, &s.hash[..s.hash.len().min(8)], s.describe, mark), MenuItem::GitModule { repo: repo.clone(), path: s.path })
                        })
                        .collect())
                });
            }
            "git-modules-update" | "git-modules-update-all" => {
                let path = (name == "git-modules-update").then(|| items.first().map(|i| i.rel.clone())).flatten();
                self.git_spawn("git submodule update", move || ops::submodule_update(&repo, path.as_deref()));
            }
            "git-ignore" => {
                let paths: Vec<(String, bool)> = items.iter().filter(|i| i.untracked()).map(|i| (i.rel.clone(), i.dir)).collect();
                self.git_spawn("git ignore", move || {
                    for (p, dir) in &paths {
                        ops::ignore(&repo, p, *dir)?;
                    }
                    Ok(format!("{} added to .gitignore", paths.len()))
                });
            }
            "git-remove" => {
                let paths = rels(&GitItem::tracked);
                let mut lines = vec!["These are removed from the folder and from git:".to_string()];
                lines.extend(paths.iter().take(8).map(|p| format!("  {p}")));
                if paths.len() > 8 {
                    lines.push(format!("  … and {} more", paths.len() - 8));
                }
                self.dialog = Some(Dialog::Confirm { title: "git rm".into(), lines, action: ConfirmAction::GitRemove { repo, paths } });
            }
            "git-rename" => {
                let Some(i) = items.first() else { return };
                let base = self.p().git_base.clone();
                self.input("git mv — the new name", &i.name.clone(), InputAction::GitMove { repo, base, from: i.rel.clone() });
            }
            "git-refresh" => self.git_refresh(),
            _ => {}
        }
    }

    /// Keys of a git list besides Enter and the moves. Returns whether the key was taken.
    pub(super) fn git_menu_key(&mut self, it: &MenuItem, c: char) -> bool {
        match (it, c) {
            (MenuItem::GitBranch { repo, name, remote, current }, 'd') => {
                if *current || *remote {
                    self.toast(if *current { "the branch you are on stays" } else { "only local branches are deleted here" }, true);
                    return false;
                }
                self.dialog = Some(Dialog::Confirm {
                    title: "Delete a branch".into(),
                    lines: vec![format!("Delete the branch `{name}` (its commits that are in no other branch are lost)?")],
                    action: ConfirmAction::GitDeleteBranch { repo: repo.clone(), name: name.clone() },
                });
                true
            }
            (MenuItem::GitBranch { repo, .. }, 'n') => {
                self.input("New branch (switched to)", "", InputAction::GitNewBranch { repo: repo.clone() });
                true
            }
            (MenuItem::GitBranch { repo, name, current: false, .. }, 'm') => {
                let (repo, name) = (repo.clone(), name.clone());
                let label = format!("git merge {name}");
                self.git_spawn(&label, move || ops::merge(&repo, &name));
                true
            }
            (MenuItem::GitBranch { repo, name, current: false, .. }, 'r') => {
                let (repo, name) = (repo.clone(), name.clone());
                let label = format!("git rebase {name}");
                self.git_spawn(&label, move || ops::rebase(&repo, &name));
                true
            }
            (MenuItem::GitStash { repo, name }, 'a') => {
                let (repo, name) = (repo.clone(), name.clone());
                self.git_spawn("git stash apply", move || ops::stash_apply(&repo, &name).map(|_| String::new()));
                true
            }
            (MenuItem::GitStash { repo, name }, 'd') => {
                self.dialog = Some(Dialog::Confirm {
                    title: "Drop a stash".into(),
                    lines: vec![format!("Drop {name} for good?")],
                    action: ConfirmAction::GitDropStash { repo: repo.clone(), name: name.clone() },
                });
                true
            }
            (MenuItem::GitStash { repo, name }, 'v') => {
                let (repo, name) = (repo.clone(), name.clone());
                let title = format!("stash {name}");
                self.git_text(&title, move || ops::show(&repo, &name));
                true
            }
            (MenuItem::GitTag { repo, name }, 'd') => {
                let (repo, name) = (repo.clone(), name.clone());
                self.git_spawn("git tag -d", move || ops::delete_tag(&repo, &name).map(|_| String::new()));
                true
            }
            (MenuItem::GitTag { repo, name }, 'c') => {
                let (repo, name) = (repo.clone(), name.clone());
                self.git_spawn("git switch", move || ops::checkout_tag(&repo, &name).map(|_| format!("at {name}")));
                true
            }
            (MenuItem::GitTag { repo, name }, 'p') => {
                let (repo, name) = (repo.clone(), name.clone());
                self.git_spawn("git push tag", move || ops::push_tag(&repo, &name));
                true
            }
            (MenuItem::GitModule { repo, path }, 'u') => {
                let (repo, path) = (repo.clone(), path.clone());
                self.git_spawn("git submodule update", move || ops::submodule_update(&repo, Some(&path)));
                true
            }
            (MenuItem::GitModule { repo, .. }, 'U') => {
                let repo = repo.clone();
                self.git_spawn("git submodule update", move || ops::submodule_update(&repo, None));
                true
            }
            _ => false,
        }
    }

    /// Enter on an item of a git list that is more than a command to run.
    pub(super) fn git_menu_pick(&mut self, it: MenuItem) {
        match it {
            MenuItem::GitMerge { repo, name } => {
                let label = format!("git merge {name}");
                self.git_spawn(&label, move || ops::merge(&repo, &name));
            }
            MenuItem::GitRebase { repo, name } => {
                let label = format!("git rebase {name}");
                self.git_spawn(&label, move || ops::rebase(&repo, &name));
            }
            MenuItem::GitResolve { repo, paths, how } => {
                self.git_spawn("git resolve", move || match how {
                    0 | 1 => ops::resolve(&repo, &paths, how == 1).map(|_| String::new()),
                    _ => ops::stage(&repo, &paths).map(|_| String::new()),
                });
            }
            MenuItem::GitTag { repo, name } => {
                let title = format!("tag {name}");
                self.git_text(&title, move || ops::show(&repo, &name));
            }
            MenuItem::GitModule { repo, path } => {
                // into the folder of the submodule, in the panel that is open
                let dir = std::path::PathBuf::from(&repo.root).join(&path);
                self.pm().go(Place::Dir(dir));
            }
            _ => {}
        }
    }

    // ----- the hunks of a diff on the screen -----

    /// The diff on the screen, when what is on the screen is one: its lines' hunks and where it came from.
    fn diff_on_screen(&self) -> Option<(DiffView, Vec<ops::Hunk>)> {
        let t = self.view_text.as_ref()?;
        let d = t.diff.clone()?;
        let hs = ops::hunks(&t.lines);
        Some((d, hs))
    }

    /// The buttons of a diff's hunks that make sense now (key, label).
    pub(super) fn hunk_buttons(&self) -> Vec<(&'static str, &'static str, char)> {
        let Some((d, hs)) = self.diff_on_screen() else { return Vec::new() };
        let mut v = vec![("Tab", if d.staged { "unstaged" } else { "staged" }, '\t')];
        if hs.is_empty() {
            return v;
        }
        v.insert(0, ("[", "prev hunk", '['));
        v.insert(0, ("]", "next hunk", ']'));
        if d.staged {
            v.push(("u", "unstage hunk", 'u'));
        } else {
            v.push(("s", "stage hunk", 's'));
            if !d.untracked {
                v.push(("d", "discard hunk", 'd'));
            }
        }
        v
    }

    /// A key of a diff on the screen: hunks to move among, stage, unstage, give up. Returns whether it was taken.
    pub(super) fn hunk_key(&mut self, c: char) -> bool {
        let Some((d, hs)) = self.diff_on_screen() else { return false };
        let n = hs.len();
        let go = |s: &mut App, to: usize| {
            if let Some(t) = s.view_text.as_mut() {
                if let Some(d) = t.diff.as_mut() {
                    d.hunk = to;
                    t.title = format!("{} {}  ·  hunk {}/{}", if d.staged { "staged diff" } else { "diff" }, d.path, to + 1, hs.len());
                }
                s.pv_off = hs[to].start;
            }
        };
        match c {
            ']' | 'n' if n > 0 => go(self, (d.hunk + 1).min(n - 1)),
            '[' | 'p' if n > 0 => go(self, d.hunk.saturating_sub(1)),
            '\t' => {
                // the other side of the same file
                self.git_diff_view(DiffView { staged: !d.staged, untracked: false, hunk: 0, ..d }, 0);
            }
            's' if !d.staged => {
                if d.untracked {
                    // a new file has no hunks to choose: the whole file goes in
                    let (repo, path) = (d.repo.clone(), d.path.clone());
                    self.git_spawn("git add", move || ops::stage(&repo, &[path]).map(|_| String::new()));
                    self.close_viewer();
                } else if n > 0 {
                    self.hunk_do(d, &hs, ops::HunkDo::Stage);
                }
            }
            'u' if d.staged && n > 0 => self.hunk_do(d, &hs, ops::HunkDo::Unstage),
            'd' if !d.staged && !d.untracked && n > 0 => {
                let Some(t) = self.view_text.as_ref() else { return true };
                let Some(patch) = ops::hunk_patch(&t.lines, &hs, d.hunk) else { return true };
                let h = &hs[d.hunk];
                let mut lines = vec!["This change is given up for good:".to_string()];
                lines.extend(t.lines[h.start..h.end].iter().take(10).cloned());
                self.dialog = Some(Dialog::Confirm { title: "Discard the hunk".into(), lines, action: ConfirmAction::GitDiscardHunk { view: d, patch } });
            }
            _ => return false,
        }
        true
    }

    /// Stage / unstage the current hunk, then read the diff again (on the same hunk number, or the last).
    fn hunk_do(&mut self, d: DiffView, hs: &[ops::Hunk], what: ops::HunkDo) {
        let Some(t) = self.view_text.as_ref() else { return };
        let Some(patch) = ops::hunk_patch(&t.lines, hs, d.hunk) else { return };
        self.hunk_apply(d, patch, what);
    }

    /// Apply a hunk's patch in the background, then show the diff again.
    pub(super) fn hunk_apply(&mut self, d: DiffView, patch: String, what: ops::HunkDo) {
        let (tx, wake) = (self.git_tx.clone(), self.wake_tx.clone());
        std::thread::spawn(move || {
            let done = ops::apply_hunk(&d.repo, &patch, what);
            let result = done.and_then(|_| ops::diff(&d.repo, Some(&d.path), d.staged, d.untracked));
            let keep = d.hunk;
            let _ = tx.send(GitMsg::Diff { view: DiffView { hunk: keep, ..d }, result });
            let _ = wake.send(());
        });
    }

    /// Leave the viewer.
    pub(super) fn close_viewer(&mut self) {
        self.viewer = false;
        self.view_text = None;
        self.pv_off = 0;
        self.drawer.forget();
    }

    // ----- the text -----

    /// The colour of a line of a diff / commit text.
    pub(super) fn text_line_style(l: &Look, line: &str) -> Style {
        let base = Style { bg: l.panel.bg, ..l.panel };
        let colour = if line.starts_with("+++") || line.starts_with("---") {
            Some("yellow")
        } else if line.starts_with('+') {
            Some("green")
        } else if line.starts_with('-') {
            Some("red")
        } else if line.starts_with("@@") {
            Some("cyan")
        } else if line.starts_with("diff ") || line.starts_with("index ") || line.starts_with("new file") || line.starts_with("deleted file") || line.starts_with("similarity") || line.starts_with("rename ") {
            Some("comment")
        } else if line.starts_with("commit ") {
            Some("yellow")
        } else if line.starts_with("Author:") || line.starts_with("Date:") {
            Some("blue")
        } else {
            None
        };
        match colour {
            Some(c) => tint(l, c, base, base.bg),
            None => base,
        }
    }

    /// The text of `view_text` on the whole area: its title on a bar, then the lines from the offset `pv_off` (kept inside the text). In a diff
    /// the current hunk's `@@` line is lit.
    pub(super) fn draw_text_view(&mut self, cv: &mut Canvas, l: &Look, r: Rect) {
        let Some(t) = &self.view_text else { return };
        let (f_bar, f_name, _) = bar_styles(l, true);
        cv.fill(Rect { x: r.x, y: r.y, w: r.w, h: 1 }, f_bar);
        let x = cv.put(r.x, r.y, " ", f_name, r.x + r.w);
        cv.put(x, r.y, &format!(" {} ", truncate(&t.title, r.w.saturating_sub(4) as usize)), f_name, r.x + r.w);
        let h = r.h.saturating_sub(1) as usize;
        let max = t.lines.len().saturating_sub(h);
        let off = self.pv_off.min(max);
        let width = r.w.saturating_sub(2) as usize;
        let cur = t.diff.as_ref().and_then(|d| ops::hunks(&t.lines).get(d.hunk).map(|h| h.start));
        for (row, line) in t.lines.iter().skip(off).take(h).enumerate() {
            let mut st = Self::text_line_style(l, line);
            if cur == Some(off + row) {
                st = Style { fg: l.sel[0].fg, bg: l.sel[0].bg, attrs: l.sel[0].attrs, ..st };
            }
            let y = r.y + 1 + row as u16;
            cv.fill(Rect { x: r.x, y, w: r.w, h: 1 }, st);
            cv.put(r.x + 1, y, &truncate(&line.replace('\t', "    "), width), st, r.x + r.w);
        }
        self.pv_off = off;
    }

    /// Where the git of a panel on a server runs: `ssh` with what the connection says (the SFTP ones with a host); `None` for any other
    /// kind of remote (a mount, rclone, ftp: nothing to run git on).
    pub(super) fn git_ssh_of(&mut self, remote: &str) -> Option<lvim_fuzzy_plugin_git::runner::SshTarget> {
        let cfg = self.remote_client().config();
        let site = &cfg["remotes"][remote];
        if site["engine"].as_str() != Some("sftp") {
            return None;
        }
        Some(lvim_fuzzy_plugin_git::runner::SshTarget {
            host: site["host"].as_str()?.to_string(),
            user: site["user"].as_str().map(str::to_string),
            port: site["port"].as_u64().map(|p| p as u16),
            key: site["key"].as_str().map(str::to_string),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keys_of_group_x_are_unique_and_the_names_are_git_commands() {
        let mut subs: Vec<char> = Vec::new();
        for s in GITSUBS {
            assert!(!subs.contains(&s.key) && !GITDIRECT.iter().any(|(k, _, _)| k.starts_with(s.key)), "{}", s.key);
            subs.push(s.key);
            for (i, (k, n, l)) in s.items.iter().enumerate() {
                assert!(n.starts_with("git-"), "{n}");
                assert!(l.split_whitespace().count() <= 2, "a button of 1–2 words: {l}");
                assert!(s.items.iter().skip(i + 1).all(|(k2, n2, _)| k != k2 && n != n2), "{} {k} {n}", s.key);
            }
        }
        assert!(!git_label("git-merge").is_empty());
    }
}
