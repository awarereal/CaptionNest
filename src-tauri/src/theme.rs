//! The interface theme's effect on the native window: light or dark window chrome and
//! native dialogs (file pickers, menus), and the colour shown before the page paints,
//! so a light theme doesn't flash dark at launch. The themes themselves are CSS
//! (`src/styles/tokens.css`); the ids here must match `src/lib/theme.ts`.

use tauri::window::Color;
use tauri::{AppHandle, Manager, Theme};

/// The page background (`--booth`) of a theme, and whether it's light.
fn look(id: &str) -> (Color, bool) {
    match id {
        "daylight" => (Color(0xeb, 0xe6, 0xdd, 0xff), true),
        "midnight" => (Color(0x0b, 0x10, 0x19, 0xff), false),
        "contrast" => (Color(0x00, 0x00, 0x00, 0xff), false),
        // "tungsten", and anything unknown (a newer settings file), is the default.
        _ => (Color(0x14, 0x11, 0x0e, 0xff), false),
    }
}

/// Applies a theme id from the settings to the main window.
pub fn apply(app: &AppHandle, id: &str) {
    let Some(window) = app.get_webview_window("main") else { return };
    let (background, light) = if id == "system" {
        // Follow the computer, then pick the matching theme's colour.
        let _ = window.set_theme(None);
        let light = matches!(window.theme(), Ok(Theme::Light));
        look(if light { "daylight" } else { "tungsten" })
    } else {
        let (background, light) = look(id);
        let _ = window.set_theme(Some(if light { Theme::Light } else { Theme::Dark }));
        (background, light)
    };
    let _ = window.set_background_color(Some(background));
    log::debug!("theme {id} (light: {light})");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_themes_fall_back_to_the_default() {
        assert_eq!(look("something-new").0, look("tungsten").0);
        assert!(look("daylight").1);
        assert!(!look("contrast").1);
    }
}
