#!/bin/sh
# Build lvim-commander and install it for the current user:
#   ~/.local/bin/lvim-commander               the binary (PREFIX overrides ~/.local)
#   ~/.local/bin/lvc                          a short name (symlink)
#   ~/.config/lvim-commander/themes/*.toml    the lvim themes, from an lvim-colorscheme checkout next to
#                                             this one (LVIM_COLORSCHEME=path); existing files are kept
set -e
cd "$(dirname "$0")/.."
prefix=${PREFIX:-$HOME/.local}
cargo build --release
mkdir -p "$prefix/bin"
install -m 755 target/release/lvim-commander "$prefix/bin/lvim-commander"
ln -sf lvim-commander "$prefix/bin/lvc"
echo "installed: $prefix/bin/lvim-commander, $prefix/bin/lvc"
# the Material icons, shared with lvim-icons and lvim-fuzzy: written when missing there or older
"$prefix/bin/lvim-commander" --install-icons
themes=${XDG_CONFIG_HOME:-$HOME/.config}/lvim-commander/themes
src=${LVIM_COLORSCHEME:-../lvim-colorscheme}/extras/lvim_commander
if [ -d "$src" ]; then
    mkdir -p "$themes"
    n=0
    for f in "$src"/*.toml; do
        [ -e "$themes/$(basename "$f")" ] || { cp "$f" "$themes/"; n=$((n + 1)); }
    done
    echo "themes: $n added to $themes ($(ls "$themes" | wc -l) in all)"
else
    echo "themes: no lvim-colorscheme checkout at $src (LVIM_COLORSCHEME=path); lvim-fuzzy's themes are used"
fi
echo "for lvcd (cd to where you left it): eval \"\$(lvim-fuzzy-commander --zsh)\" in ~/.zshrc"
