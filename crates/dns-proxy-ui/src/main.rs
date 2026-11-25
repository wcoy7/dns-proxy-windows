#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod dns_adapter;
mod service_ctl;
mod tray;
mod ui_settings;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use dns_proxy_core::{
    AppConfig, BlockMode, UpstreamChoice, UpstreamPreset, APP_NAME, UPSTREAM_PRESETS,
};
use eframe::egui;
use tracing_subscriber::EnvFilter;
use tray_icon::menu::MenuEvent;

use crate::dns_adapter::{AdapterDnsInfo, DnsBackup};
use crate::tray::{TrayHandles, WindowState};
use crate::ui_settings::UiSettings;

fn main() -> eframe::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    let icon = load_icon();
    let ui_settings = UiSettings::load_or_default();
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([740.0, 780.0])
        .with_min_inner_size([560.0, 520.0])
        .with_title(APP_NAME)
        // Keep the taskbar entry while visible; close is intercepted to hide-to-tray.
        .with_close_button(true);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |cc| Ok(Box::new(DnsProxyApp::new(cc, ui_settings)))),
    )
}

fn load_icon() -> Option<egui::IconData> {
    let candidates = [
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/dns-proxy-icon.png"),
        PathBuf::from("assets/dns-proxy-icon.png"),
    ];
    for path in candidates {
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(img) = image::load_from_memory(&bytes) {
                let rgba = img.into_rgba8();
                let (w, h) = rgba.dimensions();
                return Some(egui::IconData {
                    rgba: rgba.into_raw(),
                    width: w,
                    height: h,
                });
            }
        }
    }
    None
}

struct DnsProxyApp {
    config: AppConfig,
    ui_settings: UiSettings,
    custom_servers_text: String,
    custom_blocks_text: String,
    status: String,
    service_status: String,
    selected_preset: UpstreamPreset,
    use_custom_upstream: bool,
    window: WindowState,
    tray: Option<TrayHandles>,
    adapters: Vec<AdapterDnsInfo>,
    dns_status: String,
    dns_error: String,
    use_dns_proxy: bool,
    quit_requested: bool,
    last_adapter_refresh: Instant,
    hide_on_startup_pending: bool,
}

