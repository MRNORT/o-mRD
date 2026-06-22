#![windows_subsystem = "windows"]

mod api;
mod app;
mod download;
mod tray;

use eframe::egui;
use std::sync::Arc;

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("Failed to build tokio runtime");

    let _guard = rt.enter();

    let rt = Arc::new(rt);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("osu!mania Ranked Downloader")
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([900.0, 600.0])
            .with_icon(load_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "osu!mania Ranked Downloader",
        options,
        Box::new(move |cc| Ok(Box::new(app::OsuManiaApp::new(cc, rt)))),
    )
}

fn load_icon() -> egui::IconData {
    // Simple pink circle icon for osu!
    let size = 64u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let cx = size as f32 / 2.0;
    let cy = size as f32 / 2.0;
    let r = size as f32 / 2.0 - 2.0;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let idx = ((y * size + x) * 4) as usize;
            if dist <= r {
                // Pink osu! color
                rgba[idx] = 255;     // R
                rgba[idx + 1] = 102; // G
                rgba[idx + 2] = 170; // B
                rgba[idx + 3] = 255; // A
            } else {
                rgba[idx + 3] = 0;
            }
        }
    }

    egui::IconData {
        rgba,
        width: size,
        height: size,
    }
}
