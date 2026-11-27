# lvim-fuzzy-commander

`lvim-fuzzy-commander` (short name `lvc`, a symlink the install script makes) — a two-panel file manager for the terminal, in the style of Midnight Commander, with the lvim
look (the lvim-colorscheme themes, icons, panels) and the lvim-fuzzy matcher for filtering.

Status: early development.

- Two panels, each with its own directory, cursor, marks, query and history; Tab switches.
- Typing filters the active panel (fuzzy, as lvim-fuzzy); Esc clears.
- Operations in Rust, no shell: copy, move, rename (one, or many at once in your editor), make a
  directory, change the mode, delete — to the system trash by default, or for good.
- The system trash (the freedesktop.org one, shared with other file managers, `gio trash` and
  `trash-cli`): `gt` shows it in a panel, where items are restored, deleted for good, or the trash
  emptied.
- Every column in its own colour: the size by magnitude (bytes dim, K green, M yellow, G red), the date by
  age (today green, this week teal, this month blue, this year purple, older dim), the permissions per
  bit, the owner (root red), the group, the type (DIR blue, EXE green, LNK cyan, BRK red, FILE dim; for a directory the size column shows how many entries it holds, `(23)`).
- Changes made by other programs (a file created, deleted, renamed or written; the trash) show at once
  (inotify); a panel whose directory is deleted goes to the nearest one that still exists.
- Long operations run in the background with their progress on the title bar; Esc cancels. A target
  that exists asks: overwrite, skip, keep both — once, or for all the rest.
- `au` undoes the last operation (a move or rename moves back, a trashing is restored, a copy or a new
  directory goes to the trash).
- The quick view (`vv`) shows the file under the cursor in place of the other panel: a real Neovim (taken
  warm from `lvim-fuzzy-cli pool` when it runs), with the built-in preview while Neovim catches up, for
  directories, and as a fallback; an archive shows its listing.
