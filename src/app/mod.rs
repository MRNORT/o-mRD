pub mod config;

use crate::api::{Beatmapset, OsuClient, SearchFilters, SearchResponse, SortBy};
use crate::download::{DownloadItem, DownloadProgress, DownloadStatus, Downloader};
use config::AppConfig;
use eframe::egui::{
    self, Color32, CornerRadius, FontId, Margin, Pos2, Rect, RichText, Stroke, Ui, Vec2,
};
use egui_notify::Toasts;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use rodio::{OutputStream, OutputStreamHandle, Sink, Decoder};
use std::io::Cursor;

// ─── Colour palette ─────────────────────────────────────────────────────────
const COL_BG: Color32 = Color32::from_rgb(13, 12, 20);
const COL_SURFACE: Color32 = Color32::from_rgb(22, 20, 35);
const COL_SURFACE2: Color32 = Color32::from_rgb(30, 28, 46);
const COL_BORDER: Color32 = Color32::from_rgb(50, 45, 75);
const COL_ACCENT: Color32 = Color32::from_rgb(255, 102, 170); // osu! pink
const COL_ACCENT2: Color32 = Color32::from_rgb(130, 80, 255); // purple
const COL_TEXT: Color32 = Color32::from_rgb(230, 225, 245);
const COL_TEXT_DIM: Color32 = Color32::from_rgb(140, 130, 170);
const COL_SUCCESS: Color32 = Color32::from_rgb(80, 200, 120);
const COL_WARNING: Color32 = Color32::from_rgb(255, 180, 60);
const COL_ERROR: Color32 = Color32::from_rgb(255, 80, 80);
const COL_CARD_BG: Color32 = Color32::from_rgb(26, 24, 40);

fn cr(r: u8) -> CornerRadius {
    CornerRadius::same(r)
}

fn margin_xy(x: f32, y: f32) -> Margin {
    Margin {
        left: x as i8,
        right: x as i8,
        top: y as i8,
        bottom: y as i8,
    }
}

fn margin_same(v: f32) -> Margin {
    Margin::same(v as i8)
}

// ─── App state ───────────────────────────────────────────────────────────────
#[derive(Debug, Clone, PartialEq)]
enum Tab {
    Browse,
    Downloads,
    Settings,
}

pub struct OsuManiaApp {
    rt: Arc<tokio::runtime::Runtime>,

    // Config
    config: AppConfig,

    // API
    api_client: Option<OsuClient>,
    auth_status: AuthStatus,

    // Browse tab
    current_tab: Tab,
    filters: SearchFilters,
    key_filter_4k: bool,
    key_filter_5k: bool,
    key_filter_6k: bool,
    key_filter_7k: bool,
    key_filter_8k: bool,
    search_results: Vec<Beatmapset>,
    is_loading: bool,
    load_error: Option<String>,
    cursor_string: Option<String>,
    has_more: bool,
    total_results: Option<u64>,
    pending_results: Arc<Mutex<Option<Result<SearchResponse, String>>>>,
    search_query_input: String,
    sr_min_input: String,
    sr_max_input: String,
    bpm_min_input: String,
    bpm_max_input: String,

    // Downloads
    downloads: HashMap<u64, DownloadItem>,
    progress_rx: Arc<Mutex<mpsc::Receiver<DownloadProgress>>>,
    progress_tx: mpsc::Sender<DownloadProgress>,
    downloader: Arc<Downloader>,
    download_dir_input: String,
    osu_dir_input: String,
    auto_dl_active: bool,
    /// Receives (beatmapset_id, title, artist) tuples from the auto-download task
    meta_rx: Option<tokio::sync::mpsc::Receiver<(u64, String, String)>>,

    // Settings
    client_id_input: String,
    client_secret_input: String,
    show_secret: bool,

    // Audio
    _audio_stream: Option<OutputStream>,
    _audio_handle: Option<OutputStreamHandle>,
    audio_sink: Option<Sink>,
    audio_tx: mpsc::Sender<(u64, Vec<u8>)>,
    audio_rx: Arc<Mutex<mpsc::Receiver<(u64, Vec<u8>)>>>,
    playing_preview_id: Option<u64>,

    // UI helpers
    toasts: Toasts,
    scroll_to_top: bool,

