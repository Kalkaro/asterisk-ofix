//! Palette, type, the embedded Google Sans Flex font, icons, and the rune-kit theme.

use rune::{Color, Icon, TextStyle};
use rune_kit::{Kit, PaletteSeed};

/// Colours are linear sRGB; each constant is decoded from the sRGB hex beside it.
const fn lin(r: f32, g: f32, b: f32) -> Color {
    Color { r, g, b, a: 1.0 }
}

// Neutral greys on pure black, white for the primary action, no blue anywhere.
pub const BACKGROUND: Color = Color::BLACK;
pub const SIDEBAR: Color = lin(0.00605, 0.00605, 0.00750) /* #121215 */;
pub const RAISED: Color = lin(0.01370, 0.01370, 0.01764) /* #1f1f24 */;
pub const BORDER: Color = lin(0.01938, 0.01938, 0.02519) /* #26262c */;
pub const TEXT: Color = lin(0.83880, 0.83880, 0.86316) /* #ececef */;
pub const MUTED: Color = lin(0.26225, 0.26225, 0.30054) /* #8c8c95 */;
pub const FAINT: Color = lin(0.10702, 0.10702, 0.12744) /* #5c5c64 */;
pub const PRIMARY: Color = lin(0.87137, 0.87137, 0.88792) /* #f0f0f2 */;
pub const GREEN: Color = lin(0.07819, 0.63760, 0.23074) /* #4fd184 */;

// Google Sans Flex, cut to static Regular and SemiBold instances from the variable font
// with its roundness axis all the way up (ROND=100), so every text, kit controls included,
// is rounded (fonts/OFL.txt). Both instances name their family "Google Sans Flex".
pub const FONTS: &[&[u8]] = &[
    include_bytes!("../../fonts/GoogleSansFlex-Regular.ttf"),
    include_bytes!("../../fonts/GoogleSansFlex-SemiBold.ttf"),
];
pub const FAMILIES: (&str, &str) = ("Google Sans Flex", "ui-monospace");

pub const BODY: TextStyle = TextStyle::new(14.0, 400.0).lh(20.0);
pub const BODY_STRONG: TextStyle = TextStyle::new(14.0, 600.0).lh(20.0);
pub const CAPTION: TextStyle = TextStyle::new(12.0, 400.0).lh(16.0);
pub const CAPTION_STRONG: TextStyle = TextStyle::new(12.0, 600.0).lh(16.0);
pub const SUBHEADING: TextStyle = TextStyle::new(15.0, 600.0).lh(20.0);
pub const PAGE_TITLE: TextStyle = TextStyle::new(22.0, 600.0).lh(28.0);
pub const MONOGRAM: TextStyle = TextStyle::new(34.0, 600.0).lh(40.0);

/// The rune-kit theme: its controls in the palette above.
pub fn kit() -> Kit {
    let mut k = Kit::from_palette(PaletteSeed { bg: BACKGROUND, fg: TEXT, accent: PRIMARY });
    k.c.border_focus = Color::WHITE.alpha(0.35);
    k.c.text_muted = MUTED;
    k.c.text_faint = FAINT;
    k.c.border_base = BORDER;
    k.ty.ui = BODY;
    k.ty.ui_medium = BODY_STRONG;
    k.ty.ui_compact = TextStyle::new(13.0, 400.0).lh(16.0);
    k.ty.small = CAPTION;
    k.ty.small_medium = CAPTION_STRONG;
    k.radius.sm = 8.0;
    k.radius.md = 8.0;
    k
}

/// A muted, stable background for a game's placeholder tile.
pub fn tile_color(name: &str) -> Color {
    const TONES: [Color; 6] = [
        lin(0.03434, 0.02416, 0.01938), /* #342b26 */
        lin(0.02416, 0.03190, 0.02416), /* #2b322b */
        lin(0.03310, 0.02217, 0.03071), /* #332931 */
        lin(0.03190, 0.02956, 0.01938), /* #323026 */
        lin(0.02519, 0.02519, 0.03310), /* #2c2c33 */
        lin(0.01938, 0.02956, 0.02956), /* #263030 */
    ];
    let h = name.chars().fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(c as u32));
    TONES[(h % TONES.len() as u32) as usize]
}