- Tabs: `tN` opens one (on the same two directories), `td` closes it, `tn` `tp` `t1`…`t9`
  go between them; `te` gives the tab a name of its own (empty: its directory's name again), `t<` / `t>` (past the end it wraps round)
  move it left / right. The title bar lists them: a click goes to a tab, a middle click closes it, a right click
  renames it.
  The tabs are remembered: leave lvc and start it again with no directory given, and it opens the way you left it — the
  tabs in their order with their names, both directories of each (the entry under the cursor too), the active panel
  and the quick view of each tab, the tab you were on. It is kept in `$XDG_STATE_HOME/lvim-commander/state.toml` as you
  work (so a crash loses nothing), and with directories on the command line you get what you asked for, not the saved
  tabs. With two lvc running, the one that closes last wins.
- The quick view of media: images (png, jpeg, gif, webp, bmp, tiff, ico…; HEIC / AVIF / JXL through
  ImageMagick), SVG, video (a frame: `ffmpegthumbnailer` / `ffmpeg`), PDF (the first page: `pdftoppm`),
  fonts (a sample: `fontimage`), audio (its cover) — each with lines about it (size, length, codecs, tags,
  pages…). The picture is a real image in kitty / ghostty / a zellij that takes kitty placeholders, half
  blocks everywhere else; a missing program leaves the lines alone. Photos turn as the camera says (EXIF).
  The previews are made by two workers — the file under the cursor first, then its neighbours, then the
  rest of the page; moving on drops what is stale — and kept in `~/.cache/lvim-media/` (by the file's size
  and times: a changed file is made again; kept under 500 MB), so they are instant the next time, after
  a restart too.
- Symbolic links (`aL`: absolute or relative, in the other panel), the path / name / directory into
  the clipboard (`yp` / `yn` / `yd`: OSC 52 and wl-copy / xclip).
- Several lvc at once: each listens on `$XDG_RUNTIME_DIR/lvim-commander/<pid>.sock`; `ti` lists the
  others (go to its directory, bring it here, copy / move the marked to it), `lvc --list` and
  `lvc --send <pid|all> <instruction>` from a shell or a script.
- Archives: `ae` extracts the one under the cursor into the other panel, `az` packs the marked ones
  (`.tar.gz .tar.xz .tar.bz2 .tar.zst .tar .zip .7z`); the quick view lists them, and also `.rar`, and
  `.deb .rpm .iso .cpio .cab .xar .tar.lz` through `bsdtar` (libarchive also stands in for a missing
  `unzip` / `7z` / `unrar`).
- `ff` / `fg` run the lvim-fuzzy finder (files / live grep) from the panel's directory and bring the
  panel to the result.
- Material file icons as IMAGES (as lvim-icons draws them in Neovim) where the terminal takes kitty
  placeholders — kitty, ghostty, zellij built with them — else Nerd Font glyphs (`icons = "auto"`).
- Two modes: normal (`j` / `k`, `gg` / `G`, `h` / `l`, Space marks, `.` hidden files) and input
  (Ctrl-f or `/`: typing filters; Enter / Esc back).
- `oo`: a menu of every operation and every user action; extensible — see [Extending](#extending).
- Bookmarks and the recently visited directories (`bl`, `ba` keeps one, `bd` deletes one), go to a typed path (`gc`), a command run in the shell (`ax`), the shell under the
  panels (`as` / `Ctrl-o`), the editor (`oe`), opening by extension (`[open]`, else the editor for
  text and `xdg-open` for the rest).

## Keys

The F-keys and the moving keys are direct; **every other command sits in a key group**: the first letter of
a group (`t` tabs, `m` marks, `a` actions, `y` copy, `f` find, `g` go, `v` view), then the command's letter —
`tn` is the next tab, `ma` marks all. The bar at the bottom shows the groups (buttons: a click works too);
after a group's letter it shows that group's keys, `Esc` leaves; when it is longer than the screen it scrolls (← → in a group, the wheel, the arrows at its ends, Ctrl-j then h / l). `?` or `g?` lists everything. The F-keys are not bound; `[keys]` can give any of them to an action. (There is no leader
key: `leader` in the config is an error that says so.) Any command can be moved to a key of its own in `[keys]`.

| Key | |
|---|---|
| Tab · Ctrl-h · Ctrl-l | the other panel · the left one · the right one |
| j k · gg G · ↑ ↓ PgUp PgDn Home End | move · top / bottom of the panel |
| l · Enter · → | enter a directory **or an archive** (unpacked once into `~/.cache/lvim-commander/arc`; `h` at its top comes out; changes inside are not written back), open a file |
| h · Backspace · ← | up one directory |
| Ctrl-f · / | input mode: typing filters (fuzzy); Enter / Esc back |
| Space · Insert | mark and move down |
| . | hidden files |
| J · K | the quick view: the next / previous page of a PDF or document, point of a video (5 % steps); else scroll 3 lines |
| Ctrl-e · Ctrl-y | scroll the quick view one line |
| Ctrl-d · Ctrl-u | half a page down / up — of the quick view when it is open (a Neovim preview gets the keys), else of the list |
| Delete | to the trash (`delete = "direct"`: for good) |
| Ctrl-c q | quit (also `aq`) |
| Ctrl-k | with several tabs, up onto the **tab bar** (a tab is lit): `h` / `l` (← →, Tab) choose, `Enter` goes to it, `Esc` / `j` / `Ctrl-j` back to the panels |
| Ctrl-j | onto the bottom bar: its buttons as real buttons — h / l (← →, Tab) choose, Enter or Space presses, Ctrl-k (or Esc, k) back to the panel it came from; the mouse presses them too |

The groups (`Esc` leaves one):

| Keys | |
|---|---|
| `tN` · `tn` `tp` · `td` · `te` | a new tab · the next / previous · close · rename (empty: back to its directory's name) |
| `t<` `t>` · `t1`…`t9` · `ti` | move this tab left / right (past the end it wraps round) · go to that tab · the other running lvc |
| `oc` `om` · `oN` · `on` · `oe` | copy · move (to the other panel; the target can be edited, and may end in a rename mask: `*.md` gives every file its name without the last extension + `.md`, `old_**` puts `old_` before the whole name) · rename (several marked: all in the editor) · new (a directory when the name ends in `/`; `a/b/` and `a/b/c.txt` make the directories on the way) · edit (a directory too) |
| `or` · `oR` | refresh the panels · rename the tab (`te` too) |
| `od` `oD` · `oo` | to the trash (`delete = "direct"`: for good) · delete for good · the menu: every operation and user action |
| `ot` | the running operations: operations run side by side (a second copy does not wait for the first); here each is listed with its progress, `x` cancels the selected one. `Esc` in the lists cancels the newest |
| `ma` · `m*` · `m+` `m-` | mark all · invert · mark / unmark by glob |
| `as` · `Ctrl-o` | the shell under the panels (as in Midnight Commander): a real shell on a pseudo terminal, started in this directory the first time and kept alive between visits; while it is shown lvc is out of the way (the terminal's own screen, with its history); `Ctrl-o` again comes back and the panel goes to the directory the shell is in; `exit` ends it |
| `az` · `ae` | compress the marked · extract |
| `am` | permissions: ticks (owner / group / others × read / write / exec, setuid / setgid / sticky) and a field for octal (`755`) or symbolic (`u+x,g-w,o=r`, `+X`, applied to each item's own mode) — a tick changes the field, the field changes the ticks |
| `ao` | owner and group: the group first, then the user; drop-downs of the real groups (yours first) and — for the chosen group — only the people in it (those it lists and those whose primary group it is); choosing a group moves the user to the first person of it when the chosen one is not in it; typing filters a drop-down |
| `aL` · `au` | symbolic links to the marked in the other panel · undo the last operation |
| `:` · `ax` | the **command line**: one row over the button bar, `dir $ command`; Enter types the command into the shell under the panels (in this panel's directory) and shows the shell — the output has your aliases, colours and icons; `Ctrl-o` comes back. `Tab` completes the word under the cursor — a command from `$PATH` for the first word, a path (relative to the panel's directory, `~/` too, spaces escaped) for the rest: the common start first, the candidates on the row above, `Tab` again goes through them. Up / Down recall the commands typed before (kept in `~/.local/state/lvim-commander/shell_history`), `Ctrl-u` clears, `Esc` closes |
| `ar` · `aE` · `aq` | in the trash: restore · empty · quit |
| `yy` `yx` | put the marked items (or the one under the cursor) into the **buffer**, to copy / to move; the marks go. The buffer is a file (`~/.local/state/lvim-commander/buffer.toml`), so every lvc sees it and it survives a restart On this machine `yy` / `yx` put the files themselves into the desktop clipboard too (`text/uri-list`, through `wl-copy` / `xclip`): a browser or a file manager pastes them (`clipboard_files = false` in the config turns it off). |
| `pp` `po` `pc` | paste: into this directory · into the other panel's · as copies whatever the buffer holds (a cut buffer moves, and is emptied by `pp` / `po`) |
| `pr` · `pl` `ph` | paste with the target as a field (a rename mask may end it) · as symbolic links (Tab: absolute / relative) · as hard links |
| `pm` · `pv` `pd` | copy here what is in the system clipboard (wl-paste / xclip): files copied in another program (a file manager, a browser: `text/uri-list`, `file:///…%20…` read back) or paths as text · what is in the buffer · empty it |
| `yp` `yn` `yd` | copy the path(s) · the name(s) · the panel's directory to the clipboard (text) |
| `ff` `fg` | find files · grep (lvim-fuzzy) |
| `x…` | **git** — a group with sub-groups (made by the git plugin's `plugin.toml`; installed with `install`), there where a work tree is (on this machine, or on an SSH server). A sub-group or a button that makes no sense for what is under the cursor (or is marked) is not shown. `xa` files: `a` add · `u` unstage · `d` discard (asks) · `r` remove (asks) · `i` ignore · `m` move · `A` add all · `U` unstage all. `xc` commit: `c` commit (asks the message) · `a` amend. `xd` inspect: `d` diff (hunks, see Git) · `l` log of the item · `L` log of the repository (Enter shows a commit) · `b` blame. `xb` branch: `l` list (Enter switches, `d` deletes, `n` new, `m` merge, `r` rebase) · `n` new · `m` merge · `r` rebase. `xp` sync: `f` fetch · `p` pull (fast-forward only) · `P` push. `xs` stash: `s` stash · `l` list (Enter pops, `a` applies, `v` shows, `d` drops). `xt` tags: `l` list (Enter shows, `c` switches, `p` pushes, `d` deletes) · `n` new (a name; a message after a space makes it an annotated one). `xS` submodules: `l` list (Enter goes in) · `u` update · `U` update all. `xm` **in progress** — only while a merge / rebase / cherry-pick / revert is: `c` continue · `a` abort (asks) · `r` resolve the conflicted file (ours · theirs · mark resolved). `xR` read git again. Fetch, pull, push and the rest run in the background and say how they ended |
| `rc` `rl` · `ra` · `re` · `rd` | the **remote connections** (FileZilla's site manager): a list of them with their state (`●` connected, `○` not): Enter connects, and on a connected one the button is **disconnect** — that is the only way out of a remote (`h` / Backspace / `..` stop at its top and say so), and it brings the panel back to its last folder on this machine; the **title bar is red** while the active panel is on a remote (`REMOTE` in the title; `h` at the top comes back) · make a new one in a form that asks in plain words (the kind of connection from a list: SFTP, FTP, an rclone remote, a server opened as a folder with sshfs, an rclone remote as a folder, a mount of your own; then the server from the hosts of `~/.ssh/config`, the key from `~/.ssh`, the remote from `rclone listremotes` — every such field opens a list with `Enter`, and you can type instead) · change one · delete one. They are written into `~/.config/lvim-plugins/lvim-fuzzy-plugin-remote.toml` (comments in it stay), so lfz sees them too; a password is never written — it is asked for when the server refuses the login, and kept for the session |
| `gG` | from a remote back to your own folders — the panel returns to its last folder here and the connection stays open (`rc` opens it again, and disconnects) |
| `gg` · `gc` · `gt` | top · go to a directory · the trash |
| `gd` · `gC` | **devices** (the devices plugin's buttons, in `g`): `gd` the disks, the gvfs mounts and the phones that are plugged in — Enter mounts what is not and goes into it; on a row `m` mounts it, `u` unmounts it and `e` ejects it (a stick, a card, an external disk, a phone), and the list stays open, read again; `gC` connect to `smb://`, `sftp://`, `ftp://`, `dav://` with gvfs |
| `vD` | **diff of two files** (the git plugin's button, in `v`): the two marked files, or the file under the cursor and the one of that name in the other panel, as a coloured text on the whole screen |
| `xdc` | **the changes** (the git plugin's button): a flat list, with a search field, of what is changed or new in the work tree; Enter goes to the file |
| `ba` · `bl` · `bd` · `br` | **bookmarks**: `ba` keeps the place the panel is in (a folder of this machine, or a folder of a remote) under a name; `bl` is the list — `★` the ones you kept, then the `[bookmarks]` of the config — with a search field (`/` or `Ctrl-f` types, `Esc` stops typing): Enter goes there (a remote bookmark connects), `o` goes there in the other panel, `d` deletes a kept one, `r` renames it; `bd` is a list to choose the one to delete and `br` one to rename (both have the search field). A bookmarked folder has a `★` in the type column (`vc`). They are kept in `$XDG_STATE_HOME/lvim-commander/bookmarks.toml`, shared by every lvc |
| `g[` `g]` · `go` `gi` · `g?` | back / forward in the history · the directory under the cursor / this one → the other panel · help |
| `vv` · `vi` | the quick view on / off · info about the file under the cursor: path, kind, type, size, mode, owner, the three times, link target |
| `vb` | synchronized browsing (see Remote files) |
| `al` | **retarget a symbolic link**: where the link under the cursor points is a field to change; the link is made again beside the old one and moved over it (a remote's link is removed and made again there) |
| `vz` | zoom: the active panel has the whole screen (`vz` again, or `Tab`, to get two panels) |
| `os` · `gi` | the other panel goes where this one is: a folder of this machine, or the same connection and path on a remote |
| `vd` | compare the two panels' directories by name, size and time: in each panel what the other lacks or has different is marked (copy it with `oc`) |
| `vc` · `vl` · `vh` | the columns beside the names — **git** (the two letters), type, permissions, owner, group, size, date, and the ones plugins bring — as a list: Space ticks, **`J` / `K` move the column** in the order they are drawn (Shift with the arrows too, or `>` `<`); it is remembered · long view · hidden files |
| `vs` `vS` · `vw` · `vr` · `vH` | sort · reverse · swap the panels · reload · show / hide the key row |
| `hjkl` · `/` · `Space` · `.` | move · search · mark · hidden files |
| `u` · `R` | undo the last operation (a move or rename moves back, a trashing is restored, a copy or a new file goes to the trash) · redo it, that is: do again what the last undo took back (`au` / `aU` are the same keys); a new operation clears what could be redone. Remote operations are not undone |
| `V` | visual mode: the rows from where it was pressed to the cursor are marked as you move; `Esc` or `V` ends it (the marks stay) |

## Git

Where a folder is in a git work tree, lvc shows it and works on it — all of it by the **git plugin** (`lvim-fuzzy-plugin-git`, on the
`git` program; lvc knows nothing of git — the plugin writes the column, the bar and every dialog through the plugin API below, and it serves lfz, the picker and lvim-files too).

- **Marks.** A column (it is first of the columns by default, before the type: `vc` moves it): two letters as in `git status -s`, the index
  (green: it goes into the next commit) and the work tree (`M` modified yellow, `D` deleted red, `A` added, `R` renamed, `?` untracked,
  `!` ignored dimmed, `U` conflict red). A folder takes the heaviest letters of what is in it; what is inside an untracked or ignored folder
  is that too.
- **The bar.** Right of the path: the branch (` main`, or the hash when the head is detached), `MERGE` / `REBASE` / `CHERRY-PICK` /
  `REVERT` while one is in progress, `↑2` commits ahead of its upstream and `↓1` behind, then `+staged ~changed ?untracked !conflicts`.
- **Keys.** Group `x` (see Keys), on the marked entries or on the one under the cursor.
- **Hunks (`git add -p`).** `xdd` opens the diff of the item on the whole screen (the viewer's keys); its current hunk's `@@` line is lit.
  `]` / `[` move to the next / previous hunk, `s` **stages the hunk**, `u` unstages it (in the staged diff), `d` gives it up (asks), `Tab`
  switches between the work tree's diff and the staged one. The diff is read again after each, and the viewer closes when nothing is left.
  A new file has no hunks: `s` stages the whole file.
- **Conflicts.** A merge or a rebase that stops on conflicts stays in progress (`MERGE` in the bar, `U` on the files): `xmr` resolves a file
  by taking one side or marking it as it is now, `xmc` continues, `xma` gives the whole thing up. Nothing opens an editor.
- **On a server.** A panel on an SFTP connection that has a `host` runs git there, over `ssh` (one connection kept for a while, the
  ssh-agent first): the marks, the bar and every command work as here. A mount, an rclone remote or ftp has nothing to run git on.
  Archives are not work trees (their content is unpacked in the cache): no marks there.
- **A secret.** A push to a server, an SSH key with a passphrase: git and ssh are cut off from the terminal and ask through the prompt the
  machine has — the `ward` / `pinentry` of `~/.gnupg/gpg-agent.conf`, `pinentry` on the PATH, `$SSH_ASKPASS` — the same as the remote
  plugin does. Nothing is ever written on lvc's screen.
- **How it is read.** Always in the background (a big repository does not stop the keys), again whenever a folder is read, and after
  every operation; moving about inside one work tree keeps the marks on show while the next answer comes. Without the plugin installed
  ignored files unmarked (quicker beside a huge ignored folder).
- The plugin's own settings (`timeout`, `long_timeout` for fetch / pull / push, `ignored`, `untracked`, `log`) are under
  `[plugins.lvim-fuzzy-plugin-git]` here or in `~/.config/lvim-plugins/lvim-fuzzy-plugin-git.toml`.

## Remote files

`rl` opens a remote in the active panel (`ra` makes a connection, `re` changes one, `rd` deletes one); the directories are read by the **remote plugin** (`lvim-fuzzy-plugin-remote`,
which ships inside lvc and lfz) in the background — the panel says `REMOTE …` until the answer comes, and lvc never
waits for the network. Which engine serves a remote is the plugin's configuration (what `ra` writes, or `[plugins.lvim-fuzzy-plugin-remote]`
in lvc's config, or `~/.config/lvim-plugins/lvim-fuzzy-plugin-remote.toml` for every tool):

- `rclone` (the default): the remotes of `rclone listremotes` — SFTP, FTP, S3, Google Drive, Dropbox, OneDrive, WebDAV,
  SMB … — made with `rclone config`;
- `sftp`: SFTP over SSH (libssh2; `~/.ssh/config` understood, the ssh-agent or a key) and `ftp`: plain FTP — built into the plugin;
- `mount`: `sshfs`, `rclone mount`, `gio mount` … — a `mount` command, or a `root` that is mounted already;
- an engine of your own: `kind = "command"` with the command lines to run.

```toml
[plugins.lvim-fuzzy-plugin-remote.remotes.work]
engine = "mount"
mount  = "sshfs me@work.example.org: {mnt}"
```

On a remote panel the keys are the usual ones — nothing to learn twice:

| Keys | On a remote |
|---|---|
| `Enter` · `l` · `h` | walk the directories; `Enter` on a file **downloads it to the folder of the other panel** (like a double click in FileZilla; `enter = "open"` in the plugin's settings makes it download a copy into the cache — `~/.cache/lvim-fuzzy-remote`, kept under `cache` MB, the message says where — and open it as any file is opened); on an archive it enters it (`h` at its top comes back to the remote) |
| `vv` | the quick view of a remote file: its copy is downloaded (up to `preview` MB) and the preview plugins show it |
| `oe` | edit: the copy is opened in the editor and **goes back to the remote when it changed** |
| `oc` · `om` | copy / move between a remote and a local panel, in both directions, several marked at once (one job each, with progress; `Esc` stops it; `ot` lists them) |
| `on` · `oN` · `od` `oD` | make a directory (a name ending in `/`) or an empty file · rename · delete for good (there is no trash) |
| `am` | permissions, with the same dialog (octal, symbolic, ticks) — where the engine knows them |
| `ao` | owner and group with the **real users and groups of the remote machine** (read from its `/etc/passwd` and `/etc/group`): the group first, then a user of that group |
| `aL` | a symbolic link in the remote directory to the entry under the cursor |
| `ae` | unpack a remote archive into the other (local) panel |
| `yy` `yx` · `pp` `po` `pc` | the buffer works across: `yy` / `yx` in a remote panel puts remote files in it, `yy` in a local one puts local files; `pp` pastes into this panel — a local files into a remote directory is an upload, remote files into a local one a download, a cut inside one remote is a rename, and from one remote to another goes through a temporary copy on this machine |
| `vd` | compare the local and the remote directory (name, size, time) |
| `vb` | synchronized browsing: when one panel goes into a directory or out of one, the other does the same when it can |

Not on a remote (yet): the finder (`ff` / `fg`), compress, the shell
in the remote directory, . What an engine cannot do it says:
`rclone` cannot set permissions or owners (use `sftp`), `ftp` can set permissions only, `mount` and `sftp` do it all.

## As a file chooser

`lvc --chooser-file FILE [DIR…]`: Enter on a file — or on the marked ones — writes the paths to FILE, one on a line,
and quits; a directory is entered as always. Quitting (`aq`) without choosing writes nothing, so remove FILE first and
read it after. The tabs are not restored or saved in this mode (it is a visit, not the way lvc was left) — `--keep-tabs` restores and saves them like a plain run; `--session NAME` does the same under a name of its own (`sessions/NAME.toml` beside `state.toml`), so a host can keep one set of tabs per project.
E.g. an editor's file picker: `lvc --chooser-file /tmp/pick "$PWD" && cat /tmp/pick`.

## Icons

The Material Icon Theme set lives in ONE place for every lvim program — `~/.local/share/lvim-icons/material`
(lvim-icons in Neovim, lvim-fuzzy, lvim-fuzzy-commander). Each carries the set and puts it there when it is missing
or older (never a downgrade, swapped in atomically): `scripts/install.sh` runs `lvc --install-icons`, and
`lvc` does it at start in image mode. `icons = "font"` keeps the glyphs; `zellij_images = false` for a zellij
that does not take kitty placeholders.

## Extending

```toml
[actions.gitlog]                  # a key and / or an entry of the `oo` menu → a command
title   = "git log of the file"
key     = "ctrl-c L"   
command = "git log --oneline -- {}"
mode    = "wait"                  # wait | tty | silent | control
reload  = false

[previewers.markdown]             # the quick view of these files: a command's output (ANSI colours kept)
command    = "glow -s dark {}"
extensions = ["md"]
```

Placeholders (each quoted for the shell): `{}` the entry under the cursor, `{+}` the marked ones, `{name}`,
`{dir}`, `{other}` (the other panel); the same as `$LVC_FILE`, `$LVC_FILES`, `$LVC_DIR`, `$LVC_OTHER_DIR`.
Modes: `wait` runs in the terminal and waits for a key, `tty` returns at once (an editor, a TUI), `silent`
runs in the background, `control` too — and every line it prints is an instruction for lvc: `cd PATH`,
`other-cd PATH`, `select NAME`, `mark NAME`, `reload`, `message TEXT`, `error TEXT`.

**Plugins** are directories in `~/.config/lvim-plugins/<name>/` (the directory every lvim tool shares; the old
`~/.config/lvim-commander/plugins/` is still read) with a `plugin.toml` holding the
same `[actions.*]` / `[previewers.*]` (and a `description`); their commands get `$LVC_PLUGIN_DIR`, so a
plugin ships its own scripts in any language. `lvc --plugins` lists what is loaded and any problem. Plugins by other developers come from git:
`lvim-fuzzy-plugin install <git-url>` (also `update`, `remove`, `list`), which asks before it trusts a plugin that is not
sandboxed (`wasm`) or runs its build command.
`examples/plugins/git/` is one.

**Buttons.** An action may be a button of the bottom bar: it sits in a group of keys (`t m a y p f g o x r v`, or a group of the plugin's own —
a letter that is none of those), in a sub-group of it when it says so; it is shown only where it is:

```toml
[actions.lint]
title        = "lint"             # the label of the button (1–2 words)
call         = "lint"
group        = "x"                # the group; `group_title` names a group of your own
sub          = "a"                # optional: the sub-group (git has `a c d b p s t S m`); `sub_title` names one of your own
button       = "l"                # the key of the button there
```

**Columns.** A plugin may bring a column: `[columns.<id>]` with a `title` and a `call` (a function of the plugin). lvc calls it for the
names of each folder it shows — `{"call":…,"column":"plugin.id","dir":"/d","names":["a","b"]}` — and the plugin answers
`[{"op":"cells","column":"plugin.id","dir":"/d","cells":{"a":[["12","green"],["K","comment"]]}}]` (pieces of text, each with a theme colour).
The column is in `vc` with the built-in ones: it can be ticked off and moved with `J` / `K`.

**The keys are one map.** The app's own commands and every plugin's buttons are put in the groups the same way, and the same rules hold for all:
a plugin may *add* a key to a group (one of the app's, or one another plugin made; the first to name a group's letter makes it, a sub-group the
same) — it may not take a key that is taken, by the app or by another plugin: that is an error. The groups `x` (git) and `r` (remote) are not in
the app at all: their plugins place them (`builtin = "git-stage"` in the git plugin's `plugin.toml` is the operation the app does for a button
the plugin put at `x a a`); without the plugin installed (`install`, above) the group is not there.

**Moving a key** is the config's say, for the app and for every plugin, and what it moves is checked with the rest:

```toml
[keys]
"zoom"      = "v y"               # the app's command `zoom` on `v y` (a place: group, [sub-group,] key); `find = "alt-f"` is a key of its own
[plugins.lvim-fuzzy-plugin-git.keys]
stage = "x a z"                   # a plugin's button by the action's name; the same under [keys] of ~/.config/lvim-plugins/<name>.toml
```

**A wrong config does not start lvc.** Before anything is drawn the config, the themes, the plugins (their manifests, their settings files) and the
map of the keys are checked; every problem is printed — a key that two things want says who wants it — and lvc exits with status 2.

**Installing the plugins that ship inside lvc** (preview, open, remote, git, devices) is the config's decision — nothing is written by itself:

```toml
[plugins.lvim-fuzzy-plugin-remote]
# install = "/path/to/lvim-fuzzy-plugin-remote"   # or another program answers (a build of the plugin's own)
```

Without `install` the plugin is used if it is installed otherwise (`lvim-fuzzy-plugin install`, a copy in the plugins directory), and not
at all if it is not.

The plugin host itself — `plugin.toml`, the runtimes, the `lvim-fuzzy-plugin` tool (`list`, `check`, `host`) and the SDK — is
the separate **lvim-fuzzy-plugin** crate, shared with lvim-fuzzy-cli and lvim-picker; what follows is how lvc uses it.

A plugin can name a **runtime** in `plugin.toml` (`runtime = "command"` — the default, a process per action —
`"process"`, `"native"`, `"wasm"` or `"lua"`). Its actions then say `call = "function"` instead of a
`command`. Every runtime speaks the same JSON: lvc sends the context (`call`, `arg`, `file`, `files`, `dir`,
`other_dir`, `panel`) and the plugin answers with an array of instructions: `{"op":"cd","path":...}`,
`other-cd`, `select`/`mark` (`name`), `reload`, `message`/`error` (`text`), and the interactive ones —
`input` (`prompt`, `value`, `then`), `confirm` (`text`, `then`), `menu` (`title`, `items` of `label` + `call`),
`preview` (`text`). `lvc --plugins` shows each plugin's runtime and whether its script, module or library loads.

**What a plugin can write in lvc (all runtimes).** The app knows no plugin by name: everything below is the same for each.

- **The call's context** has `call`, `arg`, `data` (the note an instruction left, see below), `file`, `files`, `dir`, `other_dir`, `panel`, plus
  `names` (the marked entries, or the one under the cursor), `dirs` (those of them that are folders) and, in a panel on a server, `remote` and
  `ssh` (`{host, user?, port?, key?}` of an SFTP connection) with `dir` the path there.
- **A column** (`[columns.<id>]`: `call`, `title`, `position = "first"`, `remote = true` to be asked for a server's folder too, `always = true`
  to be asked even when its column is off). The call is made for each folder shown, again whenever it is read again, and answers:
  `{"op":"cells","column","dir","remote"?,"cells":{"name":[["text","colour"],…]},"flags":{"name":["fact",…]}}`,
  `{"op":"bar","bar":"id","dir","segments":[["text","colour"],…]}` (a piece of the path bar; a colour may add `bold`: `"red bold"`) and
  `{"op":"state","dir","flags":["fact",…]}` (the folder as a whole). One call may answer all three.
- **A button** (`[actions.<id>]`: `call`, `group`, `sub`, `button`, `title`, …) may say `when = ["repo", "item:staged", "one:file", "!busy"]`:
  it is shown only while all hold — `x` the folder has the fact, `item:x` some target has it, `one:x` the only target has it, `!` turns round.
- **`menu`** (`title`, `items` of `label`, `call`, `arg`, `cells`, `enter` = what Enter is called, `keys`): a list with search. An item's `keys`
  are `{"key":"d","label":"delete","call","arg"?,"confirm"?}` — pressed on the item they call the plugin (after a yes / no with the `confirm`
  text), and show as buttons below.
- **`text`** (`title`, `text` or `lines` — each a string or a list of `[text, colour]` pieces —, `diff` to colour `+` `-` `@@` lines, `anchors` =
  lines `]` / `[` move among, `current`, `keys` as above — their call gets `arg` = `{"anchor","line","data"}` —, `data`): a text on the whole screen.
  `close` leaves it.
- **`input`** (`prompt`, `value`, `then`, `data`) and **`confirm`** (`title`, `text`, `then`, `data`): the call `then` gets the typed text as `arg`
  (a `confirm` gets its `data` as `arg`) and `data` as `data`.
- **`notify`** (`level` = `debug` `info` `status` `success` `warn` `error`, `title`, `text`): a card at the bottom right — a bar and a chip in the level's colour,
  the title, the time, the text — for a few seconds (an error longer), kept in `oo` → Notifications. A button that calls a plugin shows a `status`
  card at once; the answer replaces it. `message` / `error` are an `info` / `error` card.
- **`badge`** (`text`, `colour`): a mark at the right of the title bar, the plugin's own (empty `text` removes it). **`refresh`** asks the
  columns again; **`reload`** reads the folders again.

**`runtime = "process"`** (done): the plugin is one long-lived program, `exec = "./run"` (`sh -c`, started in the
plugin's directory with `$LVIM_PLUGIN_DIR`, at the first call; its stderr is dropped). lvc writes a JSON line per
call to its stdin — `{"id":1,"method":"call","params":{context}}` — and reads `{"id":1,"result":[instructions]}`
(or `{"id":1,"error":"text"}`) from its stdout; an answer shows at once. No answer in 10 s: a message, and a
late answer is ignored. A process that ended starts again on the next call. `events = ["cd", "cursor",
"selection", "quit"]` also sends `{"method":"event","params":{"event":"cd", ...}}` (no answer) when the directory,
the file under the cursor or the marks change, and `quit` as lvc ends (stdin is closed after it).
`lvim-plugin/examples/plugins/rust-process/` is one, in Rust.

**`runtime = "native"`** (done): the plugin is a shared library, `library = "./target/release/libx.so"` (relative to
the plugin's directory), loaded at the first call. The C ABI is three functions with NUL-terminated JSON strings —
`uint32_t lvim_plugin_abi(void)` (must be 1), `char *lvim_plugin_call(const char *name, const char *ctx_json)` (the
instructions as a JSON array, or `{"error":"text"}`; NULL = nothing) and `void lvim_plugin_free(char *)` — so
nothing depends on a compiler version. A wrong ABI version or a missing function refuses the library with a
message (`lvc --plugins` tries it and says). A call runs on its own thread (the interface never waits; 10 s
and it is reported as timed out). In Rust the `lvim-plugin-api` crate (`lvim-plugin/plugin-api/`) writes all three from one
function, `fn(&str, &serde_json::Value) -> Result<serde_json::Value, String>`, and catches panics.
A library is native code in lvc's own process: a crash or an endless loop in it cannot be stopped — for a plugin
that must be isolated use `process` (or `wasm`). `lvim-plugin/examples/plugins/rust-native/` is one.

**`runtime = "wasm"`** (done): the plugin is a WebAssembly module (`module = "./x.wasm"`, built for
`wasm32-wasip1`), run by the small `wasmi` interpreter. Same JSON, through a tiny guest ABI (`lvim_plugin_abi`,
`lvim_alloc`, `lvim_free`, `lvim_plugin_call` — the `lvim_plugin!` macro of the `lvim-plugin-api` crate writes them for
wasm32). It is the isolated one: every call gets a NEW instance (no state between calls, nothing left by a
crash), memory is capped (256 MB), every instruction costs fuel and an endless loop is stopped with a message
after about 2 s, and the module sees no file but the two panel directories — **read-only** unless
`allow_write = true` — no network, no environment, no terminal. `lvim-plugin/examples/plugins/rust-wasm/` is one
(`cargo build --release --target wasm32-wasip1`).

**`runtime = "lua"`** (done): the plugin is an `init.lua` (or `script = "x.lua"`) in an embedded Lua 5.4 — nothing to
install. It runs once, at the first call; its global functions (or the table it returns) are the actions'
`call`s, its state lives on between calls, `require "x"` finds `x.lua` beside it. A function gets the context table
(`file`, `files`, `dir`, `other_dir`, `panel`, `arg`) and answers through `lvim` (`lvc` is an alias): `lvim.cd(path)`, `other_cd`,
`select(name)`, `mark(name)`, `reload()`, `notify(text)`, `error(text)`, `preview(text)`,
`input(prompt, "then_function", default)`, `confirm(text, "then_function")`,
`menu(title, {{label=, call=, arg=}, ...})`, `run({"git", "status"}, function(code, stdout, stderr) ... end)` (in the
background; the callback answers later) and `current()`, `marked()`, `dirs()`. A function may also return a
string (a message) or a list of instruction tables. Calls run on a thread of their own, one at a time; more than
200 M instructions and the call is stopped with an error (an endless loop freezes nothing), memory is capped at
256 MB. A Lua plugin has the Lua standard library (files, `os.execute`) with your rights, as a shell-script
plugin has — for a sandbox use `wasm`. `lvim-plugin/examples/plugins/lua/` is one.

## Printing the choice

`Shift-Enter` (or `op`) leaves lvc printing the folder of the choice — the marked entries, or the one under the cursor (of a file: the folder it is in),
else the folder shown — so `cd "$(lvc --print-dir)"` goes there. To have `lvc` itself change the directory of your shell (a program cannot do that to
its parent) put `eval "$(lvc --zsh)"` in `~/.zshrc` (`--bash` for bash): then `lvc` — or `lvcd` — does it, also on a plain quit (the folder it was left
in). `Shift-Enter` needs a terminal that tells it apart from Enter (kitty and the like; lvc asks for the keyboard protocol,
`LVIM_KEYBOARD_PROTOCOL=0` turns that off); `op` works everywhere, and the key can be moved with `[keys]` (`cd = "leader P"`). `ot` lists the notifications.

## Several instances

```sh
lvc --list                          # pid, the left and right directories (* = the active panel)
lvc --send 12345 cd ~/Downloads     # cd · other-cd · select NAME · mark NAME · reload · message TEXT
lvc --send all reload
```

## Leaving in another directory

`lvc --print-dir` prints the active panel's directory when it ends. `lvc --zsh` (or `--bash`) prints a shell function (`lvc` and `lvcd`):

```sh
eval "$(lvc --zsh)"   # ~/.zshrc: then `lvc` opens lvc, and Shift-Enter in it leaves your shell in the folder of the choice
```

Quitting normally leaves the shell where it was. To have it go to the folder lvc was in too, turn on **quitting changes the shell's folder** in the
settings panel (`o S`, tab Files; `cd_on_quit = true` in the config). The function does not touch lvc's output: an editor lvc starts keeps the terminal.

## State

`lvc` remembers whether the quick view (`vv`) was open and which columns show (`vc`), in `~/.local/state/lvim-commander/state.toml`
(`$XDG_STATE_HOME`): it starts as you left it.

## Settings

`o S` (or `Ctrl-c C`) opens the **settings panel**: the settings that are worth changing while you work, in four tabs — **General** (editor, preview,
a Neovim kept ready, key hints, theme), **Files** (hidden files, where delete goes, whether quitting changes the shell's folder, the desktop clipboard for `yy` / `yx`, following changes made
outside), **Notifications** (the lowest level that shows as a card — `warn` shows warn and error —, how many cards at most, how many notifications the
list `o i` keeps, the seconds a card of each level stays) and **Display** (file icons, images inside Zellij, how a picture reaches the terminal).
`Tab` / `Shift-Tab` (or `[` `]`, or `1`–`4`) change the tab, `j` / `k` the row, `h` / `l` (or `Space`) the value. A change takes effect at once where it
can (a row marked `next start` waits for the next start of lvc) and is written to `~/.local/state/lvim-commander/settings.toml`
(`$XDG_STATE_HOME`). The same keys in `config.toml` win: the panel shows such a row as `(config.toml)` and does not change it. A level that does not
show as a card is still in the list of `o i`.

## Config

`~/.config/lvim-commander/config.toml` (`$LVIM_COMMANDER_CONFIG`); every key is optional:

```toml
theme   = "LvimEverforest_dark"  # an lvim theme; default: lvim-fuzzy's (~/.config/lvim-fuzzy)
delete  = "trash"                # od, Delete: "trash" | "direct"
hidden  = false                  # show dotfiles at start
cd_on_quit = false               # with the shell function: quitting also changes the shell to the folder lvc was in (Shift-Enter always does)
key_hints = true                 # the bar of key groups at the bottom (v H toggles it)
watch   = true                   # show at once what other programs change in the shown directories
editor  = "nvim"                 # default: $VISUAL, $EDITOR, nvim
preview = "nvim"                 # the quick view: "nvim" | "file" | "off"
nvim    = "nvim"
pool    = true                   # take the preview's Neovim from `lvim-fuzzy-cli pool`
clipboard_files = true           # yy / yx put the files themselves in the desktop clipboard (text/uri-list) too
image_transfer = "file"          # pictures: the terminal reads a temp file (fast); "direct" sends the bytes
notifications = "warn"           # the lowest level that shows as a card: debug info status success warn error (the list `o i` keeps all)
notification_cards = 4           # cards on the screen at most
notification_history = 200       # notifications the list keeps

[notification_seconds]           # how long a card of each level stays
warn = 6

[keys]                           # any command on another key (`?` shows the new ones)
copy = "f5"                      # the F-key actions: help view copy rename move edit new trash delete menu quit
move = "f3"                      # a key whose command moved elsewhere does nothing
edit = "f9"                      # "f5", "shift-f8", "alt-x", "ctrl-x", "tab", "space"
find = "alt-f"                   # the commands of the groups: find grep goto bookmarks shell trash-view restore
up   = "ctrl-p"                  # empty-trash chmod chown compress extract undo mark-all invert-marks mark-glob ...
"git.log" = "alt-g"              # a plugin's action: "plugin.action"; a config action: its id
                                 # (lvc --plugins lists them; plugins load first, then these move their keys)

[bookmarks]                      # bl
home = "~"
code = "~/development"

[plugins.lvim-fuzzy-plugin-preview]               # the plugins that ship inside lvc are installed only where you say so
[plugins.lvim-fuzzy-plugin-open]

[plugins.lvim-fuzzy-plugin-open.types.pictures]   # Enter on a file: the open plugin's configuration, lvc's own layer
mime   = ["image/*"]
engine = "imv"
[plugins.lvim-fuzzy-plugin-open.engines.imv]
kind    = "command"
command = "imv {}"               # {} = the file
```

The themes are lvim-fuzzy's (`~/.config/lvim-fuzzy/themes`), so both programs wear the same one.

## Build

The lvim-tech crates it builds on — [lvim-fuzzy-core](https://github.com/lvim-tech/lvim-fuzzy-core) (the matcher),
[lvim-fuzzy-ui](https://github.com/lvim-tech/lvim-fuzzy-ui) (the look and the previews), [lvim-fuzzy-plugin](https://github.com/lvim-tech/lvim-fuzzy-plugin)
(the plugin host) and [lvim-fuzzy-commander-core](https://github.com/lvim-tech/lvim-fuzzy-commander-core) (listing, operations, the trash; shared
with the Neovim plugin) — are git dependencies, so **a lone clone builds by itself**; nothing has to be checked out next to it. Next to the other
checkouts (a developer's folder) a `.cargo/config.toml` with `[patch]` entries makes cargo use them instead.

```sh
cargo build --release
scripts/install.sh      # ~/.local/bin/lvc, and the plugin host and the plugins lvc runs through (fetched and built when missing)
```

Nothing of the Neovim side is needed: lvc works without the Neovim plugins, as the Neovim plugin works without lvc.

## The plugins and where each is used

lvc carries no plugin: each is a program of its own (`scripts/install.sh` of its repository). What lvc uses them for:

| Plugin | In lvc |
|---|---|
| `lvim-fuzzy-plugin-git` | the marks column, the branch and counts in the path bar, the `x` group |
| `lvim-fuzzy-plugin-remote` | panels on servers / clouds / mounts, the connections (`r` group) |
| `lvim-fuzzy-plugin-devices` | disks, phones and shares (`g` group) |
| `lvim-fuzzy-plugin-preview` | the quick view and the viewer |
| `lvim-fuzzy-plugin-open` | which program opens a file |

Without a plugin installed, its part is simply not there. `lvc --plugins` lists what is loaded.

## License

BSD-3-Clause

Which program opens which file is the open plugin's configuration (`lvim-fuzzy-plugin-open`, shared with
lvim-fuzzy-cli and the Neovim picker; default: the editor for text, `xdg-open` for the rest), with this config's
`[plugins.lvim-fuzzy-plugin-open]` as lvc's own layer over it. A program that is not installed is said and nothing is
opened. The old `[open]` table is gone and reports where its job went.

## Preview plugins and `[plugins.<name>]`

The quick view asks the preview plugins first (`lvim-fuzzy-plugin-preview` by default): which engine shows a file type is
that plugin's configuration, lvc only draws the answer. lvc's own settings for a plugin go in the config, the layer
over the plugin's defaults and `~/.config/lvim-plugins/<name>.toml`:

```toml
[plugins.lvim-fuzzy-plugin-preview.types.rust]     # .rs files by lvc: shown with bat
ext    = ["rs"]
engine = "bat"

[plugins.lvim-fuzzy-plugin-preview.table]
rows = 200
```

A file no plugin has a preview for goes on to lvc's own providers.