    #[cfg(target_os = "windows")]
    _tray: Option<tray_icon::TrayIcon>,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
enum AuthStatus {
    NotConfigured,
    Authenticating,
    Ready,
    Error(String),
}

impl OsuManiaApp {
    pub fn new(cc: &eframe::CreationContext<'_>, rt: Arc<tokio::runtime::Runtime>) -> Self {
        setup_fonts(&cc.egui_ctx);
        setup_visuals(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);

        let config = AppConfig::load();
        let (progress_tx, progress_rx) = mpsc::channel(256);
        let (audio_tx, audio_rx) = mpsc::channel(16);

        let (stream_opt, handle_opt, sink_opt) = match OutputStream::try_default() {
            Ok((stream, handle)) => {
                let sink = Sink::try_new(&handle).ok();
                (Some(stream), Some(handle), sink)
            }
            Err(_) => (None, None, None),
        };

        if let Some(sink) = &sink_opt {
            sink.set_volume(0.2); // Not too loud
        }

        let client_id_input = config.client_id.clone();
        let client_secret_input = config.client_secret.clone();
        let download_dir_input = config.download_dir.display().to_string();
        let osu_dir_input = config
            .osu_dir
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();


        #[cfg(target_os = "windows")]
        {
            let ctx_clone = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    ctx_clone.request_repaint();
                }
            });
        }

        let mut app = Self {
            rt,
            api_client: None,
            auth_status: AuthStatus::NotConfigured,
            current_tab: Tab::Browse,
            filters: SearchFilters {
                sort: SortBy::RankedDesc,
                ..Default::default()
            },
            key_filter_4k: false,
            key_filter_5k: false,
            key_filter_6k: false,
            key_filter_7k: false,
            key_filter_8k: false,
            search_results: Vec::new(),
            is_loading: false,
            load_error: None,
            cursor_string: None,
            has_more: false,
            total_results: None,
            pending_results: Arc::new(Mutex::new(None)),
            search_query_input: String::new(),
            sr_min_input: String::new(),
            sr_max_input: String::new(),
            bpm_min_input: String::new(),
            bpm_max_input: String::new(),
            downloads: HashMap::new(),
            progress_rx: Arc::new(Mutex::new(progress_rx)),
            progress_tx,
            downloader: Arc::new(Downloader::new()),
            download_dir_input,
            osu_dir_input,
            auto_dl_active: false,
            meta_rx: None,
            client_id_input,
            client_secret_input,
            show_secret: false,
            _audio_stream: stream_opt,
            _audio_handle: handle_opt,
            audio_sink: sink_opt,
            audio_tx,
            audio_rx: Arc::new(Mutex::new(audio_rx)),
            playing_preview_id: None,
            toasts: Toasts::default(),
            scroll_to_top: false,
            config,
            #[cfg(target_os = "windows")]
            _tray: crate::tray::build_tray().ok(),
        };

        app.api_client = Some(OsuClient::new(
            app.config.client_id.clone(),
            app.config.client_secret.clone(),
        ));

        if !app.config.client_id.is_empty() && !app.config.client_secret.is_empty() {
            app.start_auth();
        } else {
            app.auth_status = AuthStatus::Ready;
        }

        app
    }

    fn start_auth(&mut self) {
        let client = OsuClient::new(
            self.config.client_id.clone(),
            self.config.client_secret.clone(),
        );
        self.auth_status = AuthStatus::Authenticating;
        self.api_client = Some(client.clone());

        let pending = self.pending_results.clone();
        self.rt.spawn(async move {
            if let Err(e) = client.authenticate().await {
                log::error!("Auth failed: {}", e);
                let mut p = pending.lock().await;
                *p = Some(Err(format!("Auth failed: {}", e)));
            }
        });

        self.auth_status = AuthStatus::Ready;
    }

    fn start_search(&mut self, append: bool) {
        if self.is_loading {
            return;
        }
        if let Some(client) = &self.api_client {
            let client = client.clone();
            let mut filters = self.filters.clone();

            let mut keys = Vec::new();
            if self.key_filter_4k { keys.push(4); }
            if self.key_filter_5k { keys.push(5); }
            if self.key_filter_6k { keys.push(6); }
            if self.key_filter_7k { keys.push(7); }
            if self.key_filter_8k { keys.push(8); }
            filters.key_counts = keys;

            filters.min_sr = self.sr_min_input.parse().ok();
            filters.max_sr = self.sr_max_input.parse().ok();
            filters.min_bpm = self.bpm_min_input.parse().ok();
            filters.max_bpm = self.bpm_max_input.parse().ok();
            filters.query = self.search_query_input.clone();

            let cursor = if append { self.cursor_string.clone() } else { None };
            let pending = self.pending_results.clone();

            self.is_loading = true;
            if !append {
                self.search_results.clear();
                self.cursor_string = None;
                self.scroll_to_top = true;
            }

            self.rt.spawn(async move {
                let result = client
                    .search_ranked_mania(&filters, cursor.as_deref())
                    .await
                    .map_err(|e| e.to_string());
                let mut p = pending.lock().await;
                *p = Some(result);
            });
        }
    }

    fn start_download(&mut self, beatmapset: &Beatmapset) {
        let id = beatmapset.id;
        if self.downloads.contains_key(&id) {
            return;
        }

        let item = DownloadItem {
            beatmapset_id: id,
            title: beatmapset.title.clone(),
            artist: beatmapset.artist.clone(),
            status: DownloadStatus::Queued,
            file_path: None,
        };
        self.downloads.insert(id, item);

        let downloader = self.downloader.clone();
        // If auto_import is on and the osu! Songs folder exists, download directly there
        let dir = if self.config.auto_import {
            self.config
                .osu_dir
                .as_ref()
                .map(|d| d.join("Songs"))
                .filter(|p| p.exists())
                .unwrap_or_else(|| self.config.download_dir.clone())
        } else {
            self.config.download_dir.clone()
        };
        let tx = self.progress_tx.clone();
        let prefer_no_video = self.config.prefer_no_video;

        self.rt.spawn(async move {
            if let Err(e) = downloader.download(id, &dir, prefer_no_video, tx.clone()).await {
                let _ = tx
                    .send(DownloadProgress {
                        beatmapset_id: id,
                        status: DownloadStatus::Failed(e.to_string()),
                        file_path: None,
                    })
                    .await;
            }
        });
    }

    fn scan_installed_maps(&mut self) {
        let mut installed = std::collections::HashSet::new();

        if let Some(osu_dir) = &self.config.osu_dir {
            let songs_dir = osu_dir.join("Songs");
            if let Ok(entries) = std::fs::read_dir(songs_dir) {
                for entry in entries.flatten() {
                    if let Ok(name) = entry.file_name().into_string() {
                        if let Some(id_str) = name.split_whitespace().next() {
                            if let Ok(id) = id_str.parse::<u64>() {
                                installed.insert(id);
                            }
                        }
                    }
                }
            }
        }

        if let Ok(entries) = std::fs::read_dir(&self.config.download_dir) {
            for entry in entries.flatten() {
                if let Ok(name) = entry.file_name().into_string() {
                    if name.ends_with(".osz") {
                        let id_str = name.trim_end_matches(".osz");
                        if let Ok(id) = id_str.parse::<u64>() {
                            installed.insert(id);
                        }
                    }
                }
            }
        }

        self.config.known_beatmapset_ids = installed.into_iter().collect();
        self.config.save();
        self.toasts.success(format!(
            "Scanned {} installed maps.",
            self.config.known_beatmapset_ids.len()
        ));
    }

    fn start_auto_download(&mut self) {
        if self.auto_dl_active {
            self.toasts.warning("Auto-download already running.");
            return;
        }

        // Re-scan to get latest installed map list
        self.scan_installed_maps();

        let known: std::collections::HashSet<u64> =
            self.config.known_beatmapset_ids.iter().copied().collect();

        if let Some(client) = &self.api_client {
            let client = client.clone();
            let mut filters = SearchFilters {
                sort: SortBy::RankedDesc,
                ..Default::default()
            };
            if self.config.download_4k_only {
                filters.key_counts = vec![4];
            }

            let downloader = self.downloader.clone();
            // Download directly to Songs folder if auto_import is on
            let dir = if self.config.auto_import {
                self.config
                    .osu_dir
                    .as_ref()
                    .map(|d| d.join("Songs"))
                    .filter(|p| p.exists())
                    .unwrap_or_else(|| self.config.download_dir.clone())
            } else {
                self.config.download_dir.clone()
            };
            let tx = self.progress_tx.clone();
            let prefer_no_video = self.config.prefer_no_video;

            // Channel to send discovered beatmapset metadata back to UI
            let (meta_tx, meta_rx) =
                tokio::sync::mpsc::channel::<(u64, String, String)>(256);

            self.auto_dl_active = true;
            self.meta_rx = Some(meta_rx);

            self.rt.spawn(async move {
                let mut cursor: Option<String> = None;

                for _ in 0..10 {
                    match client.search_ranked_mania(&filters, cursor.as_deref()).await {
                        Ok(resp) => {
                            for map in resp.beatmapsets {
                                if !known.contains(&map.id) {
                                    // Notify UI of this map's title/artist before downloading
                                    let _ = meta_tx.send((map.id, map.title.clone(), map.artist.clone())).await;
                                    let _ = downloader.download(map.id, &dir, prefer_no_video, tx.clone()).await;
                                    // Small polite delay between individual downloads
                                    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
                                }
                            }

                            cursor = resp.cursor_string;
                            if cursor.is_none() {
                                break;
                            }
                            // Polite delay between pages
                            tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                        }
                        Err(e) => {
                            log::error!("Auto-download search failed: {}", e);
                            break;
                        }
                    }
                }
                // meta_tx drops here — poll_results detects Disconnected and clears auto_dl_active
            });

            self.toasts.info("Auto-downloading missing maps in background...");
        } else {
            self.toasts
                .warning("No API client – configure credentials in Settings.");
        }
    }

    fn poll_results(&mut self) {
        // Poll metadata from auto-download task (title/artist for new items)
        let mut meta_done = false;
        if let Some(ref mut rx) = self.meta_rx {
            loop {
                match rx.try_recv() {
                    Ok((id, title, artist)) => {
                        self.downloads
                            .entry(id)
                            .and_modify(|item| {
                                if item.title.starts_with("Map #") {
                                    item.title = title.clone();
                                    item.artist = artist.clone();
                                }
                            })
                            .or_insert_with(|| DownloadItem {
                                beatmapset_id: id,
                                title,
                                artist,
                                status: DownloadStatus::Queued,
                                file_path: None,
                            });
                    }
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                        meta_done = true;
                        break;
                    }
                }
            }
        }
        if meta_done {
            self.meta_rx = None;
            self.auto_dl_active = false;
        }
        // Poll download progress
        if let Ok(mut rx) = self.progress_rx.try_lock() {
            while let Ok(prog) = rx.try_recv() {
                // Register new downloads that came from auto-download (not in map yet)
                if !self.downloads.contains_key(&prog.beatmapset_id) {
                    // We don't have title/artist info here; use ID as placeholder
                    self.downloads.insert(
                        prog.beatmapset_id,
                        DownloadItem {
                            beatmapset_id: prog.beatmapset_id,
                            title: format!("Map #{}", prog.beatmapset_id),
                            artist: String::new(),
                            status: prog.status.clone(),
                            file_path: prog.file_path.clone(),
                        },
                    );
                    continue;
                }

                if let Some(item) = self.downloads.get_mut(&prog.beatmapset_id) {
                    match &prog.status {
                        DownloadStatus::Completed => {
                            item.file_path = prog.file_path.clone();
                            item.status = DownloadStatus::Completed;
                            self.toasts.success(format!("{}", item.title));

                            // Auto-import: opening the .osz with the system handler
                            // imports it into osu! if osu! is running.
                            // If it was downloaded directly to Songs, osu! picks it up automatically.
                            if self.config.auto_import && !self.is_in_songs_dir(&prog.file_path) {
                                if let Some(path) = &prog.file_path {
                                    let _ = open::that(path);
                                }
                            }
                        }
                        DownloadStatus::AlreadyExists => {
                            item.file_path = prog.file_path.clone();
                            item.status = DownloadStatus::AlreadyExists;
                        }
                        DownloadStatus::Failed(e) => {
                            item.status = DownloadStatus::Failed(e.clone());
                            self.toasts
                                .error(format!("Failed: {}", item.title));
                        }
                        s => item.status = s.clone(),
                    }
                }
            }
        }

        // Poll search results
        if self.is_loading {
            if let Ok(mut pending) = self.pending_results.try_lock() {
                if let Some(result) = pending.take() {
                    self.is_loading = false;
                    match result {
                        Ok(resp) => {
                            self.has_more = resp.cursor_string.is_some();
                            self.cursor_string = resp.cursor_string;
                            self.total_results = resp.total;
                            self.load_error = None;
                            if resp.beatmapsets.is_empty() && self.search_results.is_empty() {
                                self.load_error =
                                    Some("No maps found with current filters.".into());
                            }
                            for bs in resp.beatmapsets {
                                if !self.search_results.iter().any(|r| r.id == bs.id) {
                                    self.search_results.push(bs);
                                }
                            }
                        }
                        Err(e) => {
                            self.load_error = Some(e);
                        }
                    }
                }
            }
        }
    }

    fn poll_audio(&mut self) {
        if let Ok(mut rx) = self.audio_rx.try_lock() {
            while let Ok((id, bytes)) = rx.try_recv() {
                if let Some(sink) = &self.audio_sink {
                    sink.stop();
                    let cursor = Cursor::new(bytes);
                    if let Ok(decoder) = Decoder::new(cursor) {
                        sink.append(decoder);
                        sink.play();
                        self.playing_preview_id = Some(id);
                    }
                }
            }
        }
        
        // Auto-clear playing ID if sink is empty
        if let Some(sink) = &self.audio_sink {
            if sink.empty() && self.playing_preview_id.is_some() {
                self.playing_preview_id = None;
            }
        }
    }

    /// Returns true if the file is already inside the osu! Songs directory
    fn is_in_songs_dir(&self, path: &Option<PathBuf>) -> bool {
        if let (Some(path), Some(osu_dir)) = (path, &self.config.osu_dir) {
            let songs = osu_dir.join("Songs");
            path.starts_with(&songs)
        } else {
            false
        }
    }
}

