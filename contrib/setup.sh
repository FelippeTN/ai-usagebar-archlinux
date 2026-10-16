#!/usr/bin/env bash
# Integra o claude-usagebar ao Waybar (com suporte ao HyDE) e ao Hyprland.
#
#   contrib/setup.sh              instala (pode rodar de novo, é idempotente)
#   contrib/setup.sh --uninstall  desfaz
#
# No HyDE, o config.jsonc é recopiado do layout ativo a cada troca de papel de
# parede ou tema, então o módulo vai num layout próprio em
# ~/.config/waybar/layouts/ (que o HyDE nunca sobrescreve), derivado do ativo.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
config_home="${XDG_CONFIG_HOME:-$HOME/.config}"
state_home="${XDG_STATE_HOME:-$HOME/.local/state}"
waybar_dir="$config_home/waybar"
module_file="$waybar_dir/modules/custom-claude.jsonc"
user_css="$waybar_dir/user-style.css"
hypr_lua="$config_home/hypr/hyprland.lua"
hyde_state="$state_home/hyde/staterc"
saved_layout="$config_home/claude-usagebar/hyde-previous-layout"
begin_mark="claude-usagebar:begin"
end_mark="claude-usagebar:end"

say() { printf '\033[1;34m::\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m!!\033[0m %s\n' "$*" >&2; }

is_hyde() { command -v hyde-shell >/dev/null && [[ -f "$hyde_state" ]]; }

hyde_layout() {
    sed -n 's/^WAYBAR_LAYOUT_PATH=//p' "$hyde_state" | tail -n1
}

# Appends $2 to $1 between markers, unless the markers or $3 (text that a
# hand-pasted copy would contain) are already there.
append_block() {
    local file="$1" block="$2" signature="$3"
    if [[ -f "$file" ]] && grep -qF -e "$begin_mark" -e "$signature" "$file"; then
        say "$(basename "$file"): já configurado"
        return
    fi
    mkdir -p "$(dirname "$file")"
    printf '\n%s\n' "$block" >>"$file"
    say "$(basename "$file"): bloco adicionado"
}

# Removes the marked block from $1.
remove_block() {
    local file="$1"
    [[ -f "$file" ]] && grep -q "$begin_mark" "$file" || return 0
    sed -i "/$begin_mark/,/$end_mark/d" "$file"
    say "$(basename "$file"): bloco removido"
}

# Prints the path of a copy of layout $1 with "custom/claude" inserted.
derive_layout() {
    local src="$1" rel name dst
    rel="${src#*/waybar/layouts/}"
    name="$(basename "$rel" .jsonc)"
    # "<name>#claude" keeps HyDE's style lookup (it strips the "#…" suffix).
    dst="$waybar_dir/layouts/$(dirname "$rel")/${name%%#*}#claude.jsonc"
    dst="${dst/\/.\//\/}"
    mkdir -p "$(dirname "$dst")"
    python3 -I - "$src" "$dst" <<'EOF'
import re, sys
src, dst = sys.argv[1], sys.argv[2]
text = open(src, encoding="utf-8").read()
# Prefer the right-hand pill used by HyDE's default layouts, else the start
# of modules-right.
match = re.search(r'"group/pill#right2"\s*:\s*\{[^{}]*?"modules"\s*:\s*\[', text, re.S) \
    or re.search(r'"modules-right"\s*:\s*\[', text)
if not match:
    sys.exit(f"não encontrei modules-right em {src}")
text = text[:match.end()] + '\n            "custom/claude",' + text[match.end():]
open(dst, "w", encoding="utf-8").write(text)
EOF
    printf '%s\n' "$dst"
}

install_layout() {
    if ! is_hyde; then
        if grep -qs '"custom/claude"' "$waybar_dir/config.jsonc"; then
            say "config.jsonc: módulo já presente"
        else
            warn "HyDE não encontrado: acrescente \"custom/claude\" a modules-right em $waybar_dir/config.jsonc"
        fi
        return
    fi
    local current new
    current="$(hyde_layout)"
    if [[ -z "$current" || ! -f "$current" ]]; then
        warn "não achei o layout ativo do HyDE; acrescente \"custom/claude\" ao seu layout manualmente"
        return
    fi
    if grep -q '"custom/claude"' "$current"; then
        say "layout do HyDE já tem o módulo: $current"
        return
    fi
    new="$(derive_layout "$current")"
    mkdir -p "$(dirname "$saved_layout")"
    printf '%s\n' "$current" >"$saved_layout"
    hyde-shell waybar --set "$new" >/dev/null
    say "layout do HyDE: $(basename "$current") -> $new"
}

uninstall_layout() {
    is_hyde && [[ -f "$saved_layout" ]] || return 0
    local previous current
    previous="$(cat "$saved_layout")"
    current="$(hyde_layout)"
    if [[ -f "$previous" ]]; then
        hyde-shell waybar --set "$previous" >/dev/null
        say "layout do HyDE restaurado: $previous"
    fi
    [[ "$current" == *"#claude.jsonc" ]] && rm -f "$current"
    rm -f "$saved_layout"
}

reload_waybar() {
    pkill -SIGUSR2 -x waybar 2>/dev/null && say "Waybar recarregado" || true
}

do_install() {
    local bin="${PREFIX:-$HOME/.local}/bin"
    if [[ ! -x "$bin/claude-usagebar" ]]; then
        warn "$bin/claude-usagebar não existe; rode 'make install' antes"
        exit 1
    fi
    command -v kitty >/dev/null ||
        warn "kitty não encontrado: ajuste o on-click em $module_file para o seu terminal"

    mkdir -p "$(dirname "$module_file")"
    install -m644 "$here/custom-claude.jsonc" "$module_file"
    say "módulo: $module_file"

    append_block "$user_css" "$(cat "$here/waybar.css")" "#custom-claude"
    install_layout

    if [[ -f "$hypr_lua" ]]; then
        append_block "$hypr_lua" "$(cat "$here/hyprland.lua")" "claude-usagebar-tui"
    else
        warn "hyprland.lua não encontrado; regra da janela flutuante em $here/hyprland.lua"
    fi

    reload_waybar
    say "pronto. Passe o mouse no 󰚩 da barra; clique para abrir o painel."
}

do_uninstall() {
    uninstall_layout
    rm -f "$module_file" && say "módulo removido"
    remove_block "$user_css"
    remove_block "$hypr_lua"
    reload_waybar
}

case "${1:-}" in
    "") do_install ;;
    --uninstall) do_uninstall ;;
    *)
        echo "uso: $0 [--uninstall]" >&2
        exit 2
        ;;
esac
