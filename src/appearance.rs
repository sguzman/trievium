//! Desktop theme following and explicit appearance overrides.
//!
//! System mode follows the XDG Desktop Portal color-scheme preference on
//! Linux, including changes while the picker is open. Forced modes do not
//! subscribe to the portal and therefore cannot be overridden by it.
use eframe::egui::{self, Color32, Theme, ThemePreference};
use std::str::FromStr;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

impl FromStr for Appearance {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "system" => Ok(Self::System),
            "light" => Ok(Self::Light),
            "dark" => Ok(Self::Dark),
            _ => anyhow::bail!("Invalid theme '{value}'; choose system, light, or dark"),
        }
    }
}

impl Appearance {
    /// Apply theme policy once at app creation. The portal follower requests
    /// repaints when the desktop switches modes.
    pub fn configure(self, ctx: &egui::Context) {
        match self {
            Self::System => {
                ctx.set_theme(ThemePreference::System);
                portal_theme::follow(ctx);
            }
            Self::Light => ctx.set_theme(ThemePreference::Light),
            Self::Dark => ctx.set_theme(ThemePreference::Dark),
        }
    }

    pub fn background(theme: Theme) -> Color32 {
        match theme {
            Theme::Light => Color32::WHITE,
            Theme::Dark => Color32::from_rgb(16, 18, 22),
        }
    }

    pub fn warning_color(theme: Theme) -> Color32 {
        match theme {
            Theme::Light => Color32::from_rgb(139, 78, 0),
            Theme::Dark => Color32::from_rgb(240, 168, 90),
        }
    }

    pub fn error_color(theme: Theme) -> Color32 {
        match theme {
            Theme::Light => Color32::from_rgb(160, 29, 29),
            Theme::Dark => Color32::from_rgb(240, 126, 126),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_uses_system_appearance() {
        assert_eq!(Appearance::default(), Appearance::System);
    }

    #[test]
    fn parses_only_supported_modes() {
        for (name, mode) in [
            ("system", Appearance::System),
            ("light", Appearance::Light),
            ("dark", Appearance::Dark),
        ] {
            assert_eq!(name.parse::<Appearance>().unwrap(), mode);
        }
        assert!("blue".parse::<Appearance>().is_err());
        assert!("".parse::<Appearance>().is_err());
    }

    #[test]
    fn explicit_modes_configure_egui() {
        let ctx = egui::Context::default();
        Appearance::Light.configure(&ctx);
        assert_eq!(ctx.theme(), Theme::Light);
        Appearance::Dark.configure(&ctx);
        assert_eq!(ctx.theme(), Theme::Dark);
    }

    #[test]
    fn background_changes_with_theme() {
        assert_eq!(Appearance::background(Theme::Light), Color32::WHITE);
        assert_ne!(
            Appearance::background(Theme::Dark),
            Appearance::background(Theme::Light)
        );
        assert_ne!(
            Appearance::error_color(Theme::Light),
            Appearance::error_color(Theme::Dark)
        );
    }
}
