use ratatui::style::Color;
use serde::Deserialize;

pub const DEFAULT_ID: &str = "tokyo-night";
const TERMINAL_ID: &str = "terminal";
const PALETTES: &str = include_str!("../../../themes.json");

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemeColors {
    pub bg: Color,
    pub bg_raised: Color,
    pub bg_hover: Color,
    pub bg_active: Color,
    pub fg: Color,
    pub fg_dim: Color,
    pub accent: Color,
    pub green: Color,
    pub yellow: Color,
    pub red: Color,
    pub border: Color,
    pub accent_fg: Color,
    pub error_bg: Color,
    pub warn_bg: Color,
    pub shadow: Color,
    pub syn_key: Color,
    pub syn_string: Color,
    pub syn_number: Color,
    pub syn_comment: Color,
    pub syn_punct: Color,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub id: String,
    pub label: String,
    pub dark: bool,
    pub colors: ThemeColors,
}

#[derive(Deserialize)]
struct ThemeFile {
    id: String,
    label: String,
    dark: bool,
    colors: ColorsFile,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ColorsFile {
    bg: String,
    bg_raised: String,
    bg_hover: String,
    bg_active: String,
    fg: String,
    fg_dim: String,
    accent: String,
    green: String,
    yellow: String,
    red: String,
    border: String,
    accent_fg: String,
    error_bg: String,
    warn_bg: String,
    #[allow(dead_code)]
    shadow: String,
    syn_key: String,
    syn_string: String,
    syn_number: String,
    syn_comment: String,
    syn_punct: String,
}

impl ColorsFile {
    fn resolve(&self) -> ThemeColors {
        ThemeColors {
            bg: hex(&self.bg),
            bg_raised: hex(&self.bg_raised),
            bg_hover: hex(&self.bg_hover),
            bg_active: hex(&self.bg_active),
            fg: hex(&self.fg),
            fg_dim: hex(&self.fg_dim),
            accent: hex(&self.accent),
            green: hex(&self.green),
            yellow: hex(&self.yellow),
            red: hex(&self.red),
            border: hex(&self.border),
            accent_fg: hex(&self.accent_fg),
            error_bg: hex(&self.error_bg),
            warn_bg: hex(&self.warn_bg),
            // shadow is an rgba() string for the DOM; the closest TUI equivalent is Reset
            shadow: Color::Reset,
            syn_key: hex(&self.syn_key),
            syn_string: hex(&self.syn_string),
            syn_number: hex(&self.syn_number),
            syn_comment: hex(&self.syn_comment),
            syn_punct: hex(&self.syn_punct),
        }
    }
}

fn hex(s: &str) -> Color {
    let b = s.as_bytes();
    if b.len() == 7 && b[0] == b'#' {
        let (r, g, bl) = (
            u8::from_str_radix(&s[1..3], 16),
            u8::from_str_radix(&s[3..5], 16),
            u8::from_str_radix(&s[5..7], 16),
        );
        if let (Ok(r), Ok(g), Ok(bl)) = (r, g, bl) {
            return Color::Rgb(r, g, bl);
        }
    }
    Color::Reset
}

/// The Rust-only 16th theme: the terminal's own ANSI palette. `shadow` has no
/// meaningful slot here and stays Reset alongside the backgrounds.
fn terminal_theme() -> Theme {
    use Color as C;
    Theme {
        id: TERMINAL_ID.into(),
        label: "Terminal".into(),
        dark: true,
        colors: ThemeColors {
            bg: C::Reset,
            bg_raised: C::DarkGray,
            bg_hover: C::DarkGray,
            bg_active: C::Gray,
            fg: C::Reset,
            fg_dim: C::DarkGray,
            accent: C::Blue,
            green: C::Green,
            yellow: C::Yellow,
            red: C::Red,
            border: C::DarkGray,
            accent_fg: C::Black,
            error_bg: C::Red,
            warn_bg: C::Yellow,
            shadow: C::Reset,
            syn_key: C::Blue,
            syn_string: C::Green,
            syn_number: C::Yellow,
            syn_comment: C::DarkGray,
            syn_punct: C::DarkGray,
        },
    }
}

fn parse_all() -> Vec<Theme> {
    serde_json::from_str::<Vec<ThemeFile>>(PALETTES)
        .map(|files| {
            files
                .into_iter()
                .map(|t| Theme {
                    id: t.id,
                    label: t.label,
                    dark: t.dark,
                    colors: t.colors.resolve(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// All themes: the 15 from themes.json plus the Rust-only `terminal` theme.
pub fn all() -> Vec<Theme> {
    let mut out = parse_all();
    out.push(terminal_theme());
    out
}

/// Theme by id; unknown ids (and `shadow`, never parsed) fall back to the default.
pub fn get(id: &str) -> Theme {
    all().into_iter().find(|t| t.id == id).unwrap_or_else(|| {
        all()
            .into_iter()
            .find(|t| t.id == DEFAULT_ID)
            .expect("default theme exists")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_16_themes() {
        assert_eq!(all().len(), 16);
    }

    #[test]
    fn json_theme_ids() {
        let all = all();
        let ids: Vec<&str> = all.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "tokyo-night",
                "darcula",
                "vscode-dark-plus",
                "catppuccin-mocha",
                "gruvbox-dark",
                "nord",
                "dracula",
                "one-dark",
                "solarized-dark",
                "blue-kubernetes",
                "catppuccin-latte",
                "vscode-light",
                "solarized-light",
                "github-light",
                "gruvbox-light",
                "terminal",
            ]
        );
    }

    #[test]
    fn known_theme_resolves_to_rgb() {
        let t = get("tokyo-night");
        assert_eq!(t.colors.bg, Color::Rgb(0x16, 0x16, 0x1e));
        assert_eq!(t.colors.accent, Color::Rgb(0x7a, 0xa2, 0xf7));
        assert_eq!(t.colors.error_bg, Color::Rgb(0x3e, 0x27, 0x32));
    }

    #[test]
    fn terminal_theme_uses_ansi_colors() {
        let t = get("terminal");
        assert_eq!(t.colors.green, Color::Green);
        assert_eq!(t.colors.bg, Color::Reset);
    }

    #[test]
    fn unknown_id_falls_back_to_default() {
        assert_eq!(get("nope").id, DEFAULT_ID);
    }
}
