#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod model;
mod netsetup;
mod transfer;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use egui::{Align, Align2, Color32, CornerRadius, FontId, Layout, Margin, RichText, Stroke, StrokeKind};

use config::Config;
use model::{ConnState, FileJob, FileOp, JobStatus, Library, LibraryStatus, RemoteFile};

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Send,
    Library,
}

/// Action requested from a remote-file row, dispatched after the list is drawn.
enum RemoteAction {
    Download(Arc<RemoteFile>),
    Delete(Arc<RemoteFile>),
}

// ---- palette -------------------------------------------------------------
const BG: Color32 = Color32::from_rgb(0x14, 0x16, 0x1A);
const PANEL: Color32 = Color32::from_rgb(0x1B, 0x1E, 0x24);
const CARD: Color32 = Color32::from_rgb(0x1E, 0x22, 0x29);
const BORDER: Color32 = Color32::from_rgb(0x2A, 0x2F, 0x38);
const TEXT: Color32 = Color32::from_rgb(0xE6, 0xE8, 0xEC);
const MUTED: Color32 = Color32::from_rgb(0x8A, 0x90, 0x9B);
const ACCENT: Color32 = Color32::from_rgb(0xE8, 0xA3, 0x3D);
const GREEN: Color32 = Color32::from_rgb(0x3F, 0xB3, 0x7F);
const RED: Color32 = Color32::from_rgb(0xE5, 0x48, 0x4D);
const GREY: Color32 = Color32::from_rgb(0x5A, 0x61, 0x6D);

fn main() -> eframe::Result {
    // Headless mode:  kindle-drop --send <file>...
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--send") {
        let cfg = Config::load();
        let paths: Vec<PathBuf> = args[pos + 1..].iter().map(PathBuf::from).collect();
        if paths.is_empty() {
            eprintln!("usage: kindle-drop --send <file>...");
            std::process::exit(2);
        }
        let rc = transfer::cli_upload(&cfg.kindle_ip, "root", &cfg.target_dir, &paths);
        std::process::exit(rc);
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([560.0, 700.0])
            .with_min_inner_size([460.0, 560.0])
            .with_app_id("kindle-drop")
            .with_icon(app_icon())
            .with_title("Kindle Drop"),
        ..Default::default()
    };
    eframe::run_native(
        "Kindle Drop",
        options,
        Box::new(|cc| {
            install_theme(&cc.egui_ctx);
            Ok(Box::new(App::new()))
        }),
    )
}

/// Cohesive dark theme applied to both theme slots so it holds regardless of the
/// system light/dark preference.
fn install_theme(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);

    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.extreme_bg_color = Color32::from_rgb(0x0F, 0x11, 0x15);
    v.faint_bg_color = Color32::from_rgb(0x20, 0x24, 0x2B);
    v.override_text_color = Some(TEXT);
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.window_corner_radius = CornerRadius::same(10);
    v.selection.bg_fill = ACCENT.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;

    let r = CornerRadius::same(8);
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.weak_bg_fill = PANEL;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, MUTED);
    v.widgets.noninteractive.corner_radius = r;

    v.widgets.inactive.bg_fill = Color32::from_rgb(0x25, 0x2A, 0x33);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x25, 0x2A, 0x33);
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.inactive.corner_radius = r;

    v.widgets.hovered.bg_fill = Color32::from_rgb(0x30, 0x36, 0x41);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x30, 0x36, 0x41);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT.gamma_multiply(0.6));
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.hovered.corner_radius = r;

    v.widgets.active.bg_fill = ACCENT.gamma_multiply(0.9);
    v.widgets.active.weak_bg_fill = ACCENT.gamma_multiply(0.9);
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.active.fg_stroke = Stroke::new(1.0, BG);
    v.widgets.active.corner_radius = r;

    ctx.all_styles_mut(|s| {
        s.visuals = v.clone();
        s.spacing.item_spacing = egui::vec2(10.0, 10.0);
        s.spacing.button_padding = egui::vec2(12.0, 8.0);
        s.spacing.interact_size.y = 30.0;
        use egui::{FontFamily::Proportional, TextStyle};
        s.text_styles.insert(TextStyle::Heading, FontId::new(21.0, Proportional));
        s.text_styles.insert(TextStyle::Body, FontId::new(14.0, Proportional));
        s.text_styles.insert(TextStyle::Button, FontId::new(14.0, Proportional));
        s.text_styles.insert(TextStyle::Small, FontId::new(11.5, Proportional));
    });
}

