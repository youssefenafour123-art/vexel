//! Brand identity constants shared by every surface (UI, tray, CLI, installer).
//! Keep these in sync with `BRAND_SYSTEM.md`.

/// Product name.
pub const PRODUCT: &str = "VEXEL";
/// Short ticker.
pub const TICKER: &str = "VXL";
/// OS / sub-brand tag shown in the title bar and splash.
pub const OS_TAG: &str = "VEXEL // SUB-MILLISECOND COMMS";
/// Marketing tagline.
pub const TAGLINE: &str = "Sub-Millisecond Comms. Zero Engine Drag.";
/// URL scheme for Sector Keys (`vxl://s/...`).
pub const URL_SCHEME: &str = "vxl";
/// File extension for theme and soundboard packs.
pub const PACK_EXTENSION: &str = "vxl";
/// HUD segment separator.
pub const SEP: &str = " // ";
/// Crate version, which is also the app version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Join HUD segments with the brand separator: `["A", "B"]` becomes `"A // B"`.
pub fn hud<I, S>(segments: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut out = String::new();
    for (i, s) in segments.into_iter().enumerate() {
        if i > 0 {
            out.push_str(SEP);
        }
        out.push_str(s.as_ref());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hud_joins_with_separator() {
        assert_eq!(
            hud(["ZERO-DRAG ARMED", "31.4 MB"]),
            "ZERO-DRAG ARMED // 31.4 MB"
        );
        assert_eq!(hud(["ONE"]), "ONE");
        assert_eq!(hud(Vec::<&str>::new()), "");
    }

    #[test]
    fn os_tag_uses_separator() {
        assert_eq!(OS_TAG, hud([PRODUCT, "SUB-MILLISECOND COMMS"]));
    }
}
