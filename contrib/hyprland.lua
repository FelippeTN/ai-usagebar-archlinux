-- Acrescente em ~/.config/hypr/hyprland.lua
hl.window_rule({
  name = "claude_usagebar_tui",
  match = { class = "^(claude-usagebar-tui)$" },
  float = true,
  center = true,
  size = "875 600",
})
