# lvim-fuzzy-commander

`lvim-fuzzy-commander` (short name `lvc`, a symlink the install script makes) — a two-panel file manager for the terminal, in the style of Midnight Commander, with the lvim
look (the lvim-colorscheme themes, icons, panels) and the lvim-fuzzy matcher for filtering.

Status: early development.

- Two panels, each with its own directory, cursor, marks, query and history; Tab switches.
- Typing filters the active panel (fuzzy, as lvim-fuzzy); Esc clears.
- Operations in Rust, no shell: copy, move, rename (one, or many at once in your editor), make a
  directory, change the mode, delete — to the system trash by default, or for good.
- The system trash (the freedesktop.org one, shared with other file managers, `gio trash` and
  `trash-cli`): Ctrl-c t shows it in a panel, where items are restored, deleted for good, or the trash
  emptied.
- Every column in its own colour: the size by magnitude (bytes dim, K green, M yellow, G red), the date by
  age (today green, this week teal, this month blue, this year purple, older dim), the permissions per
  bit, the owner (root red), the group, the type (DIR blue, EXE green, LNK cyan, BRK red, FILE dim; for a directory the size column shows how many entries it holds, `(23)`).
- Changes made by other programs (a file created, deleted, renamed or written; the trash) show at once
  (inotify); a panel whose directory is deleted goes to the nearest one that still exists.
- Long operations run in the background with their progress on the title bar; Esc cancels. A target
  that exists asks: overwrite, skip, keep both — once, or for all the rest.
- Ctrl-c u undoes the last operation (a move or rename moves back, a trashing is restored, a copy or a new
  directory goes to the trash).
- The quick view (F2) shows the file under the cursor in place of the other panel: a real Neovim (taken
  warm from `lvim-fuzzy-cli pool` when it runs), with the built-in preview while Neovim catches up, for
  directories, and as a fallback; an archive shows its listing.