/// Up to two letters or digits from the start of a name's words.
pub fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|w| w.chars().next())
        .filter(|c| c.is_alphanumeric())
        .take(2)
        .flat_map(char::to_uppercase)
        .collect()
}

macro_rules! icons {
    ($($name:ident = $body:literal;)*) => {
        $(pub static $name: Icon = Icon { name: concat!("asterisk-ofix-", stringify!($name)), view: 16.0, body: $body };)*
    };
}

icons! {
    PLAY = r#"<path d="M5 3.2v9.6a.6.6 0 0 0 .9.5l7.6-4.8a.6.6 0 0 0 0-1L5.9 2.7a.6.6 0 0 0-.9.5Z" fill="currentColor"/>"#;
    REFRESH = r#"<path d="M13 8a5 5 0 1 1-1.5-3.6M13 2.5v3h-3" stroke="currentColor" stroke-width="1.3" fill="none" stroke-linecap="round" stroke-linejoin="round"/>"#;
    PLUS = r#"<path d="M8 3v10M3 8h10" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>"#;
    FOLDER = r#"<path d="M2 4.5A1.5 1.5 0 0 1 3.5 3h2.6l1.5 1.5h4.9A1.5 1.5 0 0 1 14 6v5.5a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 11.5Z" stroke="currentColor" stroke-width="1.2" fill="none" stroke-linejoin="round"/>"#;
    FOLDER_PLUS = r#"<path d="M2 4.5A1.5 1.5 0 0 1 3.5 3h2.6l1.5 1.5h4.9A1.5 1.5 0 0 1 14 6v5.5a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 11.5Z" stroke="currentColor" stroke-width="1.2" fill="none" stroke-linejoin="round"/><path d="M8 6.8v4M6 8.8h4" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>"#;
    DOCUMENT = r#"<path d="M4 2.5h5l3 3v8H4Z" stroke="currentColor" stroke-width="1.2" fill="none" stroke-linejoin="round"/><path d="M9 2.5v3h3M6 8.5h4M6 10.5h4" stroke="currentColor" stroke-width="1.2" fill="none" stroke-linecap="round"/>"#;
    EMPTY = r#"<path d="M2 4.5A1.5 1.5 0 0 1 3.5 3h2.6l1.5 1.5h4.9A1.5 1.5 0 0 1 14 6v5.5a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 11.5Z" stroke="currentColor" stroke-width="0.6" fill="none" stroke-linejoin="round"/>"#;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_of_names() {
        for (name, want) in [
            ("Hidden Road", "HR"),
            ("pong", "P"),
            ("Zone 4", "Z4"),
            ("Sky Ride Deluxe", "SR"),
            ("", ""),
            ("- dash", "D"),
        ] {
            assert_eq!(initials(name), want, "{name:?}");
        }
    }

    #[test]
    fn tile_colour_is_stable_and_dark() {
        assert_eq!(tile_color("Pong"), tile_color("Pong"));
        let c = tile_color("Pong");
        // #404040 in linear light.
        assert!(c.a == 1.0 && c.r.max(c.g).max(c.b) <= 0.0513, "{c:?}");
    }

    #[test]
    fn embedded_fonts_are_truetype() {
        for data in FONTS {
            assert!(data.len() > 10_000 && data.starts_with(&[0, 1, 0, 0]), "not an embedded TrueType file");
        }
    }

    /// The instances are cut at ROND=100. Their STAT table then names that axis value
    /// "Rounded" (UTF-16BE in the name table); at ROND=0 it's "Default".
    #[test]
    fn embedded_fonts_are_rounded() {
        let rounded: Vec<u8> = "Rounded".encode_utf16().flat_map(u16::to_be_bytes).collect();
        for data in FONTS {
            assert!(data.windows(rounded.len()).any(|w| w == rounded), "a font isn't the rounded (ROND=100) cut");
        }
    }
}