impl eframe::App for OsuManiaApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(target_os = "windows")]
        {
            if let Ok(event) = muda::MenuEvent::receiver().try_recv() {
                if event.id.0 == "show_app" {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                } else if event.id.0 == "quit_app" {
                    std::process::exit(0);
                }
            }

            if let Ok(event) = tray_icon::TrayIconEvent::receiver().try_recv() {
                if let tray_icon::TrayIconEvent::Click {
                    button: tray_icon::MouseButton::Left,
                    button_state: tray_icon::MouseButtonState::Up,
                    ..
                } = event {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
            }

        }

        self.poll_results();
        self.poll_audio();

        if self.is_loading || self.auto_dl_active || self.playing_preview_id.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        // ── Top navbar ────────────────────────────────────────────────────────
        egui::TopBottomPanel::top("navbar")
            .frame(
                egui::Frame::new()
                    .fill(COL_SURFACE)
                    .inner_margin(margin_xy(16.0, 0.0)),
            )
            .show(ctx, |ui| {
                ui.set_min_height(56.0);
                ui.horizontal_centered(|ui| {
                    ui.add_space(4.0);
                    let (logo_rect, _) =
                        ui.allocate_exact_size(Vec2::new(36.0, 36.0), egui::Sense::hover());
                    draw_osu_logo(ui.painter(), logo_rect.center(), 16.0);

                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("osu!mania")
                            .font(FontId::proportional(22.0))
                            .color(COL_ACCENT)
                            .strong(),
                    );
                    ui.label(
                        RichText::new("Ranked Downloader")
                            .font(FontId::proportional(16.0))
                            .color(COL_TEXT_DIM),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        match &self.auth_status.clone() {
                            AuthStatus::Ready => {
                                ui.label(
                                    RichText::new("Connected")
                                        .color(COL_SUCCESS)
                                        .font(FontId::proportional(13.0)),
                                );
                            }
                            AuthStatus::Authenticating => {
                                ui.label(
                                    RichText::new("Connecting...")
                                        .color(COL_WARNING)
                                        .font(FontId::proportional(13.0)),
                                );
                            }
                            AuthStatus::NotConfigured => {
                                ui.label(
                                    RichText::new("Not configured")
                                        .color(COL_WARNING)
                                        .font(FontId::proportional(13.0)),
                                );
                            }
                            AuthStatus::Error(e) => {
                                let label = egui::Label::new(
                                    RichText::new(format!("{}", e))
                                        .color(COL_ERROR)
                                        .font(FontId::proportional(13.0)),
                                )
                                .truncate();
                                ui.add(label);
                            }
                        }

                        ui.add_space(16.0);
                        let active = self
                            .downloads
                            .values()
                            .filter(|d| matches!(d.status, DownloadStatus::Downloading { .. }))
                            .count();
                        if active > 0 {
                            ui.label(
                                RichText::new(format!("{} active", active))
                                    .color(COL_ACCENT)
                                    .font(FontId::proportional(13.0)),
                            );
                            ui.add_space(8.0);
                        }
                    });
                });
            });

        // ── Tab bar ──────────────────────────────────────────────────────────
        egui::TopBottomPanel::top("tabs")
            .frame(
                egui::Frame::new()
                    .fill(COL_BG)
                    .inner_margin(margin_xy(20.0, 0.0)),
            )
            .show(ctx, |ui| {
                ui.set_min_height(44.0);
                ui.horizontal_centered(|ui| {
                    let mut switch = None;
                    tab_button(ui, "Browse", self.current_tab == Tab::Browse, || {
                        switch = Some(Tab::Browse);
                    });
                    tab_button(ui, "Downloads", self.current_tab == Tab::Downloads, || {
                        switch = Some(Tab::Downloads);
                    });
                    tab_button(ui, "Settings", self.current_tab == Tab::Settings, || {
                        switch = Some(Tab::Settings);
                    });
                    if let Some(t) = switch {
                        self.current_tab = t;
                    }
                });
            });

        // ── Main content ─────────────────────────────────────────────────────
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(COL_BG))
            .show(ctx, |ui| match self.current_tab.clone() {
                Tab::Browse => self.show_browse_tab(ui),
                Tab::Downloads => self.show_downloads_tab(ui),
                Tab::Settings => self.show_settings_tab(ui),
            });

        self.toasts.show(ctx);
    }
}