- Tabs: Ctrl-c n opens one (on the same two directories), Ctrl-c w closes it, `gt` / `gT` / `g1`…`g9`
  go between them; Ctrl-c , gives the tab a name of its own (empty: its directory's name again), Ctrl-c < / >
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
- Symbolic links (Ctrl-c L: absolute or relative, in the other panel), the path / name / directory into
  the clipboard (Ctrl-c y / Y / D: OSC 52 and wl-copy / xclip).
- Several lvc at once: each listens on `$XDG_RUNTIME_DIR/lvim-commander/<pid>.sock`; Ctrl-c I lists the
  others (go to its directory, bring it here, copy / move the marked to it), `lvc --list` and
  `lvc --send <pid|all> <instruction>` from a shell or a script.
- Archives: Ctrl-c e extracts the one under the cursor into the other panel, Ctrl-c z packs the marked ones
  (`.tar.gz .tar.xz .tar.bz2 .tar.zst .tar .zip .7z`); the quick view lists them, and also `.rar`, and
  `.deb .rpm .iso .cpio .cab .xar .tar.lz` through `bsdtar` (libarchive also stands in for a missing
  `unzip` / `7z` / `unrar`).
- Ctrl-c f / Ctrl-c g run the lvim-fuzzy finder (files / live grep) from the panel's directory and bring the
  panel to the result.
- Material file icons as IMAGES (as lvim-icons draws them in Neovim) where the terminal takes kitty
  placeholders — kitty, ghostty, zellij built with them — else Nerd Font glyphs (`icons = "auto"`).
- Two modes: normal (`j` / `k`, `gg` / `G`, `h` / `l`, Space marks, `.` hidden files) and input
  (Ctrl-f or `/`: typing filters; Enter / Esc back).
- F9: a menu of every operation and every user action; extensible — see [Extending](#extending).
- Bookmarks and the recently visited directories (Ctrl-c b), go to a typed path (Ctrl-c c), a shell command in
  the panel's directory (Ctrl-c x), the editor (F6), opening by extension (`[open]`, else the editor for
  text and `xdg-open` for the rest).

## Keys

The F-keys and the moving keys are direct; **every other command is Ctrl-c, then a key** — after Ctrl-c
the bottom shows them all (as Neovim's key helper does). F1 lists everything. Ctrl-c is the **leader**;
`leader = "ctrl-a"` (any Ctrl- or Alt- key, not Ctrl-j / Ctrl-k) changes it, and every `Ctrl-c` in this text is
then that key. Every command's key can be moved in `[keys]` (below).

| Key | |
|---|---|
| Tab · Ctrl-h · Ctrl-l | the other panel · the left one · the right one |
| j k · gg G · ↑ ↓ PgUp PgDn Home End | move · top / bottom of the panel |
| l · Enter · → | enter a directory, open a file |
| h · Backspace · ← | up one directory |
| Ctrl-f · / | input mode: typing filters (fuzzy); Enter / Esc back |
| Space · Insert | mark and move down |
| . | hidden files |
| J · K | the quick view: the next / previous page of a PDF or document, point of a video (5 % steps); else scroll 3 lines |
| Ctrl-e · Ctrl-y | scroll the quick view one line |
| Ctrl-d · Ctrl-u | half a page down / up — of the quick view when it is open (a Neovim preview gets the keys), else of the list |
| F1 | help |
| F2 | quick view |
| F3 | copy (to the other panel; the target can be edited) |
| F4 | rename (with several marked: all of them in the editor) |
| F5 | move (to the other panel; the target can be edited) |
| F6 | edit — a file, or a directory (the editor opens it there) |
| F7 | new: a file, or a directory when the name ends in `/` (`a/b/` and `a/b/c.txt` make the directories on the way) |
| F8 · Delete | to the trash (`delete = "direct"`: for good) |
| Shift-F8 | delete for good |
| F9 | the menu: every operation and user action |
| F10 · Ctrl-q | quit |
| Ctrl-j | onto the bottom bar: its buttons as real buttons — h / l (← →, Tab) choose, Enter or Space presses, Ctrl-k (or Esc, k) back to the panel it came from; the mouse presses them too |

After **Ctrl-c**:

| Key | |
|---|---|
| f · g | find files · grep (lvim-fuzzy) |
| c · b | go to a directory · bookmarks and recent directories |
| x | run a shell command here |
| t | the trash (there: F5 or r restore, F8 delete for good, e empty) |
| m | permissions: ticks (owner / group / others × read / write / exec, setuid / setgid / sticky) and a field for octal (`755`) or symbolic (`u+x,g-w,o=r`, `+X`, applied to each item's own mode) — a tick changes the field, the field changes the ticks |
| O | owner and group: drop-downs of the real users (root and the people's accounts) and groups (yours first); typing filters a drop-down |
| z · e | compress the marked · extract |
| u | undo the last operation |
| a · * · + · - | mark all · invert · mark / unmark by glob |
| h · l · s · S | hidden files · long view (permissions, owner, group) · sort · reverse |
| n · w | a new tab · close this tab (`gt` `gT` `g1`…`g9` go between them) |
| , · < · > | rename this tab (empty: back to its directory's name) · move it left · right |
| I | the other running lvc |
| y · Y · D | copy the path(s) · the name(s) · the panel's directory to the clipboard |
| L | symbolic links to the marked in the other panel (Tab in the dialog: absolute / relative) |
| v | the columns beside the names — type, permissions, owner, group, size, date — each on / off (remembered) |
| p | swap the panels |
| o · i | the directory under the cursor / this one in the other panel |
| [ · ] | back / forward in the history |
| r | reload |
| 1 … 0 | F1 … F10, for a terminal (or multiplexer) that keeps the F-keys |
| H | show / hide this bar of hints |
| ? · q | help · quit |

## Icons

The Material Icon Theme set lives in ONE place for every lvim program — `~/.local/share/lvim-icons/material`
(lvim-icons in Neovim, lvim-fuzzy, lvim-fuzzy-commander). Each carries the set and puts it there when it is missing
or older (never a downgrade, swapped in atomically): `scripts/install.sh` runs `lvc --install-icons`, and
`lvc` does it at start in image mode. `icons = "font"` keeps the glyphs; `zellij_images = false` for a zellij
that does not take kitty placeholders.

## Extending

```toml
[actions.gitlog]                  # a key and / or an entry of the F9 menu → a command
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

The plugin host itself — `plugin.toml`, the runtimes, the `lvim-fuzzy-plugin` tool (`list`, `check`, `host`) and the SDK — is
the separate **lvim-fuzzy-plugin** crate, shared with lvim-fuzzy-cli and lvim-picker; what follows is how lvc uses it.

A plugin can name a **runtime** in `plugin.toml` (`runtime = "command"` — the default, a process per action —
`"process"`, `"native"`, `"wasm"` or `"lua"`). Its actions then say `call = "function"` instead of a
`command`. Every runtime speaks the same JSON: lvc sends the context (`call`, `arg`, `file`, `files`, `dir`,
`other_dir`, `panel`) and the plugin answers with an array of instructions: `{"op":"cd","path":...}`,
`other-cd`, `select`/`mark` (`name`), `reload`, `message`/`error` (`text`), and the interactive ones —
`input` (`prompt`, `value`, `then`), `confirm` (`text`, `then`), `menu` (`title`, `items` of `label` + `call`),
`preview` (`text`). `lvc --plugins` shows each plugin's runtime and whether its script, module or library loads.

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

## Several instances

```sh
lvc --list                          # pid, the left and right directories (* = the active panel)
lvc --send 12345 cd ~/Downloads     # cd · other-cd · select NAME · mark NAME · reload · message TEXT
lvc --send all reload
```

## Leaving in another directory

`lvc --print-dir` prints the active panel's directory when it ends. `lvc --zsh` (or `--bash`) prints a
shell function for that:

```sh
eval "$(lvc --zsh)"   # ~/.zshrc: then `lvcd` opens lvc and cds to where you left it
```

## State

`lvc` remembers whether the quick view (F2) was open and which columns show (Ctrl-c v), in `~/.local/state/lvim-commander/state.toml`
(`$XDG_STATE_HOME`): it starts as you left it.

## Config

`~/.config/lvim-commander/config.toml` (`$LVIM_COMMANDER_CONFIG`); every key is optional:

```toml
theme   = "LvimEverforest_dark"  # an lvim theme; default: lvim-fuzzy's (~/.config/lvim-fuzzy)
delete  = "trash"                # F8: "trash" | "direct"
hidden  = false                  # show dotfiles at start
key_hints = true                 # the bar of the Ctrl-c commands (Ctrl-c H toggles it)
watch   = true                   # show at once what other programs change in the shown directories
editor  = "nvim"                 # default: $VISUAL, $EDITOR, nvim
preview = "nvim"                 # the quick view: "nvim" | "file" | "off"
nvim    = "nvim"
pool    = true                   # take the preview's Neovim from `lvim-fuzzy-cli pool`
image_transfer = "file"          # pictures: the terminal reads a temp file (fast); "direct" sends the bytes

leader  = "ctrl-c"               # the key that starts a command; "ctrl-a", "alt-x", ...

[keys]                           # any command on another key (F1 shows the new ones, the bar too)
copy = "f5"                      # the F-key actions: help view copy rename move edit new trash delete menu quit
move = "f3"                      # a key whose command moved elsewhere does nothing
edit = "leader k"                # "f5", "shift-f8", "leader x" (after the leader), "alt-x", "ctrl-x", "tab", "space"
find = "alt-f"                   # the commands after the leader: find grep goto bookmarks shell trash-view restore
up   = "ctrl-p"                  # empty-trash chmod chown compress extract undo mark-all invert-marks mark-glob ...
"git.log" = "alt-g"              # a plugin's action: "plugin.action"; a config action: its id
                                 # (lvc --plugins lists them; plugins load first, then these move their keys)

[bookmarks]                      # Ctrl-c b
home = "~"
code = "~/development"

[plugins.lvim-fuzzy-plugin-open.types.pictures]   # Enter on a file: the open plugin's configuration, lvc's own layer
mime   = ["image/*"]
engine = "imv"
[plugins.lvim-fuzzy-plugin-open.engines.imv]
kind    = "command"
command = "imv {}"               # {} = the file
```

The themes are lvim-fuzzy's (`~/.config/lvim-fuzzy/themes`), so both programs wear the same one.

## Build

lvim-fuzzy-commander builds on two sibling crates: [lvim-fuzzy-core](https://github.com/lvim-tech/lvim-fuzzy-core)
(the matcher) and [lvim-fuzzy-ui](https://github.com/lvim-tech/lvim-fuzzy-ui) (the look and the previews,
shared with lvim-fuzzy), checked out next to it.

```sh
cargo build --release
scripts/install.sh      # ~/.local/bin/lvc
```

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
