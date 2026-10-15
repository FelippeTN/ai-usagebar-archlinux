# claude-usagebar

Mostra os limites de uso do **Claude Code** (sessão de 5h, semanal, Sonnet, limites por modelo, uso extra) no **Waybar**, com um painel TUI flutuante. Feito para Arch + Hyprland + HyDE.

É uma reimplementação em Rust, só para Claude e só para Linux, das ideias do [akitaonrails/ai-usagebar](https://github.com/akitaonrails/ai-usagebar).

## Como funciona

- O token OAuth vem de `~/.claude/.credentials.json` (ou `$CLAUDE_CONFIG_DIR`). **Esse arquivo só é lido, nunca escrito.**
- Os dados vêm de `GET https://api.anthropic.com/api/oauth/usage?cedar_ember=1`, com o header `anthropic-beta: oauth-2025-04-20` e um User-Agent do Claude Code.
- O widget **nunca renova o token**. Quando ele expira, a barra mostra os últimos dados do cache com `⏸`, e o tooltip pede para abrir o `claude` (o Claude Code renova o token sozinho quando é usado).
- A resposta fica em cache em `~/.cache/claude-usagebar/`:
  - Validade de 60 s, então vários monitores fazem uma única requisição.
  - Dados antigos são servidos como fallback por até 7 dias.
  - Depois de um HTTP 429, nenhuma requisição é feita por 5 minutos.
- O binário sempre sai com código 0. Qualquer erro vira um `⚠` com a explicação no tooltip.
- As cores neutras (texto, borda, cinza) vêm do wallbash do HyDE (`~/.cache/hyde/wallbash/gtk.css`). As cores de severidade continuam fixas:
  - verde abaixo de 50%
  - amarelo a partir de 50%
  - laranja a partir de 75%
  - vermelho a partir de 90%

## Instalação

```sh
sudo pacman -S --needed rustup make
make install          # instala em ~/.local/bin
```

Não é preciso rodar `rustup default`: o `rust-toolchain.toml` do repositório faz o rustup baixar e usar a versão stable sozinho. O `Cargo.lock` versionado garante as mesmas versões de dependências que foram testadas.

### Waybar (HyDE)

1. Copie o módulo:
   ```sh
   mkdir -p ~/.config/waybar/modules
   cp contrib/custom-claude.jsonc ~/.config/waybar/modules/
   ```
2. Acrescente `"custom/claude"` a um dos grupos em `~/.config/waybar/config.jsonc`, por exemplo no `group/pill#right2`.
   > O HyDE regenera o `config.jsonc` quando você troca de layout. Para não perder o módulo, salve uma cópia do layout em `~/.config/waybar/layouts/`.
3. Cole `contrib/waybar.css` em `~/.config/waybar/user-style.css`.
4. Recarregue: `killall -SIGUSR2 waybar`.

### Hyprland (TUI flutuante)

Cole `contrib/hyprland.lua` no final de `~/.config/hypr/hyprland.lua`.

## Uso

| Comando | O que faz |
|---|---|
| `claude-usagebar` | Imprime o JSON do Waybar (num terminal, imprime uma versão legível) |
| `claude-usagebar --pretty` | Força a saída legível |
| `claude-usagebar --refresh` | Ignora a validade do cache e pede para o Waybar redesenhar (`pkill -RTMIN+13 waybar`) |
| `claude-usagebar --format '{session_pct}% {session_pace} · S {weekly_pct}%'` | Texto customizado |
| `claude-usagebar-tui` | Abre o painel (`r` atualiza, `q` sai) |

As classes CSS emitidas são `low`, `mid`, `high` e `critical`, mais `stale` (dados do cache) e `error`.

O arquivo de configuração é opcional e fica em `~/.config/claude-usagebar/config.toml`. Veja [config.example.toml](config.example.toml) com todas as chaves e placeholders.

## Aviso

O endpoint `/api/oauth/usage` não é documentado nem suportado oficialmente pela Anthropic. Ele pode mudar ou deixar de funcionar sem aviso. Se o User-Agent mínimo exigido mudar, ajuste `user_agent` no config.

## Licença

MIT, veja [LICENSE](LICENSE). Derivado do [ai-usagebar](https://github.com/akitaonrails/ai-usagebar) de AkitaOnRails, também sob MIT.

## Desenvolvimento

```sh
make test
make lint
```