/// Extensions treated as "books" by the Library filter (KUAL's `.azw2` app and
/// other non-book files are intentionally excluded).
const BOOK_EXTS: &[&str] = &[
    "epub", "pdf", "mobi", "azw", "azw3", "kfx", "txt", "fb2", "cbz", "cbr", "djvu", "prc",
    "pdb", "rtf", "doc", "docx", "htm", "html", "lit",
];

fn is_book(name: &str) -> bool {
    name.rsplit_once('.')
        .map(|(_, ext)| BOOK_EXTS.contains(&ext.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

struct App {
    cfg: Config,
    tab: Tab,
    jobs: Vec<Arc<FileJob>>,
    library: Arc<Mutex<Library>>,
    confirm_delete: Option<Arc<RemoteFile>>,
    books_only: bool,
    ifaces: Vec<String>,
    selected_iface: Option<String>,
    conn: Arc<Mutex<ConnState>>,
    transferring: Arc<AtomicBool>,
    koreader_busy: Arc<AtomicBool>,
    koreader_msg: Arc<Mutex<String>>,
    show_settings: bool,
    watcher_started: bool,
}

impl App {
    fn new() -> Self {
        let cfg = Config::load();
        let ifaces = netsetup::detect_candidates();
        let selected = cfg
            .last_iface
            .clone()
            .filter(|i| ifaces.contains(i))
            .or_else(|| ifaces.first().cloned());
        let mut app = Self {
            cfg,
            tab: Tab::Send,
            jobs: Vec::new(),
            library: Arc::new(Mutex::new(Library::new())),
            confirm_delete: None,
            books_only: true,
            ifaces,
            selected_iface: selected,
            conn: Arc::new(Mutex::new(ConnState::Disconnected)),
            transferring: Arc::new(AtomicBool::new(false)),
            koreader_busy: Arc::new(AtomicBool::new(false)),
            koreader_msg: Arc::new(Mutex::new(String::new())),
            show_settings: false,
            watcher_started: false,
        };
        if std::env::args().any(|a| a == "--demo") {
            app.seed_demo();
        }
        if std::env::args().any(|a| a == "library") {
            app.tab = Tab::Library;
        }
        app
    }

    /// Populate the UI with sample data for screenshots / offline UI testing.
    fn seed_demo(&mut self) {
        *self.conn.lock().unwrap() = ConnState::Connected;
        let mb = 1024 * 1024;
        self.jobs = [
            FileJob::demo("The Pragmatic Programmer.epub", 4 * mb + 210 * 1024, 4 * mb + 210 * 1024, JobStatus::Done),
            FileJob::demo("Dune — Frank Herbert.azw3", 1854 * 1024, 1140 * 1024, JobStatus::Sending),
            FileJob::demo("Thinking in Systems.pdf", 12 * mb + 300 * 1024, 0, JobStatus::Queued),
            FileJob::demo("SICP.mobi", 3 * mb + 90 * 1024, 0, JobStatus::Queued),
        ]
        .into_iter()
        .map(Arc::new)
        .collect();
        self.transferring.store(true, Ordering::Relaxed);

        let mut lib = self.library.lock().unwrap();
        lib.status = LibraryStatus::Loaded;
        lib.files = [
            ("The Pragmatic Programmer.epub", 4 * mb + 210 * 1024),
            ("Project Hail Mary.azw3", 2 * mb + 640 * 1024),
            ("Designing Data-Intensive Applications.pdf", 18 * mb + 120 * 1024),
            ("Meditations — Marcus Aurelius.mobi", 820 * 1024),
            ("KUAL-KDK-1.0.azw2", 48 * 1024),
        ]
        .into_iter()
        .map(|(n, s)| Arc::new(RemoteFile::new(n.to_string(), s)))
        .collect();
    }

    fn conn_state(&self) -> ConnState {
        self.conn.lock().unwrap().clone()
    }

    fn add_paths(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        for p in paths {
            if self.jobs.iter().any(|j| j.path == p) {
                continue;
            }
            if let Some(job) = FileJob::new(p) {
                self.jobs.push(Arc::new(job));
            }
        }
    }

    fn pending_count(&self) -> usize {
        self.jobs
            .iter()
            .filter(|j| matches!(j.status(), JobStatus::Queued | JobStatus::Error(_)))
            .count()
    }

    /// Bring up the USB link (via polkit) and verify SSH reachability, off-thread.
    fn start_connect(&mut self, ctx: &egui::Context) {
        let iface = self
            .selected_iface
            .clone()
            .or_else(|| netsetup::detect_candidates().into_iter().next());
        self.cfg.last_iface = iface.clone();
        self.cfg.save();

        let conn = self.conn.clone();
        let ctx = ctx.clone();
        let cfg = self.cfg.clone();
        *conn.lock().unwrap() = ConnState::Connecting;

        std::thread::spawn(move || {
            let set = |s: ConnState| {
                *conn.lock().unwrap() = s;
                ctx.request_repaint();
            };
            // Already reachable? Skip the privileged network step entirely — no
            // polkit prompt when you're effectively already connected.
            if netsetup::reachable(&cfg.kindle_ip, 22, Duration::from_millis(700)) {
                set(ConnState::Connected);
                return;
            }
            let Some(iface) = iface else {
                set(ConnState::Failed("no Kindle USB interface found".into()));
                return;
            };
            if let Err(e) = netsetup::bring_up(&iface, &cfg.host_cidr) {
                set(ConnState::Failed(e));
                return;
            }
            // Link may take a moment to come up; retry the reachability probe.
            let reachable = (0..12).any(|i| {
                if i > 0 {
                    std::thread::sleep(Duration::from_millis(400));
                }
                netsetup::reachable(&cfg.kindle_ip, 22, Duration::from_millis(700))
            });
            set(if reachable {
                ConnState::Connected
            } else {
                ConnState::Failed(format!("no SSH on {}:22", cfg.kindle_ip))
            });
        });
    }

    /// Spawn (once) a background watcher that flips the pill to Disconnected when
    /// the Kindle stops responding — e.g. when you unplug the cable.
    fn ensure_watcher(&mut self, ctx: &egui::Context) {
        if self.watcher_started {
            return;
        }
        self.watcher_started = true;
        let conn = self.conn.clone();
        let ctx = ctx.clone();
        let ip = self.cfg.kindle_ip.clone();
        std::thread::spawn(move || {
            let mut misses = 0;
            loop {
                std::thread::sleep(Duration::from_secs(2));
                if *conn.lock().unwrap() != ConnState::Connected {
                    misses = 0;
                    continue;
                }
                if netsetup::reachable(&ip, 22, Duration::from_millis(800)) {
                    misses = 0;
                } else {
                    misses += 1;
                    if misses >= 2 {
                        misses = 0;
                        if *conn.lock().unwrap() == ConnState::Connected {
                            *conn.lock().unwrap() = ConnState::Disconnected;
                            ctx.request_repaint();
                        }
                    }
                }
            }
        });
    }

    /// Upload all pending files, off-thread.
    fn start_send(&mut self, ctx: &egui::Context) {
        let jobs: Vec<Arc<FileJob>> = self
            .jobs
            .iter()
            .filter(|j| matches!(j.status(), JobStatus::Queued | JobStatus::Error(_)))
            .cloned()
            .collect();
        if jobs.is_empty() {
            return;
        }
        for j in &jobs {
            j.sent.store(0, Ordering::Relaxed);
            j.set_status(JobStatus::Queued);
        }
        let ip = self.cfg.kindle_ip.clone();
        let dir = self.cfg.target_dir.clone();
        let ctx = ctx.clone();
        let active = self.transferring.clone();
        // For the optional "open KOReader after sending" step.
        let after = self.cfg.koreader_after_send;
        let ko_cmd = self.cfg.koreader_cmd.clone();
        let ko_busy = self.koreader_busy.clone();
        let ko_msg = self.koreader_msg.clone();
        std::thread::spawn(move || {
            transfer::run_batch(ip.clone(), "root".into(), dir, jobs.clone(), ctx.clone(), active);
            if after && jobs.iter().any(|j| matches!(j.status(), JobStatus::Done)) {
                open_koreader(ip, ko_cmd, ko_busy, ko_msg, ctx);
            }
        });
    }

    /// Relaunch KOReader on the Kindle, off-thread.
    fn start_koreader(&self, ctx: &egui::Context) {
        let ip = self.cfg.kindle_ip.clone();
        let cmd = self.cfg.koreader_cmd.clone();
        let busy = self.koreader_busy.clone();
        let msg = self.koreader_msg.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || open_koreader(ip, cmd, busy, msg, ctx));
    }

    /// Fetch the Kindle's file listing, off-thread.
    fn start_refresh(&self, ctx: &egui::Context) {
        let lib = self.library.clone();
        let ctx = ctx.clone();
        let ip = self.cfg.kindle_ip.clone();
        let dir = self.cfg.target_dir.clone();
        lib.lock().unwrap().status = LibraryStatus::Loading;
        std::thread::spawn(move || {
            match transfer::list_remote(&ip, "root", &dir) {
                Ok(files) => {
                    let mut l = lib.lock().unwrap();
                    l.files = files.into_iter().map(Arc::new).collect();
                    l.status = LibraryStatus::Loaded;
                }
                Err(e) => {
                    let mut l = lib.lock().unwrap();
                    l.files.clear();
                    l.status = LibraryStatus::Error(e);
                }
            }
            ctx.request_repaint();
        });
    }

    /// Delete one remote file (and its `.sdr` sidecar), off-thread.
    fn start_delete(&self, ctx: &egui::Context, file: Arc<RemoteFile>) {
        let lib = self.library.clone();
        let ctx = ctx.clone();
        let ip = self.cfg.kindle_ip.clone();
        let dir = self.cfg.target_dir.clone();
        file.set_op(FileOp::Deleting);
        std::thread::spawn(move || {
            match transfer::delete_remote(&ip, "root", &dir, &file.name) {
                Ok(()) => lib.lock().unwrap().files.retain(|f| !Arc::ptr_eq(f, &file)),
                Err(e) => file.set_op(FileOp::Error(e)),
            }
            ctx.request_repaint();
        });
    }

    /// Download one remote file to the PC's download folder, off-thread.
    fn start_download(&self, ctx: &egui::Context, file: Arc<RemoteFile>) {
        let ctx = ctx.clone();
        let ip = self.cfg.kindle_ip.clone();
        let dir = self.cfg.target_dir.clone();
        let dest = self.cfg.download_path();
        file.got.store(0, Ordering::Relaxed);
        file.set_op(FileOp::Downloading);
        std::thread::spawn(move || {
            match transfer::download_remote(&ip, "root", &dir, &file, &dest) {
                Ok(path) => file.set_op(FileOp::Saved(path.display().to_string())),
                Err(e) => file.set_op(FileOp::Error(e)),
            }
            ctx.request_repaint();
        });
    }

    /// True while the library is loading or any remote file op is in flight.
    fn library_busy(&self) -> bool {
        let l = self.library.lock().unwrap();
        matches!(l.status, LibraryStatus::Loading)
            || l.files
                .iter()
                .any(|f| matches!(f.op(), FileOp::Downloading | FileOp::Deleting))
    }

    // ---- UI pieces -------------------------------------------------------

    fn header(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("Kindle Drop").size(21.0).strong().color(TEXT));
                ui.label(RichText::new("Sideload books over USBnet").small().color(MUTED));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                self.connection_pill(ui);
            });
        });
    }

    fn connection_pill(&self, ui: &mut egui::Ui) {
        let (dot, label) = match self.conn_state() {
            ConnState::Disconnected => (GREY, "Not connected".to_string()),
            ConnState::Connecting => (ACCENT, "Connecting…".to_string()),
            ConnState::Connected => (GREEN, format!("Connected · {}", self.cfg.kindle_ip)),
            ConnState::Failed(e) => (RED, e),
        };
        egui::Frame::group(ui.style())
            .fill(PANEL)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::symmetric(10, 5))
            .show(ui, |ui| {
                // The header places this pill in a right-to-left layout (so it
                // hugs its content at the right edge). Items are therefore laid
                // out right-to-left: add the label first, then the dot, so the
                // dot ends up on the left.
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(RichText::new(label).small().color(TEXT));
                status_dot(ui, dot);
            });
    }

    fn drop_zone(&mut self, ui: &mut egui::Ui, hovering: bool) {
        let size = egui::vec2(ui.available_width(), 148.0);
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());

        let fill = if hovering {
            Color32::from_rgb(0x24, 0x29, 0x31)
        } else {
            Color32::from_rgb(0x17, 0x1A, 0x20)
        };
        let stroke = if hovering { ACCENT } else { BORDER };
        let radius = CornerRadius::same(12);
        let painter = ui.painter();
        painter.rect_filled(rect, radius, fill);
        painter.rect_stroke(rect, radius, Stroke::new(1.5, stroke), StrokeKind::Inside);

        let center = rect.center();
        painter.text(
            center - egui::vec2(0.0, 20.0),
            Align2::CENTER_CENTER,
            "⬇",
            FontId::proportional(34.0),
            if hovering { ACCENT } else { MUTED },
        );
        painter.text(
            center + egui::vec2(0.0, 18.0),
            Align2::CENTER_CENTER,
            "Drop books here",
            FontId::proportional(17.0),
            TEXT,
        );
        painter.text(
            center + egui::vec2(0.0, 42.0),
            Align2::CENTER_CENTER,
            "EPUB · MOBI · PDF · AZW3 — or use the button below",
            FontId::proportional(11.5),
            MUTED,
        );
    }

    /// Tab switcher on the left, connection + settings controls on the right.
    fn tab_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let connecting = matches!(self.conn_state(), ConnState::Connecting);
        ui.horizontal(|ui| {
            let tab = |ui: &mut egui::Ui, sel: bool, text: &str| {
                ui.selectable_label(sel, RichText::new(text).size(15.0))
            };
            if tab(ui, self.tab == Tab::Send, "  Send  ").clicked() {
                self.tab = Tab::Send;
            }
            if tab(ui, self.tab == Tab::Library, "  Library  ").clicked() {
                self.tab = Tab::Library;
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .selectable_label(self.show_settings, "⚙")
                    .on_hover_text("Settings")
                    .clicked()
                {
                    self.show_settings = !self.show_settings;
                    if !self.show_settings {
                        self.cfg.save();
                    }
                }
                let label = if connecting { "Connecting…" } else { "Connect" };
                if ui
                    .add_enabled(!connecting, egui::Button::new(label))
                    .clicked()
                {
                    self.start_connect(ctx);
                }
            });
        });
    }

    /// Send-tab action row: add files + the primary send button.
    fn send_controls(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let busy = self.transferring.load(Ordering::Relaxed);
        let ko_busy = self.koreader_busy.load(Ordering::Relaxed);
        ui.horizontal(|ui| {
            if ui.button("+  Add files…").clicked() {
                if let Some(paths) = rfd::FileDialog::new().set_title("Select books").pick_files() {
                    self.add_paths(paths);
                }
            }
            let ko_label = if ko_busy { "Restarting…" } else { "Restart KOReader" };
            if ui
                .add_enabled(!ko_busy, egui::Button::new(ko_label))
                .on_hover_text("Restart KOReader on the Kindle so it picks up newly sent books")
                .clicked()
            {
                self.start_koreader(ctx);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let pending = self.pending_count();
                let can_send = pending > 0 && !busy;
                let label = if busy {
                    "Sending…".to_string()
                } else {
                    format!("Send to Kindle  ({pending})")
                };
                let btn = egui::Button::new(RichText::new(label).color(BG).strong()).fill(ACCENT);
                if ui.add_enabled(can_send, btn).clicked() {
                    self.start_send(ctx);
                }
            });
        });

        let msg = self.koreader_msg.lock().unwrap().clone();
        if !msg.is_empty() {
            let color = if msg.starts_with("Couldn't") { RED } else { MUTED };
            ui.label(RichText::new(msg).small().color(color));
        }
    }

    fn settings_panel(&mut self, ui: &mut egui::Ui) {
        egui::Frame::group(ui.style())
            .fill(PANEL)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                egui::Grid::new("settings_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(RichText::new("Interface").color(MUTED));
                        ui.horizontal(|ui| {
                            let ifaces = self.ifaces.clone();
                            let selected = self
                                .selected_iface
                                .clone()
                                .unwrap_or_else(|| "— none —".into());
                            egui::ComboBox::from_id_salt("iface_combo")
                                .selected_text(selected)
                                .show_ui(ui, |ui| {
                                    for i in &ifaces {
                                        ui.selectable_value(
                                            &mut self.selected_iface,
                                            Some(i.clone()),
                                            i,
                                        );
                                    }
                                });
                            if ui.button("Rescan").on_hover_text("Re-detect USB-ethernet interfaces").clicked() {
                                self.ifaces = netsetup::detect_candidates();
                                // Reset the selection if the current one vanished.
                                if self
                                    .selected_iface
                                    .as_ref()
                                    .map_or(true, |s| !self.ifaces.contains(s))
                                {
                                    self.selected_iface = self.ifaces.first().cloned();
                                }
                            }
                            ui.label(
                                RichText::new(format!("{} found", self.ifaces.len()))
                                    .small()
                                    .color(MUTED),
                            );
                        });
                        ui.end_row();

                        ui.label(RichText::new("Kindle IP").color(MUTED));
                        ui.add(egui::TextEdit::singleline(&mut self.cfg.kindle_ip).desired_width(220.0));
                        ui.end_row();

                        ui.label(RichText::new("Host address/CIDR").color(MUTED));
                        ui.add(egui::TextEdit::singleline(&mut self.cfg.host_cidr).desired_width(220.0));
                        ui.end_row();

                        ui.label(RichText::new("Target dir").color(MUTED));
                        ui.add(egui::TextEdit::singleline(&mut self.cfg.target_dir).desired_width(220.0));
                        ui.end_row();

                        ui.label(RichText::new("After sending").color(MUTED));
                        ui.checkbox(&mut self.cfg.koreader_after_send, "Restart KOReader automatically");
                        ui.end_row();

                        ui.label(RichText::new("KOReader command").color(MUTED));
                        ui.vertical(|ui| {
                            ui.add(
                                egui::TextEdit::multiline(&mut self.cfg.koreader_cmd)
                                    .desired_width(340.0)
                                    .desired_rows(2)
                                    .font(egui::TextStyle::Monospace),
                            );
                            if ui.small_button("Reset to default").clicked() {
                                self.cfg.koreader_cmd = config::DEFAULT_KOREADER_CMD.to_string();
                            }
                        });
                        ui.end_row();
                    });
            });
    }

    fn queue(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Queue · {} file(s)", self.jobs.len())).color(MUTED));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(egui::Button::new(RichText::new("Clear done").small()).frame(false)).clicked() {
                    self.jobs.retain(|j| !matches!(j.status(), JobStatus::Done));
                }
            });
        });

        let mut remove: Option<usize> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if self.jobs.is_empty() {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("No files queued yet.").color(MUTED));
                });
                return;
            }
            for (i, job) in self.jobs.iter().enumerate() {
                if self.job_row(ui, job) {
                    remove = Some(i);
                }
            }
        });

        if let Some(i) = remove {
            self.jobs.remove(i);
        }
    }

    /// Renders one queue row. Returns true if its remove button was clicked.
    fn job_row(&self, ui: &mut egui::Ui, job: &FileJob) -> bool {
        let mut remove = false;
        let (dot, badge, badge_color) = match job.status() {
            JobStatus::Queued => (GREY, "Queued".to_string(), MUTED),
            JobStatus::Sending => (ACCENT, "Sending".to_string(), ACCENT),
            JobStatus::Done => (GREEN, "Done".to_string(), GREEN),
            JobStatus::Error(_) => (RED, "Error".to_string(), RED),
        };

        egui::Frame::group(ui.style())
            .fill(CARD)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    status_dot(ui, dot);
                    ui.add_space(2.0);
                    ui.label(RichText::new(&job.name).strong().color(TEXT));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(egui::Button::new(RichText::new("×").size(16.0).color(MUTED)).frame(false))
                            .on_hover_text("Remove")
                            .clicked()
                        {
                            remove = true;
                        }
                        ui.label(RichText::new(badge).small().color(badge_color));
                    });
                });

                ui.add(
                    egui::ProgressBar::new(job.progress())
                        .desired_height(7.0)
                        .corner_radius(CornerRadius::same(4))
                        .fill(if matches!(job.status(), JobStatus::Error(_)) { RED } else { ACCENT }),
                );

                let detail = match job.status() {
                    JobStatus::Error(e) => e,
                    JobStatus::Done => format!("{} — sent", human(job.size)),
                    JobStatus::Sending => format!("{} / {}", human(job.sent()), human(job.size)),
                    JobStatus::Queued => human(job.size),
                };
                ui.label(RichText::new(detail).small().color(MUTED));
            });
        remove
    }

    fn library_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let (status, files) = {
            let l = self.library.lock().unwrap();
            (l.status.clone(), l.files.clone())
        };

        // First visit while connected: load automatically.
        if matches!(status, LibraryStatus::Idle)
            && matches!(self.conn_state(), ConnState::Connected)
        {
            self.start_refresh(ctx);
        }

        // Toolbar: refresh + download destination.
        ui.horizontal(|ui| {
            let loading = matches!(status, LibraryStatus::Loading);
            let label = if loading { "Refreshing…" } else { "Refresh" };
            if ui.add_enabled(!loading, egui::Button::new(label)).clicked() {
                self.start_refresh(ctx);
            }
            ui.add_space(6.0);
            ui.checkbox(&mut self.books_only, "Books only")
                .on_hover_text("Hide non-book files (e.g. the KUAL launcher)");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Change…").on_hover_text("Choose download folder").clicked() {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        self.cfg.download_dir = Some(dir.display().to_string());
                        self.cfg.save();
                    }
                }
                let dest = self.cfg.download_path();
                ui.label(
                    RichText::new(format!("Save to  {}", dest.display()))
                        .small()
                        .color(MUTED),
                );
            });
        });
        ui.add_space(4.0);

        let mut action: Option<RemoteAction> = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| match &status {
                LibraryStatus::Idle => {
                    hint(ui, "Connect to the Kindle, then press Refresh to list its books.");
                }
                LibraryStatus::Loading => {
                    ui.add_space(24.0);
                    ui.vertical_centered(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Reading the Kindle…").color(MUTED));
                    });
                }
                LibraryStatus::Error(e) => {
                    ui.add_space(16.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("Couldn't read the Kindle").color(RED).strong());
                        ui.label(RichText::new(e).small().color(MUTED));
                    });
                }
                LibraryStatus::Loaded => {
                    let shown: Vec<&Arc<RemoteFile>> = files
                        .iter()
                        .filter(|f| !self.books_only || is_book(&f.name))
                        .collect();
                    let hidden = files.len() - shown.len();
                    if shown.is_empty() {
                        let msg = if hidden > 0 {
                            format!("No books to show ({hidden} non-book file(s) hidden).")
                        } else {
                            "No books on the device.".to_string()
                        };
                        hint(ui, &msg);
                    } else {
                        for file in shown {
                            if let Some(a) = self.remote_row(ui, file) {
                                action = Some(a);
                            }
                        }
                        if hidden > 0 {
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(format!("{hidden} non-book file(s) hidden"))
                                    .small()
                                    .color(MUTED),
                            );
                        }
                    }
                }
            });

        match action {
            Some(RemoteAction::Download(f)) => self.start_download(ctx, f),
            Some(RemoteAction::Delete(f)) => self.confirm_delete = Some(f),
            None => {}
        }
    }

    /// One remote-file row. Returns a requested action, if any.
    fn remote_row(&self, ui: &mut egui::Ui, file: &Arc<RemoteFile>) -> Option<RemoteAction> {
        let mut action = None;
        let op = file.op();
        let (dot, busy) = match &op {
            FileOp::Idle => (GREY, false),
            FileOp::Downloading => (ACCENT, true),
            FileOp::Deleting => (RED, true),
            FileOp::Saved(_) => (GREEN, false),
            FileOp::Error(_) => (RED, false),
        };

        egui::Frame::group(ui.style())
            .fill(CARD)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    status_dot(ui, dot);
                    ui.add_space(2.0);
                    ui.label(RichText::new(&file.name).strong().color(TEXT));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if busy {
                            ui.spinner();
                        } else {
                            let del = egui::Button::new(RichText::new("Delete").color(RED))
                                .frame(false);
                            if ui.add(del).on_hover_text("Delete from Kindle").clicked() {
                                action = Some(RemoteAction::Delete(file.clone()));
                            }
                            if ui.button("Download").clicked() {
                                action = Some(RemoteAction::Download(file.clone()));
                            }
                        }
                    });
                });

                if matches!(op, FileOp::Downloading) {
                    ui.add(
                        egui::ProgressBar::new(file.progress())
                            .desired_height(7.0)
                            .corner_radius(CornerRadius::same(4))
                            .fill(ACCENT),
                    );
                }

                let (detail, color) = match &op {
                    FileOp::Error(e) => (e.clone(), RED),
                    FileOp::Saved(p) => (format!("Saved to {p}"), GREEN),
                    FileOp::Downloading => {
                        (format!("{} / {}", human(file.got()), human(file.size)), MUTED)
                    }
                    FileOp::Deleting => ("Deleting…".to_string(), MUTED),
                    FileOp::Idle => (human(file.size), MUTED),
                };
                ui.label(RichText::new(detail).small().color(color));
            });
        action
    }

    /// Modal confirming a destructive delete before it runs.
    fn delete_confirm_modal(&mut self, ctx: &egui::Context) {
        let Some(file) = self.confirm_delete.clone() else {
            return;
        };
        let mut open = true;
        egui::Window::new("Delete book")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_max_width(360.0);
                ui.label(RichText::new(format!("Delete “{}” from the Kindle?", file.name)).strong());
                ui.label(
                    RichText::new("This removes the book and its reading-data sidecar. It can't be undone.")
                        .small()
                        .color(MUTED),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        open = false;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let del = egui::Button::new(RichText::new("Delete").color(BG).strong())
                            .fill(RED);
                        if ui.add(del).clicked() {
                            self.start_delete(ctx, file.clone());
                            open = false;
                        }
                    });
                });
            });
        if !open {
            self.confirm_delete = None;
        }
    }
}

