mod dns_server;

#[cfg(windows)]
mod service;

use anyhow::Result;
use clap::{Parser, Subcommand};
use dns_proxy_core::AppConfig;
use tracing_subscriber::EnvFilter;

use dns_server::{run_dns_proxy, RuntimeState};

#[derive(Debug, Parser)]
#[command(
    name = "dns-proxy-service",
    about = "DNS Proxy Windows service and console runner",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Run in the foreground (useful for debugging; still needs admin for :53).
    Console,
    /// Install as a Windows service (requires Administrator).
    Install,
    /// Uninstall the Windows service (requires Administrator).
    Uninstall,
    /// Start the installed Windows service.
    Start,
    /// Stop the installed Windows service.
    Stop,
    /// Print service status.
    Status,
    /// Write a default config.json if missing, then print its path.
    InitConfig,
}

fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

fn main() -> Result<()> {
    init_logging();
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Console) => run_console()?,
        Some(Commands::InitConfig) => {
            let cfg = AppConfig::load_or_default()?;
            let path = dns_proxy_core::config_path()?;
            println!("Config ready at {}", path.display());
            println!(
                "Upstream hosts: {}",
                cfg.upstream.hosts().join(", ")
            );
        }
        #[cfg(windows)]
        Some(Commands::Install) => service::install_service()?,
        #[cfg(windows)]
        Some(Commands::Uninstall) => service::uninstall_service()?,
        #[cfg(windows)]
        Some(Commands::Start) => service::start_service_cmd()?,
        #[cfg(windows)]
        Some(Commands::Stop) => service::stop_service_cmd()?,
        #[cfg(windows)]
        Some(Commands::Status) => service::service_status_cmd()?,
        #[cfg(not(windows))]
        Some(
            Commands::Install
            | Commands::Uninstall
            | Commands::Start
            | Commands::Stop
            | Commands::Status,
        ) => {
            anyhow::bail!("Windows service commands are only available on Windows");
        }
        None => {
            #[cfg(windows)]
            {
                // When launched by the Service Control Manager, there is no subcommand.
                if let Err(err) = service::run_service_dispatcher() {
                    // Fall back to console if not started by SCM (e.g. double-click).
                    tracing::warn!(error = %err, "service dispatcher unavailable; use `console`");
                    eprintln!("Tip: run `dns-proxy-service console` for foreground mode.");
                    eprintln!("     run `dns-proxy-service install` (as Admin) to register the service.");
                    std::process::exit(1);
                }
            }
            #[cfg(not(windows))]
            {
                run_console()?;
            }
        }
    }

    Ok(())
}

fn run_console() -> Result<()> {
    let config = AppConfig::load_or_default()?;
    let state = RuntimeState::from_config(config);
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(run_dns_proxy(state))
}