impl OsuManiaApp {
    fn show_browse_tab(&mut self, ui: &mut Ui) {
        // ── Left sidebar (filters) ────────────────────────────────────────────
        egui::SidePanel::left("filters_panel")
            .resizable(false)
            .exact_width(240.0)
            .frame(
                egui::Frame::new()
                    .fill(COL_SURFACE)
                    .inner_margin(margin_same(16.0)),
            )
            .show_inside(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 10.0;

                ui.label(
                    RichText::new("SEARCH")
                        .color(COL_ACCENT)
                        .font(FontId::proportional(11.0))
                        .strong(),
                );
                ui.add_space(2.0);
                let search_resp = ui.add(
                    egui::TextEdit::singleline(&mut self.search_query_input)
                        .hint_text("Title, artist, mapper...")
                        .desired_width(ui.available_width()),
                );
                if search_resp.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                {
                    self.start_search(false);
                }

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                ui.label(
                    RichText::new("KEY COUNT")
                        .color(COL_ACCENT)
                        .font(FontId::proportional(11.0))
                        .strong(),
                );
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    key_toggle(ui, "4K", &mut self.key_filter_4k);
                    key_toggle(ui, "5K", &mut self.key_filter_5k);
                    key_toggle(ui, "6K", &mut self.key_filter_6k);
                    key_toggle(ui, "7K", &mut self.key_filter_7k);
                    key_toggle(ui, "8K", &mut self.key_filter_8k);
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                ui.label(
                    RichText::new("STAR RATING")
                        .color(COL_ACCENT)
                        .font(FontId::proportional(11.0))
                        .strong(),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Min")
                            .color(COL_TEXT_DIM)
                            .font(FontId::proportional(12.0)),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.sr_min_input)
                            .desired_width(52.0)
                            .hint_text("0.0"),
                    );
                    ui.label(
                        RichText::new("Max")
                            .color(COL_TEXT_DIM)
                            .font(FontId::proportional(12.0)),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.sr_max_input)
                            .desired_width(52.0)
                            .hint_text("∞"),
                    );
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                ui.label(
                    RichText::new("BPM")
                        .color(COL_ACCENT)
                        .font(FontId::proportional(11.0))
                        .strong(),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Min")
                            .color(COL_TEXT_DIM)
                            .font(FontId::proportional(12.0)),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.bpm_min_input)
                            .desired_width(52.0)
                            .hint_text("0"),
                    );
                    ui.label(
                        RichText::new("Max")
                            .color(COL_TEXT_DIM)
                            .font(FontId::proportional(12.0)),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.bpm_max_input)
                            .desired_width(52.0)
                            .hint_text("∞"),
                    );
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                ui.label(
                    RichText::new("SORT BY")
                        .color(COL_ACCENT)
                        .font(FontId::proportional(11.0))
                        .strong(),
                );
                ui.add_space(4.0);
                let sorts = [
                    SortBy::RankedDesc,
                    SortBy::RankedAsc,
                    SortBy::Plays,
                    SortBy::Favourites,
                    SortBy::Difficulty,
                    SortBy::Title,
                ];
                egui::ComboBox::from_id_salt("sort_by")
                    .selected_text(self.filters.sort.display())
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        for sort in &sorts {
                            ui.selectable_value(
                                &mut self.filters.sort,
                                sort.clone(),
                                sort.display(),
                            );
                        }
                    });

                ui.add_space(16.0);

                ui.vertical_centered(|ui| {
                    let btn = egui::Button::new(
                        RichText::new(if self.is_loading {
                            "  Searching...  "
                        } else {
                            "  Search  "
                        })
                        .color(Color32::WHITE)
                        .font(FontId::proportional(14.0)),
                    )
                    .fill(if self.is_loading { COL_SURFACE2 } else { COL_ACCENT })
                    .corner_radius(cr(8))
                    .min_size(Vec2::new(180.0, 36.0));

                    if ui.add_enabled(!self.is_loading, btn).clicked() {
                        if matches!(self.auth_status, AuthStatus::Ready) {
                            self.start_search(false);
                        } else {
                            self.toasts
                                .warning("Configure API credentials in Settings first.");
                        }
                    }
                });

                if let Some(total) = self.total_results {
                    ui.add_space(8.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new(format!("{} maps found", total))
                                .color(COL_TEXT_DIM)
                                .font(FontId::proportional(12.0)),
                        );
                    });
                }

                if !self.search_results.is_empty() {
                    ui.add_space(12.0);
                    ui.vertical_centered(|ui| {
                        let dl_all_btn = egui::Button::new(
                            RichText::new("Download All Listed")
                                .color(Color32::WHITE)
                                .font(FontId::proportional(14.0)),
                        )
                        .fill(COL_SUCCESS)
                        .corner_radius(cr(8))
                        .min_size(Vec2::new(180.0, 36.0));

                        if ui.add(dl_all_btn).clicked() {
                            let results = self.search_results.clone();
                            let count = results.len();
                            for result in results.iter() {
                                self.start_download(result);
                            }
                            self.toasts.success(format!("Queued {} downloads!", count));
                        }
                    });
                }
            });

        // ── Results area ──────────────────────────────────────────────────────
        egui::Frame::new()
            .fill(COL_BG)
            .inner_margin(margin_xy(16.0, 12.0))
            .show(ui, |ui| {
                if let Some(err) = self.load_error.clone() {
                    ui.add_space(80.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new(format!("{}", err))
                                .color(COL_ERROR)
                                .font(FontId::proportional(15.0)),
                        );
                    });
                    return;
                }

                if self.search_results.is_empty() && !self.is_loading {
                    ui.add_space(100.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("🎹")
                                .font(FontId::proportional(64.0))
                                .color(COL_ACCENT),
                        );
                        ui.add_space(16.0);
                        ui.label(
                            RichText::new("Search for ranked osu!mania maps")
                                .font(FontId::proportional(20.0))
                                .color(COL_TEXT),
                        );
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("Use the filters on the left and press Search")
                                .font(FontId::proportional(14.0))
                                .color(COL_TEXT_DIM),
                        );
                    });
                    return;
                }

                let scroll_to_top = self.scroll_to_top;
                self.scroll_to_top = false;

                let results_clone: Vec<Beatmapset> = self.search_results.clone();
                let downloads_clone: HashMap<u64, DownloadStatus> = self
                    .downloads
                    .iter()
                    .map(|(k, v)| (*k, v.status.clone()))
                    .collect();

                let mut download_idx: Option<usize> = None;
                let mut open_url_id: Option<u64> = None;
                let mut load_more = false;
                let is_loading = self.is_loading;
                let has_more = self.has_more;

                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        if scroll_to_top {
                            ui.scroll_to_cursor(Some(egui::Align::TOP));
                        }

                        for (idx, bs) in results_clone.iter().enumerate() {
                            let dl_status = downloads_clone.get(&bs.id).cloned();
                            let is_playing = self.playing_preview_id == Some(bs.id);
                            
                            if let Some(action) = show_beatmapset_card(ui, bs, dl_status, is_playing) {
                                match action {
                                    CardAction::Download => download_idx = Some(idx),
                                    CardAction::OpenInBrowser => open_url_id = Some(bs.id),
                                    CardAction::PlayPreview(id, url) => {
                                        if self.playing_preview_id == Some(id) {
                                            // Clicked the playing track again, stop it
                                            if let Some(sink) = &self.audio_sink {
                                                sink.stop();
                                            }
                                            self.playing_preview_id = None;
                                        } else {
                                            // Stop current playback to be responsive
                                            if let Some(sink) = &self.audio_sink {
                                                sink.stop();
                                            }
                                            self.playing_preview_id = Some(id);
                                            
                                            // Spawn a task to fetch and play the audio
                                            let audio_tx = self.audio_tx.clone();
                                            // Ensure url starts with https:
                                            let fetch_url = if url.starts_with("//") {
                                                format!("https:{}", url)
                                            } else {
                                                url.clone()
                                            };
                                            self.rt.spawn(async move {
                                                if let Ok(resp) = reqwest::get(&fetch_url).await {
                                                    if let Ok(bytes) = resp.bytes().await {
                                                        let _ = audio_tx.send((id, bytes.to_vec())).await;
                                                    }
                                                }
                                            });
                                        }
                                    }
                                }
                            }
                            ui.add_space(8.0);
                        }

                        if has_more && !results_clone.is_empty() {
                            ui.vertical_centered(|ui| {
                                ui.add_space(8.0);
                                let btn = egui::Button::new(
                                    RichText::new(if is_loading {
                                        "Loading..."
                                    } else {
                                        "Load More"
                                    })
                                    .font(FontId::proportional(14.0))
                                    .color(COL_TEXT),
                                )
                                .fill(COL_SURFACE2)
                                .corner_radius(cr(8))
                                .min_size(Vec2::new(160.0, 36.0));

                                if ui.add_enabled(!is_loading, btn).clicked() {
                                    load_more = true;
                                }
                                ui.add_space(16.0);
                            });
                        }
                    });

                if load_more {
                    self.start_search(true);
                }
                if let Some(idx) = download_idx {
                    let bs = self.search_results[idx].clone();
                    self.start_download(&bs);
                }
                if let Some(id) = open_url_id {
                    let _ = open::that(format!("https://osu.ppy.sh/beatmapsets/{}", id));
                }
            });
    }

    fn show_downloads_tab(&mut self, ui: &mut Ui) {
        egui::Frame::new()
            .fill(COL_BG)
            .inner_margin(margin_xy(20.0, 16.0))
            .show(ui, |ui| {
                ui.label(
                    RichText::new("Downloads")
                        .color(COL_TEXT)
                        .font(FontId::proportional(22.0))
                        .strong(),
                );
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    if ui.button("Open Folder").clicked() {
                        let _ = open::that(&self.config.download_dir);
                    }
                    ui.add_space(8.0);
                    let completed = self
                        .downloads
                        .values()
                        .filter(|d| matches!(d.status, DownloadStatus::Completed))
                        .count();
                    let active = self
                        .downloads
                        .values()
                        .filter(|d| matches!(d.status, DownloadStatus::Downloading { .. }))
                        .count();
                    let failed = self
                        .downloads
                        .values()
                        .filter(|d| matches!(d.status, DownloadStatus::Failed(_)))
                        .count();

                    ui.label(
                        RichText::new(format!(
                            "Completed: {}  Active: {}  Failed: {}",
                            completed, active, failed
                        ))
                        .color(COL_TEXT_DIM)
                        .font(FontId::proportional(13.0)),
                    );

                    if self.auto_dl_active {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("Auto-DL running...")
                                .color(COL_WARNING)
                                .font(FontId::proportional(13.0)),
                        );
                    }

                    if self.config.download_4k_only {
                        ui.add_space(4.0);
                        let (badge_rect, _) = ui.allocate_exact_size(
                            Vec2::new(46.0, 20.0),
                            egui::Sense::hover(),
                        );
                        ui.painter().rect_filled(badge_rect, cr(4), COL_ACCENT2);
                        ui.painter().text(
                            badge_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "4K Only",
                            FontId::proportional(10.0),
                            Color32::WHITE,
                        );
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !self.auto_dl_active {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Auto-Download Missing")
                                            .color(Color32::WHITE),
                                    )
                                    .fill(COL_ACCENT)
                                    .corner_radius(cr(6)),
                                )
                                .clicked()
                            {
                                self.start_auto_download();
                            }
                        }

                        if ui.button("Clear Completed").clicked() {
                            self.downloads.retain(|_, v| {
                                !matches!(
                                    v.status,
                                    DownloadStatus::Completed | DownloadStatus::AlreadyExists
                                )
                            });
                        }
                    });
                });

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);

                if self.downloads.is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(80.0);
                            ui.label(
                                RichText::new("⬇")
                                    .font(FontId::proportional(64.0))
                                    .color(COL_ACCENT),
                            );
                            ui.add_space(16.0);
                            ui.label(
                                RichText::new("No downloads yet")
                                    .font(FontId::proportional(18.0))
                                    .color(COL_TEXT),
                            );
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new("Browse maps and click Download to get started")
                                    .font(FontId::proportional(13.0))
                                    .color(COL_TEXT_DIM),
                            );
                        });
                    });
                    return;
                }

                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        let items: Vec<_> = self.downloads.values().cloned().collect();
                        for item in items {
                            show_download_item(ui, &item);
                            ui.add_space(4.0);
                        }
                    });
            });
    }

    fn show_settings_tab(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                egui::Frame::new()
                    .fill(COL_BG)
                    .inner_margin(margin_xy(40.0, 24.0))
                    .show(ui, |ui| {
                        ui.set_max_width(600.0);

                        settings_section(ui, "🔑 osu! API Credentials (Optional)");
                        ui.label(
                            RichText::new(
                                "By default, search uses the free Nerinyan API. For the official osu! search, add your API v2 client below.",
                            )
                            .color(COL_TEXT_DIM)
                            .font(FontId::proportional(13.0)),
                        );
                        ui.add_space(4.0);
                        if ui
                            .link("Create one at osu.ppy.sh/home/account/edit#oauth")
                            .clicked()
                        {
                            let _ =
                                open::that("https://osu.ppy.sh/home/account/edit#oauth");
                        }
                        ui.add_space(12.0);

                        settings_row(ui, "Client ID", |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.client_id_input)
                                    .desired_width(ui.available_width().min(400.0))
                                    .hint_text("Your client ID"),
                            );
                        });

                        settings_row(ui, "Client Secret", |ui| {
                            ui.add(
                                egui::TextEdit::singleline(
                                    &mut self.client_secret_input,
                                )
                                .desired_width(240.0)
                                .password(!self.show_secret)
                                .hint_text("Your client secret"),
                            );
                            if ui
                                .small_button(if self.show_secret { "🙈" } else { "👁" })
                                .clicked()
                            {
                                self.show_secret = !self.show_secret;
                            }
                        });

                        ui.add_space(12.0);

                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Save & Connect")
                                            .color(Color32::WHITE),
                                    )
                                    .fill(COL_ACCENT)
                                    .corner_radius(cr(6))
                                    .min_size(Vec2::new(140.0, 32.0)),
                                )
                                .clicked()
                            {
                                self.config.client_id = self.client_id_input.clone();
                                self.config.client_secret = self.client_secret_input.clone();
                                self.config.save();
                                
                                // Re-initialize API client
                                self.api_client = Some(crate::api::OsuClient::new(
                                    self.config.client_id.clone(),
                                    self.config.client_secret.clone(),
                                ));

                                if !self.config.client_id.is_empty() && !self.config.client_secret.is_empty() {
                                    self.start_auth();
                                    self.toasts.success("Credentials saved! Connecting...");
                                } else {
                                    self.toasts.success("Settings saved! Using Nerinyan API.");
                                    self.auth_status = AuthStatus::Ready;
                                }
                            }

                            match &self.auth_status.clone() {
                                AuthStatus::Ready => {
                                    ui.label(
                                        RichText::new("Connected").color(COL_SUCCESS),
                                    );
                                }
                                AuthStatus::Error(e) => {
                                    ui.label(
                                        RichText::new(format!("{}", e))
                                            .color(COL_ERROR),
                                    );
                                }
                                _ => {}
                            }
                        });

                        ui.add_space(24.0);
                        ui.separator();
                        ui.add_space(24.0);

                        settings_section(ui, "📁 Download Directory");
                        settings_row(ui, "Save .osz to", |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.download_dir_input)
                                    .desired_width(300.0)
                                    .hint_text("Download folder path"),
                            );
                            if ui.button("Browse...").clicked() {
                                if let Some(path) =
                                    rfd::FileDialog::new().pick_folder()
                                {
                                    self.download_dir_input =
                                        path.display().to_string();
                                }
                            }
                        });

                        if ui.button("Apply Directory").clicked() {
                            self.config.download_dir =
                                PathBuf::from(&self.download_dir_input);
                            self.config.save();
                            self.toasts.success("Download directory updated.");
                        }

                        ui.add_space(24.0);
                        ui.separator();
                        ui.add_space(24.0);

                        settings_section(ui, "🎮 osu! Integration");

                        // Show status of detected osu! folder
                        let songs_exists = self
                            .config
                            .osu_dir
                            .as_ref()
                            .map(|d| d.join("Songs").exists())
                            .unwrap_or(false);

                        if songs_exists {
                            ui.label(
                                RichText::new("osu! Songs folder detected")
                                    .color(COL_SUCCESS)
                                    .font(FontId::proportional(12.0)),
                            );
                        } else {
                            ui.label(
                                RichText::new("osu! Songs folder not found — set path below")
                                    .color(COL_WARNING)
                                    .font(FontId::proportional(12.0)),
                            );
                        }
                        ui.add_space(8.0);

                        settings_row(ui, "osu! Folder", |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.osu_dir_input)
                                    .desired_width(300.0)
                                    .hint_text("e.g. C:\\Users\\You\\AppData\\Local\\osu!"),
                            );
                            if ui.button("Browse...").clicked() {
                                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                    self.osu_dir_input = path.display().to_string();
                                }
                            }
                        });

                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Apply osu! Folder")
                                            .color(Color32::WHITE),
                                    )
                                    .fill(COL_ACCENT2)
                                    .corner_radius(cr(6))
                                    .min_size(Vec2::new(140.0, 28.0)),
                                )
                                .clicked()
                            {
                                self.config.osu_dir = Some(PathBuf::from(&self.osu_dir_input));
                                self.config.save();
                                self.toasts.success("osu! directory updated.");
                            }

                            if ui.button("Scan Installed Maps").clicked() {
                                self.scan_installed_maps();
                            }
                        });

                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(format!(
                                "{} installed map IDs known",
                                self.config.known_beatmapset_ids.len()
                            ))
                            .color(COL_TEXT_DIM)
                            .font(FontId::proportional(12.0)),
                        );

                        ui.add_space(10.0);

                        let auto_import_changed = ui
                            .checkbox(
                                &mut self.config.auto_import,
                                RichText::new("Auto-import: download maps directly into osu! Songs folder")
                                    .color(COL_TEXT),
                            )
                            .changed();
                        ui.label(
                            RichText::new(
                                "When enabled, .osz files go straight to osu!/Songs and are imported automatically.",
                            )
                            .color(COL_TEXT_DIM)
                            .font(FontId::proportional(11.0)),
                        );

                        ui.add_space(6.0);

                        let k4_changed = ui
                            .checkbox(
                                &mut self.config.download_4k_only,
                                RichText::new("Only download 4K maps in Auto-Download")
                                    .color(COL_TEXT),
                            )
                            .changed();

                        if auto_import_changed || k4_changed {
                            self.config.save();
                        }

                        ui.add_space(24.0);
                        ui.separator();
                        ui.add_space(24.0);

                        settings_section(ui, "Behaviour");
                        ui.add_space(6.0);
                        ui.checkbox(
                            &mut self.config.notification_on_new,
                            RichText::new(
                                "Show notification when new ranked maps are found",
                            )
                            .color(COL_TEXT),
                        );
                        ui.add_space(6.0);
                        ui.checkbox(
                            &mut self.config.auto_download_new,
                            RichText::new(
                                "Auto-download new ranked maps (uses active filters)",
                            )
                            .color(COL_TEXT),
                        );
                        ui.add_space(6.0);
                        ui.checkbox(
                            &mut self.config.prefer_no_video,
                            RichText::new("Prefer downloading without video")
                                .color(COL_TEXT),
                        );

                        if ui.button("Save Settings").clicked() {
                            self.config.save();
                            self.toasts.success("Settings saved.");
                        }

                        ui.add_space(24.0);
                        ui.separator();
                        ui.add_space(24.0);

                        settings_section(ui, "About");
                        ui.label(
                            RichText::new("osu!mania Ranked Downloader v0.2")
                                .color(COL_TEXT_DIM)
                                .font(FontId::proportional(13.0)),
                        );
                        ui.label(
                            RichText::new(
                                "Downloads via Nerinyan -> BeatConnect -> Chimu mirrors.",
                            )
                            .color(COL_TEXT_DIM)
                            .font(FontId::proportional(12.0)),
                        );
                        ui.label(
                            RichText::new(
                                "Auto-import places .osz files directly into osu!/Songs.",
                            )
                            .color(COL_TEXT_DIM)
                            .font(FontId::proportional(12.0)),
                        );
                    });
            });
    }
}

