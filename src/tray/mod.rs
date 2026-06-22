/// System tray integration for osu!mania downloader
/// Uses tray-icon + muda for cross-platform tray support

#[cfg(target_os = "windows")]
pub mod windows_tray {
    use muda::{Menu, MenuItem, PredefinedMenuItem};
    use tray_icon::{TrayIcon, TrayIconBuilder};

    pub fn build_tray() -> anyhow::Result<TrayIcon> {
        let tray_menu = Menu::new();
        let show_item = MenuItem::with_id("show_app", "Show", true, None);
        let sep = PredefinedMenuItem::separator();
        let quit_item = MenuItem::with_id("quit_app", "Quit", true, None);
        tray_menu.append_items(&[&show_item, &sep, &quit_item])?;

        let icon = load_tray_icon();
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(tray_menu))
            .with_tooltip("osu!mania Downloader")
            .with_icon(icon)
            .build()?;
        Ok(tray)
    }

    fn load_tray_icon() -> tray_icon::Icon {
        let size = 32u32;
        let mut rgba = vec![0u8; (size * size * 4) as usize];
        let cx = size as f32 / 2.0;
        let cy = size as f32 / 2.0;
        let r = size as f32 / 2.0 - 1.0;
        for y in 0..size {
            for x in 0..size {
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                let dist = (dx * dx + dy * dy).sqrt();
                let idx = ((y * size + x) * 4) as usize;
                if dist <= r {
                    rgba[idx] = 255;
                    rgba[idx + 1] = 102;
                    rgba[idx + 2] = 170;
                    rgba[idx + 3] = 255;
                } else {
                    rgba[idx + 3] = 0;
                }
            }
        }
        tray_icon::Icon::from_rgba(rgba, size, size).expect("Failed to create tray icon")
    }
}

#[cfg(target_os = "windows")]
pub use self::windows_tray::*;