impl DnsProxyApp {
    fn new(cc: &eframe::CreationContext<'_>, ui_settings: UiSettings) -> Self {
        let config = AppConfig::load_or_default().unwrap_or_default();
        let (use_custom_upstream, selected_preset, custom_servers_text) = match &config.upstream {
            UpstreamChoice::Preset { preset } => (false, *preset, String::new()),
            UpstreamChoice::Custom { servers } => {
                (true, UpstreamPreset::Cloudflare, servers.join("\n"))
            }
        };
        let custom_blocks_text = config.block.custom.join("\n");
        let service_status = service_ctl::query_status_string();

        let window = WindowState::new();
        window.set_hwnd_from_cc(cc);

        let tray = match tray::build_tray(&window, cc.egui_ctx.clone()) {
            Ok(t) => Some(t),
            Err(err) => {
                tracing::error!("tray icon failed: {err}");
                None
            }
        };

        let adapters = dns_adapter::list_adapters().unwrap_or_default();
        let use_dns_proxy = DnsBackup::is_active()
            || adapters.iter().any(|a| a.operational && a.points_at_proxy());

        let hide_on_startup_pending = ui_settings.start_minimized_to_tray;

        let mut app = Self {
            config,
            ui_settings,
            custom_servers_text,
            custom_blocks_text,
            status: format!(
                "Config: {}",
                dns_proxy_core::config_path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| "(unknown)".into())
            ),
            service_status,
            selected_preset,
            use_custom_upstream,
            window,
            tray,
            adapters,
            dns_status: String::new(),
            dns_error: String::new(),
            use_dns_proxy,
            quit_requested: false,
            last_adapter_refresh: Instant::now(),
            hide_on_startup_pending,
        };
        app.refresh_dns_status_text();
        app
    }

    fn apply_ui_to_config(&mut self) -> Result<()> {
        if self.use_custom_upstream {
            let servers: Vec<String> = self
                .custom_servers_text
                .lines()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
            self.config.upstream = UpstreamChoice::Custom { servers };
        } else {
            self.config.upstream = UpstreamChoice::Preset {
                preset: self.selected_preset,
            };
        }
        self.config.block.custom = self
            .custom_blocks_text
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        self.config.validate()?;
        Ok(())
    }

    fn save_config(&mut self) {
        match self.apply_ui_to_config().and_then(|_| self.config.save()) {
            Ok(()) => {
                // Nice-to-have: restart service so changes apply without a full reinstall.
                if service_ctl::is_running() {
                    let _ = service_ctl::stop();
                    std::thread::sleep(Duration::from_millis(800));
                    self.status = format!(
                        "Configuration saved. {}",
                        service_ctl::start()
                    );
                } else {
                    self.status =
                        "Configuration saved. Start the service to apply.".into();
                }
                self.refresh_service_status();
            }
            Err(err) => self.status = format!("Save failed: {err}"),
        }
    }

    fn save_ui_settings(&mut self) {
        if let Err(err) = self.ui_settings.save() {
            self.status = format!("Failed to save UI settings: {err}");
        }
    }

    fn refresh_service_status(&mut self) {
        self.service_status = service_ctl::query_status_string();
    }

    fn refresh_adapters(&mut self) {
        // Do NOT clear dns_error here — enable/disable failures were previously wiped
        // by a successful adapter scan (and by the 8s periodic refresh), which made
        // "Use DNS Proxy" look like a silent no-op.
        match dns_adapter::list_adapters() {
            Ok(list) => {
                self.adapters = list;
            }
            Err(err) => {
                self.dns_error = format!("Adapter scan failed: {err}");
            }
        }
        self.use_dns_proxy = DnsBackup::is_active()
            || self
                .adapters
                .iter()
                .any(|a| a.operational && a.points_at_proxy());
        self.refresh_dns_status_text();
        self.last_adapter_refresh = Instant::now();
    }

    /// If adapters still point at 127.0.0.1 but the proxy is dead, restore DNS.
    fn watchdog_proxy_or_restore(&mut self) {
        let using_proxy = DnsBackup::is_active()
            || self
                .adapters
                .iter()
                .any(|a| a.operational && a.points_at_proxy());
        if !using_proxy {
            return;
        }
        if dns_adapter::local_proxy_healthy() {
            return;
        }
        tracing::error!("local proxy not answering while Use DNS Proxy is active — restoring DNS");
        match dns_adapter::restore_proxy_dns() {
            Ok(msg) => {
                self.dns_error = format!(
                    "Proxy stopped answering on 127.0.0.1:53 — DNS was restored automatically. {msg}"
                );
                self.status = self.dns_error.clone();
                self.use_dns_proxy = false;
            }
            Err(err) => {
                self.dns_error = format!(
                    "Proxy is down on 127.0.0.1:53 and automatic restore failed: {err}. \
                     Run elevated: .\\scripts\\restore-system-dns.ps1"
                );
                self.status = self.dns_error.clone();
            }
        }
        self.refresh_adapters();
    }

    fn refresh_dns_status_text(&mut self) {
        if self.adapters.is_empty() {
            self.dns_status = "No adapters detected.".into();
            return;
        }
        let lines: Vec<String> = self.adapters.iter().map(|a| a.summary()).collect();
        let elev = if dns_adapter::is_elevated() {
            "Elevated (Administrator)"
        } else {
            "Not elevated — DNS changes will fail until you Run as administrator"
        };
        let backup = if DnsBackup::is_active() {
            "Backup: present (restore available)"
        } else {
            "Backup: none"
        };
        self.dns_status = format!("{}\n{elev}\n{backup}", lines.join("\n"));
    }

    fn enable_dns_proxy(&mut self) {
        if !service_ctl::is_running() {
            self.dns_error =
                "Proxy service is not running. Start it before switching adapter DNS \
                 (to avoid breaking name resolution)."
                    .into();
            self.use_dns_proxy = false;
            return;
        }
        if !dns_adapter::local_proxy_healthy() {
            self.dns_error =
                "DnsProxy service is marked Running but 127.0.0.1:53 is not answering. \
                 Stop/Start the service, or run `dns-proxy-service console` and \
                 .\\scripts\\smoke-dns-proxy.ps1 before enabling Use DNS Proxy."
                    .into();
            self.use_dns_proxy = false;
            return;
        }
        match dns_adapter::enable_proxy_dns() {
            Ok(msg) => {
                self.status = msg;
                self.dns_error.clear();
                self.use_dns_proxy = true;
            }
            Err(err) => {
                self.dns_error = err.to_string();
                self.status = format!("DNS switch failed: {err}");
                self.use_dns_proxy = false;
            }
        }
        self.refresh_adapters();
    }

    fn disable_dns_proxy(&mut self) {
        match dns_adapter::restore_proxy_dns() {
            Ok(msg) => {
                self.status = msg;
                self.dns_error.clear();
                self.use_dns_proxy = false;
            }
            Err(err) => {
                // If there was nothing to restore but adapters already point elsewhere, clear flag.
                self.dns_error = err.to_string();
                self.status = format!("DNS restore: {err}");
            }
        }
        self.refresh_adapters();
    }

    fn request_quit(&mut self) {
        if self.ui_settings.restore_dns_on_quit && DnsBackup::is_active() {
            match dns_adapter::restore_proxy_dns() {
                Ok(msg) => self.status = msg,
                Err(err) => {
                    tracing::warn!("DNS restore on quit failed: {err}");
                    self.status = format!("Quit: DNS restore failed: {err}");
                }
            }
        }
        self.quit_requested = true;
    }

    fn handle_tray_menu(&mut self) {
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let id = event.id.as_ref();
            if id == tray::MENU_OPEN {
                self.window.show();
            } else if id == tray::MENU_START {
                self.status = service_ctl::start();
                self.refresh_service_status();
            } else if id == tray::MENU_STOP {
                // Restoring DNS before stop avoids a soft brick if we had switched.
                if DnsBackup::is_active() {
                    match dns_adapter::restore_proxy_dns() {
                        Ok(msg) => self.status = format!("{msg} — then stopping service."),
                        Err(err) => {
                            self.status = format!("DNS restore before stop failed: {err}");
                        }
                    }
                    self.refresh_adapters();
                }
                self.status = format!("{} {}", self.status, service_ctl::stop());
                self.refresh_service_status();
            } else if id == tray::MENU_QUIT {
                self.request_quit();
            }
        }
    }
}

