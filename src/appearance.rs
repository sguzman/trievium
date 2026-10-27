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
                follow_portal(ctx);
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

// Portal settings following, adapted for egui 0.37 from the MIT-licensed
// portal-theme 0.1 by David M. Anderson. D-Bus work never blocks the UI.
#[cfg(target_os = "linux")]
fn portal_preference(value: u32) -> ThemePreference {
    if value == 1 {
        ThemePreference::Dark
    } else {
        ThemePreference::Light
    }
}

#[cfg(target_os = "linux")]
fn scheme_number(value: &zbus::zvariant::Value<'_>) -> Option<u32> {
    let mut current = value;
    for _ in 0..4 {
        match current {
            zbus::zvariant::Value::U32(number) => return Some(*number),
            zbus::zvariant::Value::Value(inner) => current = inner,
            _ => return None,
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn follow_portal(ctx: &egui::Context) {
    let ctx = ctx.clone();
    let _ = std::thread::Builder::new()
        .name("trievium-portal-theme".into())
        .spawn(move || {
            const PORTAL: &str = "org.freedesktop.portal.Desktop";
            const PATH: &str = "/org/freedesktop/portal/desktop";
            const INTERFACE: &str = "org.freedesktop.portal.Settings";
            const NAMESPACE: &str = "org.freedesktop.appearance";
            const KEY: &str = "color-scheme";
            let Ok(connection) = zbus::blocking::Connection::session() else {
                return;
            };
            let Ok(proxy) = zbus::blocking::Proxy::new(&connection, PORTAL, PATH, INTERFACE) else {
                return;
            };
            // No response is not equivalent to light mode; preserve egui's fallback.
            let current: Result<zbus::zvariant::OwnedValue, _> =
                proxy.call("Read", &(NAMESPACE, KEY));
            if let Ok(value) = current
                && let Some(number) = scheme_number(&value)
            {
                ctx.set_theme(portal_preference(number));
                ctx.request_repaint();
            }
            let Ok(changes) = proxy.receive_signal("SettingChanged") else {
                return;
            };
            for message in changes {
                let body = message.body();
                let Ok((namespace, key, value)) =
                    body.deserialize::<(String, String, zbus::zvariant::Value<'_>)>()
                else {
                    continue;
                };
                if namespace == NAMESPACE
                    && key == KEY
                    && let Some(number) = scheme_number(&value)
                {
                    ctx.set_theme(portal_preference(number));
                    ctx.request_repaint();
                }
            }
        });
}

#[cfg(not(target_os = "linux"))]
fn follow_portal(_ctx: &egui::Context) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn portal_scheme_zero_means_light() {
        assert_eq!(portal_preference(0), ThemePreference::Light);
        assert_eq!(portal_preference(1), ThemePreference::Dark);
        assert_eq!(portal_preference(2), ThemePreference::Light);
    }

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