/// Centered muted hint used for empty/idle states.
fn hint(ui: &mut egui::Ui, text: &str) {
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(text).color(MUTED));
    });
}

/// Procedurally drawn app icon: an amber rounded square with a dark down-arrow.
fn app_icon() -> egui::IconData {
    let size = 64usize;
    let sz = size as f32;
    let r = 14.0f32;
    let amber = [0xE8u8, 0xA3, 0x3D, 0xFF];
    let dark = [0x14u8, 0x16, 0x1A, 0xFF];
    let mut rgba = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            // Rounded-square mask; outside stays transparent.
            let dx = (r - fx).max(fx - (sz - r)).max(0.0);
            let dy = (r - fy).max(fy - (sz - r)).max(0.0);
            if (dx * dx + dy * dy).sqrt() > r {
                continue;
            }
            let cx = 32.0;
            let in_shaft = fx >= 27.0 && fx < 37.0 && (14.0..38.0).contains(&fy);
            let in_head = (34.0..=52.0).contains(&fy) && (fx - cx).abs() <= (52.0 - fy) * (15.0 / 18.0);
            let c = if in_shaft || in_head { dark } else { amber };
            let i = (y * size + x) * 4;
            rgba[i..i + 4].copy_from_slice(&c);
        }
    }
    egui::IconData {
        rgba,
        width: size as u32,
        height: size as u32,
    }
}