// ─── Card widget ─────────────────────────────────────────────────────────────
enum CardAction {
    Download,
    OpenInBrowser,
    PlayPreview(u64, String),
}

fn show_beatmapset_card(
    ui: &mut Ui,
    bs: &Beatmapset,
    dl_status: Option<DownloadStatus>,
    is_playing: bool,
) -> Option<CardAction> {
    let mut action = None;

    // Avoid layout with zero or NaN width (can happen on first frame)
    let avail_w = ui.available_width();
    if avail_w < 4.0 || avail_w.is_nan() || avail_w.is_infinite() {
        return None;
    }

    let (card_rect, _) =
        ui.allocate_exact_size(Vec2::new(avail_w, 110.0), egui::Sense::hover());

    let painter = ui.painter();

    // Card background
    painter.rect_filled(card_rect, cr(12), COL_CARD_BG);
    painter.rect_stroke(
        card_rect,
        cr(12),
        Stroke::new(1.0, COL_BORDER),
        egui::StrokeKind::Middle,
    );

    // Left accent strip
    let accent_rect = Rect::from_min_size(card_rect.min, Vec2::new(4.0, card_rect.height()));
    painter.rect_filled(
        accent_rect,
        CornerRadius {
            nw: 12,
            sw: 12,
            ne: 0,
            se: 0,
        },
        COL_ACCENT,
    );

    // Thumbnail image area
    let thumb_size = Vec2::new(140.0, card_rect.height());
    let thumb_rect = Rect::from_min_size(card_rect.min + Vec2::new(4.0, 0.0), thumb_size);
    
    let cover_url = bs.covers
        .as_ref()
        .map(|c| c.list.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("https://assets.ppy.sh/beatmaps/{}/covers/list.jpg", bs.id));
    
    // We draw the image via ui.put
    let image_widget = egui::Image::new(&cover_url)
        .fit_to_exact_size(thumb_size)
        .sense(egui::Sense::click());
    
    let img_response = ui.put(thumb_rect, image_widget);
    if img_response.clicked() {
        let preview_url = bs.preview_url
            .as_ref()
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("//b.ppy.sh/preview/{}.mp3", bs.id));
        action = Some(CardAction::PlayPreview(bs.id, preview_url));
    }

    let painter = ui.painter();

    if img_response.hovered() || is_playing {
        let center = thumb_rect.center();
        let bg_rect = Rect::from_center_size(center, Vec2::new(36.0, 36.0));
        painter.rect_filled(bg_rect, cr(18), Color32::from_black_alpha(180));
        painter.text(
            center,
            egui::Align2::CENTER_CENTER,
            if is_playing { "🔊" } else { "▶" },
            FontId::proportional(20.0),
            Color32::WHITE,
        );
    }

    let tx_pos = card_rect.min + Vec2::new(160.0, 12.0);

    painter.text(tx_pos, egui::Align2::LEFT_TOP, &bs.title, FontId::proportional(16.0), COL_TEXT);
    painter.text(
        tx_pos + Vec2::new(0.0, 22.0),
        egui::Align2::LEFT_TOP,
        format!("by {} — mapped by {}", bs.artist, bs.creator),
        FontId::proportional(12.5),
        COL_TEXT_DIM,
    );

    // Stats
    let mut parts = Vec::new();
    let keys = bs.key_counts();
    if !keys.is_empty() {
        parts.push(format!(
            "🎹 {}",
            keys.iter()
                .map(|k| format!("{}K", k))
                .collect::<Vec<_>>()
                .join("/")
        ));
    }
    if let Some(bpm) = bs.bpm {
        parts.push(format!("♩ {:.0}", bpm));
    }
    let sr = bs.max_sr();
    if sr > 0.0 {
        parts.push(format!("★ {:.2}", sr));
    }
    parts.push(format!("▶ {}", fmt_num(bs.play_count)));
    parts.push(format!("♥ {}", fmt_num(bs.favourite_count)));

    painter.text(
        tx_pos + Vec2::new(0.0, 44.0),
        egui::Align2::LEFT_TOP,
        parts.join("  ·  "),
        FontId::proportional(12.0),
        COL_ACCENT,
    );

    if let Some(dt) = &bs.ranked_date {
        painter.text(
            tx_pos + Vec2::new(0.0, 66.0),
            egui::Align2::LEFT_TOP,
            format!("Ranked {}", dt.format("%Y-%m-%d")),
            FontId::proportional(11.0),
            Color32::from_rgb(100, 95, 140),
        );
    }

    // Buttons area (right side)
    let btn_cx = card_rect.max.x - 68.0;
    let btn_cy = card_rect.min.y + card_rect.height() / 2.0 - 8.0;
    let dl_btn = Rect::from_center_size(Pos2::new(btn_cx, btn_cy), Vec2::new(116.0, 32.0));

    match &dl_status {
        Some(DownloadStatus::Downloading { progress }) => {
            painter.rect_filled(dl_btn, cr(6), COL_SURFACE2);
            if *progress >= 0.0 {
                let fill = Rect::from_min_size(
                    dl_btn.min,
                    Vec2::new(dl_btn.width() * progress, dl_btn.height()),
                );
                painter.rect_filled(fill, cr(6), COL_ACCENT2);
            }
            painter.text(
                dl_btn.center(),
                egui::Align2::CENTER_CENTER,
                if *progress >= 0.0 {
                    format!("{:.0}%", progress * 100.0)
                } else {
                    "Downloading...".into()
                },
                FontId::proportional(12.0),
                Color32::WHITE,
            );
        }
        Some(DownloadStatus::Completed) | Some(DownloadStatus::AlreadyExists) => {
            painter.rect_filled(dl_btn, cr(6), COL_SUCCESS.linear_multiply(0.15));
            painter.text(
                dl_btn.center(),
                egui::Align2::CENTER_CENTER,
                "Downloaded",
                FontId::proportional(12.0),
                COL_SUCCESS,
            );
        }
        Some(DownloadStatus::Failed(_)) => {
            painter.rect_filled(dl_btn, cr(6), COL_ERROR.linear_multiply(0.15));
            painter.text(
                dl_btn.center(),
                egui::Align2::CENTER_CENTER,
                "Failed",
                FontId::proportional(12.0),
                COL_ERROR,
            );
        }
        Some(DownloadStatus::Queued) => {
            painter.rect_filled(dl_btn, cr(6), COL_WARNING.linear_multiply(0.15));
            painter.text(
                dl_btn.center(),
                egui::Align2::CENTER_CENTER,
                "Queued",
                FontId::proportional(12.0),
                COL_WARNING,
            );
        }
        None => {
            let dl_resp = ui.interact(
                dl_btn,
                ui.id().with(format!("dl_btn_{}", bs.id)),
                egui::Sense::click(),
            );
            let hover = dl_resp.hovered();
            
            painter.rect_filled(
                dl_btn,
                cr(6),
                if hover { COL_ACCENT } else { COL_ACCENT.linear_multiply(0.7) },
            );
            painter.text(
                dl_btn.center(),
                egui::Align2::CENTER_CENTER,
                "Download",
                FontId::proportional(13.0),
                Color32::WHITE,
            );
            
            if dl_resp.clicked() {
                action = Some(CardAction::Download);
            }
        }
    }

    // Web button
    let web_btn =
        Rect::from_center_size(Pos2::new(btn_cx, btn_cy + 40.0), Vec2::new(116.0, 24.0));
    
    let web_resp = ui.interact(
        web_btn,
        ui.id().with(format!("web_btn_{}", bs.id)),
        egui::Sense::click(),
    );
    let hover_web = web_resp.hovered();
    
    if hover_web {
        painter.rect_filled(web_btn, cr(5), COL_SURFACE2);
    }
    painter.rect_stroke(web_btn, cr(5), Stroke::new(1.0, COL_BORDER), egui::StrokeKind::Middle);
    painter.text(
        web_btn.center(),
        egui::Align2::CENTER_CENTER,
        "Open in Browser",
        FontId::proportional(11.0),
        COL_TEXT_DIM,
    );
    
    if action.is_none() && web_resp.clicked() {
        action = Some(CardAction::OpenInBrowser);
    }

    action
}