impl eframe::App for DnsProxyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.hide_on_startup_pending {
            self.hide_on_startup_pending = false;
            self.window.hide();
        }

        self.handle_tray_menu();

        // Close button → hide to tray (unless quitting).
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.quit_requested {
                // Allow the close to proceed.
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.window.hide();
            }
        }

        if self.quit_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // Watchdog must run even when minimized to tray — otherwise a dead proxy
        // leaves adapters on 127.0.0.1 with no UI refresh to restore DNS.
        let watchdog_every = if self.window.is_visible() {
            Duration::from_secs(5)
        } else {
            Duration::from_secs(8)
        };
        if self.last_adapter_refresh.elapsed() > watchdog_every {
            if self.window.is_visible() {
                self.refresh_adapters();
                self.refresh_service_status();
            }
            self.watchdog_proxy_or_restore();
            self.last_adapter_refresh = Instant::now();
        }

        // Keep receiving tray events / watchdog ticks while hidden.
        if !self.window.is_visible() {
            ctx.request_repaint_after(Duration::from_secs(2));
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading(APP_NAME);
                ui.label(
                    "Local DNS proxy with selectable upstream resolvers and domain blocking.",
                );
                ui.add_space(8.0);

                ui.group(|ui| {
                    ui.heading("Upstream DNS");
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.use_custom_upstream, "Use custom upstreams");
                    });
                    if self.use_custom_upstream {
                        ui.label("One IPv4/IPv6 address per line:");
                        ui.add(
                            egui::TextEdit::multiline(&mut self.custom_servers_text)
                                .desired_rows(3)
                                .desired_width(f32::INFINITY),
                        );
                    } else {
                        ui.horizontal(|ui| {
                            for preset in UPSTREAM_PRESETS {
                                let selected = self.selected_preset == preset;
                                if ui.selectable_label(selected, preset.label()).clicked() {
                                    self.selected_preset = preset;
                                }
                            }
                        });
                        ui.monospace(format!(
                            "{} / {}",
                            self.selected_preset.primary(),
                            self.selected_preset.secondary()
                        ));
                    }
                });

                ui.add_space(8.0);

                ui.group(|ui| {
                    ui.heading("Blocking");
                    ui.checkbox(&mut self.config.block.youtube, "YouTube");
                    ui.checkbox(&mut self.config.block.roblox, "Roblox");
                    ui.checkbox(&mut self.config.block.social, "Social media");
                    ui.horizontal(|ui| {
                        ui.label("Mode:");
                        ui.selectable_value(
                            &mut self.config.block.mode,
                            BlockMode::NxDomain,
                            "NXDOMAIN",
                        );
                        ui.selectable_value(
                            &mut self.config.block.mode,
                            BlockMode::Sinkhole,
                            "Sinkhole",
                        );
                    });
                    if self.config.block.mode == BlockMode::Sinkhole {
                        ui.horizontal(|ui| {
                            ui.label("Sinkhole IP:");
                            ui.text_edit_singleline(&mut self.config.block.sinkhole_ip);
                        });
                    }
                    ui.label("Custom domains (one per line):");
                    ui.add(
                        egui::TextEdit::multiline(&mut self.custom_blocks_text)
                            .desired_rows(5)
                            .desired_width(f32::INFINITY),
                    );
                });

                ui.add_space(8.0);

                ui.group(|ui| {
                    ui.heading("Listen & logging");
                    ui.horizontal(|ui| {
                        ui.label("Listen:");
                        ui.text_edit_singleline(&mut self.config.listen);
                    });
                    ui.checkbox(&mut self.config.verbose_logging, "Verbose query logging");
                });

                ui.add_space(8.0);

                ui.group(|ui| {
                    ui.heading("Network DNS");
                    ui.label("Point active adapters at this machine's proxy (127.0.0.1).");

                    let elevated = dns_adapter::is_elevated();
                    let service_running = service_ctl::is_running();
                    let proxy_ok = dns_adapter::local_proxy_healthy();
                    if !elevated {
                        ui.colored_label(
                            egui::Color32::from_rgb(180, 120, 40),
                            "Not elevated — right-click dns-proxy-ui.exe → Run as administrator, then try again.",
                        );
                    }
                    if !service_running {
                        ui.colored_label(
                            egui::Color32::from_rgb(180, 120, 40),
                            "Proxy service is not running — Start it below before enabling Use DNS Proxy.",
                        );
                    } else if !proxy_ok {
                        ui.colored_label(
                            egui::Color32::from_rgb(200, 80, 60),
                            "Service Running but 127.0.0.1:53 not answering — do not enable Use DNS Proxy.",
                        );
                    }

                    let mut want = self.use_dns_proxy;
                    let response = ui.checkbox(
                        &mut want,
                        "Use DNS Proxy (set preferred DNS to 127.0.0.1)",
                    );
                    if response.changed() {
                        if want {
                            self.enable_dns_proxy();
                        } else {
                            self.disable_dns_proxy();
                        }
                    }
                    if response.hovered() {
                        let tip = if !elevated {
                            "Requires Administrator elevation."
                        } else if !service_running && !self.use_dns_proxy {
                            "Start the DNS Proxy Windows service first."
                        } else {
                            "When enabled, active adapters use 127.0.0.1. Disable restores the previous DNS."
                        };
                        response.on_hover_text(tip);
                    }

                    ui.add_space(4.0);
                    ui.monospace(&self.dns_status);
                    if !self.dns_error.is_empty() {
                        ui.colored_label(egui::Color32::from_rgb(200, 80, 60), &self.dns_error);
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Refresh adapters").clicked() {
                            self.refresh_adapters();
                        }
                        if ui.button("Restore previous DNS").clicked() {
                            self.disable_dns_proxy();
                        }
                        if !self.dns_error.is_empty() && ui.button("Clear error").clicked() {
                            self.dns_error.clear();
                        }
                    });
                    ui.small(
                        "Requires Administrator + a healthy proxy on 127.0.0.1:53. \
                         Previous DNS is saved under %ProgramData%\\DNS Proxy\\dns-backup.json. \
                         Emergency: .\\scripts\\restore-system-dns.ps1",
                    );
                });

                ui.add_space(8.0);

                ui.group(|ui| {
                    ui.heading("Windows service");
                    ui.label(&self.service_status);
                    ui.horizontal(|ui| {
                        if ui.button("Refresh status").clicked() {
                            self.refresh_service_status();
                        }
                        if ui.button("Install").clicked() {
                            self.status = service_ctl::install();
                            self.refresh_service_status();
                        }
                        if ui.button("Uninstall").clicked() {
                            if DnsBackup::is_active() {
                                let _ = dns_adapter::restore_proxy_dns();
                                self.refresh_adapters();
                            }
                            self.status = service_ctl::uninstall();
                            self.refresh_service_status();
                        }
                        if ui.button("Start").clicked() {
                            self.status = service_ctl::start();
                            self.refresh_service_status();
                        }
                        if ui.button("Stop").clicked() {
                            if DnsBackup::is_active() {
                                match dns_adapter::restore_proxy_dns() {
                                    Ok(msg) => self.status = msg,
                                    Err(err) => {
                                        self.status = format!("DNS restore before stop: {err}")
                                    }
                                }
                                self.refresh_adapters();
                            }
                            self.status = format!("{} {}", self.status, service_ctl::stop());
                            self.refresh_service_status();
                        }
                    });
                    ui.small(
                        "Install / uninstall / bind to port 53 require an elevated (Administrator) process.",
                    );
                });

                ui.add_space(8.0);

                ui.group(|ui| {
                    ui.heading("Tray & window");
                    if ui
                        .checkbox(
                            &mut self.ui_settings.start_minimized_to_tray,
                            "Start minimized to tray",
                        )
                        .changed()
                    {
                        self.save_ui_settings();
                    }
                    if ui
                        .checkbox(
                            &mut self.ui_settings.restore_dns_on_quit,
                            "Restore DNS on Quit",
                        )
                        .changed()
                    {
                        self.save_ui_settings();
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Hide to tray").clicked() {
                            self.window.hide();
                        }
                        if ui.button("Quit").clicked() {
                            self.request_quit();
                        }
                    });
                    ui.small(
                        "Closing the window hides to the tray. Use Quit (tray menu or button) to exit.",
                    );
                    if self.tray.is_none() {
                        ui.colored_label(
                            egui::Color32::from_rgb(200, 80, 60),
                            "Tray icon failed to create — check logs.",
                        );
                    }
                });

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Save configuration").clicked() {
                        self.save_config();
                    }
                    if ui.button("Reload from disk").clicked() {
                        match AppConfig::load_or_default() {
                            Ok(cfg) => {
                                self.config = cfg;
                                match &self.config.upstream {
                                    UpstreamChoice::Preset { preset } => {
                                        self.use_custom_upstream = false;
                                        self.selected_preset = *preset;
                                    }
                                    UpstreamChoice::Custom { servers } => {
                                        self.use_custom_upstream = true;
                                        self.custom_servers_text = servers.join("\n");
                                    }
                                }
                                self.custom_blocks_text = self.config.block.custom.join("\n");
                                self.status = "Reloaded configuration from disk.".into();
                            }
                            Err(err) => self.status = format!("Reload failed: {err}"),
                        }
                    }
                });

                ui.add_space(6.0);
                ui.separator();
                ui.label(&self.status);
            });
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.ui_settings.restore_dns_on_quit && DnsBackup::is_active() {
            if let Err(err) = dns_adapter::restore_proxy_dns() {
                tracing::warn!("DNS restore on exit failed: {err}");
            }
        }
    }
}
