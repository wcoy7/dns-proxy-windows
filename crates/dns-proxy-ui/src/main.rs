mod service_ctl;

use std::path::PathBuf;

use anyhow::Result;
use dns_proxy_core::{
    AppConfig, BlockMode, UpstreamChoice, UpstreamPreset, APP_NAME, UPSTREAM_PRESETS,
};
use eframe::egui;
use tracing_subscriber::EnvFilter;

fn main() -> eframe::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    let icon = load_icon();
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([720.0, 640.0])
        .with_min_inner_size([560.0, 480.0])
        .with_title(APP_NAME);
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
        Box::new(|cc| Ok(Box::new(DnsProxyApp::new(cc)))),
    )
}

fn load_icon() -> Option<egui::IconData> {
    // Prefer assets next to the workspace / binary.
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
    custom_servers_text: String,
    custom_blocks_text: String,
    status: String,
    service_status: String,
    selected_preset: UpstreamPreset,
    use_custom_upstream: bool,
}

impl DnsProxyApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let config = AppConfig::load_or_default().unwrap_or_default();
        let (use_custom_upstream, selected_preset, custom_servers_text) = match &config.upstream {
            UpstreamChoice::Preset { preset } => (false, *preset, String::new()),
            UpstreamChoice::Custom { servers } => {
                (true, UpstreamPreset::Cloudflare, servers.join("\n"))
            }
        };
        let custom_blocks_text = config.block.custom.join("\n");
        let service_status = service_ctl::query_status_string();
        Self {
            config,
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
        }
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
                self.status = "Configuration saved. Restart or reload the service to apply."
                    .into();
            }
            Err(err) => self.status = format!("Save failed: {err}"),
        }
    }

    fn refresh_service_status(&mut self) {
        self.service_status = service_ctl::query_status_string();
    }
}

impl eframe::App for DnsProxyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(APP_NAME);
            ui.label("Local DNS proxy with selectable upstream resolvers and domain blocking.");
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
                        self.status = service_ctl::uninstall();
                        self.refresh_service_status();
                    }
                    if ui.button("Start").clicked() {
                        self.status = service_ctl::start();
                        self.refresh_service_status();
                    }
                    if ui.button("Stop").clicked() {
                        self.status = service_ctl::stop();
                        self.refresh_service_status();
                    }
                });
                ui.small("Install / uninstall / bind to port 53 require an elevated (Administrator) process.");
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
    }
}
