//! The JSON line Waybar's `custom` module reads (`return-type: json`).

use serde::Serialize;

use crate::pango::escape;
use crate::usage::Severity;

/// `signal: 13` in the Waybar module.
pub const REFRESH_SIGNAL: &str = "-RTMIN+13";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WaybarOutput {
    pub text: String,
    pub tooltip: String,
    /// `low|mid|high|critical`, plus `stale` / `error` for CSS.
    pub class: Vec<&'static str>,
}

impl WaybarOutput {
    pub fn new(text: String, tooltip: String, severity: Severity, stale: bool) -> Self {
        let mut class = vec![severity.as_str()];
        if stale {
            class.push("stale");
        }
        Self {
            text,
            tooltip,
            class,
        }
    }

    pub fn error(msg: &str, icon: Option<&str>) -> Self {
        Self {
            text: with_icon(icon, "⚠"),
            tooltip: escape(msg),
            class: vec!["critical", "error"],
        }
    }

    pub fn loading(icon: Option<&str>) -> Self {
        Self {
            text: with_icon(icon, "…"),
            tooltip: "Aguardando a rede.\nNova tentativa no próximo ciclo do Waybar.".into(),
            class: vec!["low", "stale"],
        }
    }

    pub fn to_json_line(&self) -> String {
        format!("{}\n", serde_json::to_string(self).unwrap_or_default())
    }
}

fn with_icon(icon: Option<&str>, text: &str) -> String {
    match icon.filter(|i| !i.is_empty()) {
        Some(i) => format!("{} {text}", escape(i)),
        None => text.to_string(),
    }
}

/// Ask every Waybar instance to re-run the module now.
pub fn request_refresh() {
    let _ = std::process::Command::new("pkill")
        .args([REFRESH_SIGNAL, "waybar"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_shape() {
        let out = WaybarOutput::new("62%".into(), "tip".into(), Severity::Mid, true);
        assert_eq!(
            out.to_json_line(),
            "{\"text\":\"62%\",\"tooltip\":\"tip\",\"class\":[\"mid\",\"stale\"]}\n"
        );
        let err = WaybarOutput::error("a <b>", Some("󰚩"));
        assert_eq!(err.text, "󰚩 ⚠");
        assert_eq!(err.tooltip, "a &lt;b&gt;");
    }
}
