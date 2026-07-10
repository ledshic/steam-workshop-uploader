//! Runtime app / Dock icon switching (light / dark / system).

use tauri::{AppHandle, Manager, Runtime};

const ICON_LIGHT_PNG: &[u8] = include_bytes!("../icons/app-icon-light.png");
const ICON_DARK_PNG: &[u8] = include_bytes!("../icons/app-icon-dark.png");

/// Preference stored / sent from the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconThemePref {
    Light,
    Dark,
    System,
}

impl IconThemePref {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "light" => Ok(Self::Light),
            "dark" => Ok(Self::Dark),
            "system" | "auto" => Ok(Self::System),
            other => Err(format!(
                "Unknown icon theme '{other}'. Use light, dark, or system."
            )),
        }
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedIconTheme {
    Light,
    Dark,
}

/// Whether the OS is currently in dark appearance.
pub fn is_system_dark() -> bool {
    #[cfg(target_os = "macos")]
    {
        // `defaults read -g AppleInterfaceStyle` → "Dark" when dark; fails in light mode.
        std::process::Command::new("defaults")
            .args(["read", "-g", "AppleInterfaceStyle"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().eq_ignore_ascii_case("Dark"))
            .unwrap_or(false)
    }
    #[cfg(target_os = "windows")]
    {
        // AppsUseLightTheme = 0 → dark
        use std::process::Command;
        let output = Command::new("reg")
            .args([
                "query",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
                "/v",
                "AppsUseLightTheme",
            ])
            .output();
        match output {
            Ok(o) if o.status.success() => {
                let text = String::from_utf8_lossy(&o.stdout);
                text.contains("0x0") || text.lines().any(|l| {
                    l.contains("AppsUseLightTheme") && l.trim_end().ends_with('0')
                })
            }
            _ => false,
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        // Best-effort via env (rarely set)
        std::env::var("GTK_THEME")
            .map(|v| v.to_ascii_lowercase().contains("dark"))
            .unwrap_or(false)
            || std::env::var("COLORFGBG")
                .ok()
                .and_then(|v| v.split(';').nth(1)?.parse::<u8>().ok())
                .map(|bg| bg < 8)
                .unwrap_or(false)
    }
}

pub fn resolve_theme(pref: IconThemePref) -> ResolvedIconTheme {
    match pref {
        IconThemePref::Light => ResolvedIconTheme::Light,
        IconThemePref::Dark => ResolvedIconTheme::Dark,
        IconThemePref::System => {
            if is_system_dark() {
                ResolvedIconTheme::Dark
            } else {
                ResolvedIconTheme::Light
            }
        }
    }
}

fn png_bytes(resolved: ResolvedIconTheme) -> &'static [u8] {
    match resolved {
        ResolvedIconTheme::Light => ICON_LIGHT_PNG,
        ResolvedIconTheme::Dark => ICON_DARK_PNG,
    }
}

/// Apply light/dark/system icon to windows + macOS Dock.
pub fn apply_icon_theme<R: Runtime>(
    app: &AppHandle<R>,
    pref: IconThemePref,
) -> Result<String, String> {
    let resolved = resolve_theme(pref);
    let bytes = png_bytes(resolved);

    apply_window_icons(app, bytes)?;

    #[cfg(target_os = "macos")]
    apply_macos_dock_icon(bytes)?;

    Ok(match resolved {
        ResolvedIconTheme::Light => "light",
        ResolvedIconTheme::Dark => "dark",
    }
    .to_string())
}

fn apply_window_icons<R: Runtime>(app: &AppHandle<R>, bytes: &[u8]) -> Result<(), String> {
    use tauri::image::Image;

    let icon = Image::from_bytes(bytes).map_err(|e| format!("Could not decode icon: {e}"))?;
    for window in app.webview_windows().values() {
        let _ = window.set_icon(icon.clone());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn apply_macos_dock_icon(png_bytes: &[u8]) -> Result<(), String> {
    use objc2::{AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    // NSApplication must be touched on the main thread; Tauri commands run on main by default
    // for most desktop ops, but be defensive.
    let mtm = MainThreadMarker::new().ok_or_else(|| {
        "Dock icon must be updated on the main thread".to_string()
    })?;
    let app = NSApplication::sharedApplication(mtm);
    let data = NSData::with_bytes(png_bytes);
    let image = NSImage::initWithData(NSImage::alloc(), &data)
        .ok_or_else(|| "Could not create NSImage from PNG".to_string())?;
    unsafe {
        app.setApplicationIconImage(Some(&image));
    }
    Ok(())
}