fn show_download_item(ui: &mut Ui, item: &DownloadItem) {
    let (row_rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 56.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(row_rect, cr(8), COL_CARD_BG);

    let tx = Pos2::new(row_rect.min.x + 16.0, row_rect.min.y + 8.0);
    painter.text(
        tx,
        egui::Align2::LEFT_TOP,
        format!("{} — {}", item.artist, item.title),
        FontId::proportional(14.0),
        COL_TEXT,
    );

    match &item.status {
        DownloadStatus::Queued => {
            painter.text(
                tx + Vec2::new(0.0, 22.0),
                egui::Align2::LEFT_TOP,
                "Queued",
                FontId::proportional(12.0),
                COL_WARNING,
            );
        }
        DownloadStatus::Downloading { progress } => {
            let bar = Rect::from_min_size(
                tx + Vec2::new(0.0, 22.0),
                Vec2::new(row_rect.width() - 200.0, 8.0),
            );
            painter.rect_filled(bar, cr(4), COL_SURFACE2);
            if *progress >= 0.0 {
                painter.rect_filled(
                    Rect::from_min_size(bar.min, Vec2::new(bar.width() * progress, 8.0)),
                    cr(4),
                    COL_ACCENT,
                );
            }
            let pct = if *progress >= 0.0 {
                format!("{:.0}%", progress * 100.0)
            } else {
                "...".into()
            };
            painter.text(
                Pos2::new(row_rect.max.x - 80.0, tx.y + 22.0),
                egui::Align2::LEFT_TOP,
                pct,
                FontId::proportional(12.0),
                COL_TEXT_DIM,
            );
        }
        DownloadStatus::Completed => {
            painter.text(
                tx + Vec2::new(0.0, 22.0),
                egui::Align2::LEFT_TOP,
                "Downloaded",
                FontId::proportional(12.0),
                COL_SUCCESS,
            );
        }
        DownloadStatus::AlreadyExists => {
            painter.text(
                tx + Vec2::new(0.0, 22.0),
                egui::Align2::LEFT_TOP,
                "Already downloaded",
                FontId::proportional(12.0),
                COL_TEXT_DIM,
            );
        }
        DownloadStatus::Failed(e) => {
            painter.text(
                tx + Vec2::new(0.0, 22.0),
                egui::Align2::LEFT_TOP,
                format!("{}", e),
                FontId::proportional(12.0),
                COL_ERROR,
            );
        }
    }
}

// ─── Helper widgets ───────────────────────────────────────────────────────────
fn tab_button(ui: &mut Ui, label: &str, active: bool, mut on_click: impl FnMut()) {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(140.0, 44.0), egui::Sense::click());
    if resp.clicked() {
        on_click();
    }
    let painter = ui.painter();
    if active {
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(rect.min.x, rect.max.y - 3.0),
                Vec2::new(rect.width(), 3.0),
            ),
            cr(2),
            COL_ACCENT,
        );
    }
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(14.0),
        if active || resp.hovered() { COL_TEXT } else { COL_TEXT_DIM },
    );
}