/// Relaunch KOReader on the Kindle (blocking; call from a worker thread).
/// `busy` guards against overlapping runs; `msg` holds user-facing feedback.
fn open_koreader(
    ip: String,
    cmd: String,
    busy: Arc<AtomicBool>,
    msg: Arc<Mutex<String>>,
    ctx: egui::Context,
) {
    if busy.swap(true, Ordering::SeqCst) {
        return;
    }
    *msg.lock().unwrap() = "Restarting KOReader…".into();
    ctx.request_repaint();
    let text = match transfer::run_remote_cmd(&ip, "root", &cmd) {
        Ok(_) => "KOReader is restarting on the Kindle".into(),
        Err(e) => format!("Couldn't restart KOReader: {e}"),
    };
    *msg.lock().unwrap() = text;
    busy.store(false, Ordering::SeqCst);
    ctx.request_repaint();
}

/// A small filled status circle, painted (avoids missing-glyph boxes for `●`).
fn status_dot(ui: &mut egui::Ui, color: Color32) {
    let d = 10.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(d, d), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), d * 0.5, color);
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.ensure_watcher(&ctx);

        // Intake drag-and-drop.
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if !dropped.is_empty() {
            let paths: Vec<PathBuf> = dropped.into_iter().filter_map(|f| f.path).collect();
            self.add_paths(paths);
        }
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());

        // Keep animating while any transfer/connection/library op is in flight.
        if self.transferring.load(Ordering::Relaxed)
            || matches!(self.conn_state(), ConnState::Connecting)
            || self.library_busy()
        {
            ctx.request_repaint_after(Duration::from_millis(80));
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(BG).inner_margin(Margin::same(16)))
            .show(ui, |ui| {
                self.header(ui);
                ui.add_space(6.0);
                self.tab_bar(ui, &ctx);
                if self.show_settings {
                    self.settings_panel(ui);
                }
                ui.add_space(6.0);
                match self.tab {
                    Tab::Send => {
                        self.drop_zone(ui, hovering);
                        ui.add_space(4.0);
                        self.send_controls(ui, &ctx);
                        ui.add_space(6.0);
                        self.queue(ui);
                    }
                    Tab::Library => self.library_tab(ui, &ctx),
                }
            });

        self.delete_confirm_modal(&ctx);
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        self.cfg.save();
    }
}

/// Human-readable byte size.
fn human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut b = bytes as f64;
    let mut i = 0;
    while b >= 1024.0 && i < UNITS.len() - 1 {
        b /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{b:.1} {}", UNITS[i])
    }
}
