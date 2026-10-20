//! Optional system-font integration, without embedding or redistributing
//! font files. All font loading is local and falls back to egui defaults.
use eframe::egui::{self, FontData, FontDefinitions, FontFamily, FontId, TextStyle};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

const BODY_FAMILIES: &[&str] = &[
    "Lexica Ultralegible",
    "Lexica Hyperlegible",
    "Atkinson Hyperlegible Next",
    "Atkinson Hyperlegible",
];
const TITLE_FAMILIES: &[&str] = &["Luciole"];

fn matched_font(request: &str, bold: bool) -> Option<PathBuf> {
    let spec = if bold {
        format!("{request}:style=Bold")
    } else {
        request.to_owned()
    };
    let output = Command::new("fc-match")
        .args(["-f", "%{family}\n%{file}\n", &spec])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let result = String::from_utf8(output.stdout).ok()?;
    let mut lines = result.lines();
    let actual = lines.next()?;
    let path = lines.next()?;
    if !actual
        .split(',')
        .any(|name| name.trim().eq_ignore_ascii_case(request))
    {
        return None;
    }
    let font_path = PathBuf::from(path);
    let extension = font_path.extension()?.to_str()?.to_ascii_lowercase();
    if extension != "ttf" && extension != "otf" {
        return None;
    }
    Some(font_path)
}

fn read_installed_font(families: &[&str], bold: bool) -> Option<Vec<u8>> {
    for family in families {
        if let Some(path) = matched_font(family, bold)
            && let Ok(data) = std::fs::read(&path)
            && data.len() <= 8 * 1024 * 1024
        {
            return Some(data);
        }
    }
    None
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let mut title_family = FontFamily::Proportional;

    if let Some(body) = read_installed_font(BODY_FAMILIES, false) {
        fonts
            .font_data
            .insert("trievium_body".into(), Arc::new(FontData::from_owned(body)));
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "trievium_body".into());
    }
    if let Some(title) = read_installed_font(TITLE_FAMILIES, true)
        .or_else(|| read_installed_font(TITLE_FAMILIES, false))
    {
        let name = "trievium_title".to_owned();
        fonts
            .font_data
            .insert(name.clone(), Arc::new(FontData::from_owned(title)));
        let mut fallback = vec![name];
        if let Some(proportional) = fonts.families.get(&FontFamily::Proportional) {
            fallback.extend(proportional.iter().cloned());
        }
        title_family = FontFamily::Name("trievium_heading".into());
        fonts.families.insert(title_family.clone(), fallback);
    }
    ctx.set_fonts(fonts);
    ctx.all_styles_mut(|style| {
        style
            .text_styles
            .insert(TextStyle::Heading, FontId::new(21.0, title_family.clone()));
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(16.0));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::proportional(15.0));
        style
            .text_styles
            .insert(TextStyle::Small, FontId::proportional(13.0));
        style
            .text_styles
            .insert(TextStyle::Monospace, FontId::monospace(14.0));
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configures_typography_without_installed_optional_fonts() {
        let ctx = egui::Context::default();
        install(&ctx);
        let style = ctx.style();
        assert_eq!(style.text_styles.get(&TextStyle::Body).unwrap().size, 16.0);
        assert_eq!(
            style.text_styles.get(&TextStyle::Heading).unwrap().size,
            21.0
        );
    }

    #[test]
    fn explicit_preferred_families_are_ordered_before_fallback() {
        assert_eq!(BODY_FAMILIES[0], "Lexica Ultralegible");
        assert_eq!(TITLE_FAMILIES[0], "Luciole");
    }
}