fn key_toggle(ui: &mut Ui, label: &str, state: &mut bool) {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(46.0, 28.0), egui::Sense::click());
    if resp.clicked() {
        *state = !*state;
    }
    let painter = ui.painter();
    painter.rect_filled(rect, cr(6), if *state { COL_ACCENT } else { COL_SURFACE2 });
    if !*state {
        painter.rect_stroke(rect, cr(6), Stroke::new(1.0, COL_BORDER), egui::StrokeKind::Middle);
    }
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(12.0),
        if *state { Color32::WHITE } else { COL_TEXT_DIM },
    );
}

fn settings_section(ui: &mut Ui, label: &str) {
    ui.label(
        RichText::new(label)
            .font(FontId::proportional(17.0))
            .color(COL_TEXT)
            .strong(),
    );
    ui.add_space(8.0);
}

fn settings_row(ui: &mut Ui, label: &str, add_content: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(28.0);
        ui.label(
            RichText::new(label)
                .color(COL_TEXT_DIM)
                .font(FontId::proportional(13.0)),
        );
        ui.add_space(8.0);
        add_content(ui);
    });
    ui.add_space(6.0);
}

fn draw_osu_logo(painter: &egui::Painter, center: Pos2, radius: f32) {
    painter.circle_filled(center, radius, COL_ACCENT);
    painter.circle_filled(center, radius * 0.65, COL_BG);
    painter.circle_stroke(center, radius * 0.4, Stroke::new(2.5, COL_ACCENT));
}

