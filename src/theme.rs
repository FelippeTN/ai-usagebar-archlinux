//! Colors: a One Dark base, neutral tones from HyDE's wallbash palette, then
//! explicit `--color-*` / config overrides.
//!
//! Wallbash derives a monochrome palette from the wallpaper, so it supplies
//! the neutral roles (text, border, dim, empty bar). Severity stays on fixed
//! green/yellow/orange/red: "90% used" has to read as red on any wallpaper.

use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub green: String,
    pub yellow: String,
    pub orange: String,
    pub red: String,
    /// Accent: tooltip border and header.
    pub blue: String,
    pub dim: String,
    pub fg: String,
    pub bar_empty: String,
    pub marker: String,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            green: "#98c379".into(),
            yellow: "#e5c07b".into(),
            orange: "#d19a66".into(),
            red: "#e06c75".into(),
            blue: "#61afef".into(),
            dim: "#5c6370".into(),
            fg: "#abb2bf".into(),
            bar_empty: "#3e4451".into(),
            marker: "#d19a66".into(),
        }
    }
}

/// Severity colors set explicitly by the user.
#[derive(Debug, Clone, Default)]
pub struct ColorOverrides {
    pub low: Option<String>,
    pub mid: Option<String>,
    pub high: Option<String>,
    pub critical: Option<String>,
}

impl Theme {
    /// Re-read on every run, so a wallpaper change shows on the next refresh.
    pub fn load(use_wallbash: bool, overrides: &ColorOverrides) -> Self {
        let mut theme = Self::default();
        if use_wallbash
            && let Some(path) = wallbash_path()
            && let Ok(css) = std::fs::read_to_string(path)
        {
            theme.apply_wallbash(&parse_defines(&css));
        }
        theme.apply_overrides(overrides);
        theme
    }

    pub fn apply_wallbash(&mut self, colors: &HashMap<String, String>) {
        let set = |slot: &mut String, key: &str| {
            if let Some(c) = colors.get(key) {
                *slot = c.clone();
            }
        };
        set(&mut self.fg, "wallbash_txt1");
        set(&mut self.blue, "wallbash_1xa6");
        set(&mut self.dim, "wallbash_1xa4");
        set(&mut self.bar_empty, "wallbash_1xa2");
    }

    pub fn apply_overrides(&mut self, o: &ColorOverrides) {
        for (slot, value) in [
            (&mut self.green, &o.low),
            (&mut self.yellow, &o.mid),
            (&mut self.orange, &o.high),
            (&mut self.red, &o.critical),
        ] {
            if let Some(v) = value.as_ref().filter(|v| is_hex_color(v)) {
                *slot = v.clone();
            }
        }
    }
}

/// `$XDG_CACHE_HOME/hyde/wallbash/gtk.css`.
pub fn wallbash_path() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.cache_dir().join("hyde/wallbash/gtk.css"))
}

/// Collect `@define-color name #RRGGBB;` lines. Anything that is not a plain
/// hex color is skipped: the values end up inside Pango attributes.
pub fn parse_defines(css: &str) -> HashMap<String, String> {
    css.lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("@define-color")?;
            let mut parts = rest.split_whitespace();
            let name = parts.next()?;
            let value = parts.next()?.trim_end_matches(';');
            is_hex_color(value).then(|| (name.to_string(), value.to_string()))
        })
        .collect()
}

pub fn is_hex_color(s: &str) -> bool {
    s.strip_prefix('#')
        .is_some_and(|hex| matches!(hex.len(), 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CSS: &str = "/* header */\n\
        @define-color wallbash_pry1 #0D2137;\n\
        @define-color wallbash_txt1 #FFFFFF;\n\
        @define-color wallbash_1xa2 #3A516B;\n\
        @define-color wallbash_1xa4 #57728F;\n\
        @define-color wallbash_1xa6 #7A9CC2;\n\
        @define-color bad rgba(0,0,0,0);\n\
        @define-color evil #fff' onload='x;\n";

    #[test]
    fn parses_hex_defines_only() {
        let d = parse_defines(CSS);
        assert_eq!(d["wallbash_txt1"], "#FFFFFF");
        assert!(!d.contains_key("bad"));
        assert!(!d.contains_key("evil"));
    }

    #[test]
    fn wallbash_sets_neutrals_but_not_severity() {
        let mut t = Theme::default();
        t.apply_wallbash(&parse_defines(CSS));
        assert_eq!(t.fg, "#FFFFFF");
        assert_eq!(t.blue, "#7A9CC2");
        assert_eq!(t.dim, "#57728F");
        assert_eq!(t.bar_empty, "#3A516B");
        assert_eq!(t.red, Theme::default().red);
    }

    #[test]
    fn overrides_validate_hex() {
        let mut t = Theme::default();
        t.apply_overrides(&ColorOverrides {
            low: Some("#00ff00".into()),
            critical: Some("red".into()),
            ..Default::default()
        });
        assert_eq!(t.green, "#00ff00");
        assert_eq!(t.red, Theme::default().red);
    }
}
