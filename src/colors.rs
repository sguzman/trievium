//! Stable, perceptually spaced entry accents. Hex is persisted only for overrides;
//! entries without one derive their color from their label without migration.
use eframe::egui::Color32;
use std::f32::consts::TAU;

pub fn parse_hex(value: &str) -> Option<Color32> {
    let digits = value.strip_prefix('#')?;
    if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |start| u8::from_str_radix(&digits[start..start + 2], 16).ok();
    Some(Color32::from_rgb(channel(0)?, channel(2)?, channel(4)?))
}

pub fn to_hex(color: Color32) -> String {
    format!("#{:02X}{:02X}{:02X}", color.r(), color.g(), color.b())
}

pub fn for_label(label: &str, override_hex: Option<&str>) -> Color32 {
    override_hex
        .and_then(parse_hex)
        .unwrap_or_else(|| auto_color(label))
}

pub fn auto_color(label: &str) -> Color32 {
    // FNV-1a is stable across launches, machines and reordering (unlike DefaultHasher).
    let mut hash = 0x811c9dc5_u32;
    for byte in label.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x01000193);
    }
    let hue = hash as f32 / u32::MAX as f32 * TAU;
    // OKLCH is cylindrical OKLab. Binary-search chroma to avoid out-of-gamut
    // colors, preserving the lightness/hue instead of clipping RGB channels.
    let (l, mut low, mut high) = (0.72_f32, 0.0_f32, 0.15_f32);
    for _ in 0..15 {
        let mid = (low + high) * 0.5;
        if oklch_linear_rgb(l, mid, hue)
            .iter()
            .all(|value| (0.0..=1.0).contains(value))
        {
            low = mid;
        } else {
            high = mid;
        }
    }
    let rgb = oklch_linear_rgb(l, low, hue);
    Color32::from_rgb(
        linear_to_srgb(rgb[0]),
        linear_to_srgb(rgb[1]),
        linear_to_srgb(rgb[2]),
    )
}

fn oklch_linear_rgb(l: f32, c: f32, h: f32) -> [f32; 3] {
    let a = c * h.cos();
    let b = c * h.sin();
    let ll = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let mm = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let ss = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    [
        4.076_741_7 * ll - 3.307_711_6 * mm + 0.230_969_94 * ss,
        -1.268_438 * ll + 2.609_757_4 * mm - 0.341_319_4 * ss,
        -0.004_196_086_3 * ll - 0.703_418_6 * mm + 1.707_614_7 * ss,
    ]
}

fn linear_to_srgb(value: f32) -> u8 {
    let clamped = value.clamp(0.0, 1.0);
    let srgb = if clamped <= 0.0031308 {
        12.92 * clamped
    } else {
        1.055 * clamped.powf(1.0 / 2.4) - 0.055
    };
    (srgb * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_colors_are_stable_and_custom_overrides_win() {
        assert_eq!(auto_color("Project init"), auto_color("Project init"));
        assert_ne!(auto_color("Project init"), auto_color("Continue"));
        let pink = for_label("Project init", Some("#eB3291"));
        assert_eq!(to_hex(pink), "#EB3291");
        assert_eq!(for_label("Project init", None), auto_color("Project init"));
    }

    #[test]
    fn strict_six_digit_hex_colors_only() {
        for invalid in ["red", "#ffff", "FFFFFF", "#12GG00", "#11223344"] {
            assert!(parse_hex(invalid).is_none(), "{invalid}");
        }
        assert_eq!(to_hex(parse_hex("#0000ff").unwrap()), "#0000FF");
    }
}