fn fmt_num(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

// ─── Font & visuals ───────────────────────────────────────────────────────────
fn setup_fonts(ctx: &egui::Context) {
    let fonts = egui::FontDefinitions::default();
    ctx.set_fonts(fonts);

    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (egui::TextStyle::Heading, FontId::proportional(22.0)),
        (egui::TextStyle::Body, FontId::proportional(14.0)),
        (egui::TextStyle::Monospace, FontId::monospace(13.0)),
        (egui::TextStyle::Button, FontId::proportional(14.0)),
        (egui::TextStyle::Small, FontId::proportional(12.0)),
    ]
    .into();
    ctx.set_style(style);
}

fn setup_visuals(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();

    visuals.override_text_color = Some(COL_TEXT);
    visuals.panel_fill = COL_BG;
    visuals.window_fill = COL_SURFACE;
    visuals.window_stroke = Stroke::new(1.0, COL_BORDER);
    visuals.widgets.noninteractive.bg_fill = COL_SURFACE;
    visuals.widgets.inactive.bg_fill = COL_SURFACE2;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, COL_BORDER);
    visuals.widgets.hovered.bg_fill = COL_SURFACE2.linear_multiply(1.2);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, COL_ACCENT);
    visuals.widgets.active.bg_fill = COL_ACCENT.linear_multiply(0.8);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, COL_ACCENT);
    visuals.selection.bg_fill = COL_ACCENT.linear_multiply(0.4);
    visuals.selection.stroke = Stroke::new(1.0, COL_ACCENT);
    visuals.hyperlink_color = COL_ACCENT;
    visuals.faint_bg_color = COL_SURFACE;
    visuals.extreme_bg_color = COL_BG;

    ctx.set_visuals(visuals);
}
