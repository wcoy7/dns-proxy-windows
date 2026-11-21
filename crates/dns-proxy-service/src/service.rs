//! Windows service entry points (install / uninstall / run under SCM).

#![cfg(windows)]

use std::ffi::OsString;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use dns_proxy_core::{
    AppConfig, SERVICE_DESCRIPTION, SERVICE_DISPLAY_NAME, SERVICE_NAME,
};
use tracing::{error, info};
use windows_service::service::{
    ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode,
    ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
use windows_service::{define_windows_service, service_dispatcher};

use crate::dns_server::{run_dns_proxy, RuntimeState};

const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;

define_windows_service!(ffi_service_main, service_main);

pub fn run_service_dispatcher() -> Result<()> {
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)
        .context("failed to start service dispatcher")?;
    Ok(())
}

fn service_main(_args: Vec<OsString>) {
    if let Err(err) = run_service() {
        error!(error = %err, "service failed");
    }
}

fn run_service() -> Result<()> {
    let (shutdown_tx, shutdown_rx) = mpsc::channel();

    let event_handler = move |control| -> ServiceControlHandlerResult {
        match control {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                let _ = shutdown_tx.send(());
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let status_handle = service_control_handler::register(SERVICE_NAME, event_handler)?;

    status_handle.set_service_status(ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    let config = AppConfig::load_or_default()?;
    let state = RuntimeState::from_config(config);
    let runtime = tokio::runtime::Runtime::new()?;

    runtime.block_on(async {
        tokio::select! {
            result = run_dns_proxy(state) => {
                if let Err(err) = result {
                    error!(error = %err, "DNS proxy exited with error");
                }
            }
            _ = async {
                // Poll the shutdown channel without blocking the async runtime hard.
                loop {
                    if shutdown_rx.try_recv().is_ok() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            } => {
                info!("service stop requested");
            }
        }
    });

    status_handle.set_service_status(ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    Ok(())
}

pub fn install_service() -> Result<()> {
    let exe = std::env::current_exe().context("current_exe")?;
    let manager =
        ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CREATE_SERVICE)?;

    let service_info = ServiceInfo {
        name: OsString::from(SERVICE_NAME),
        display_name: OsString::from(SERVICE_DISPLAY_NAME),
        service_type: SERVICE_TYPE,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: exe.clone(),
        launch_arguments: vec![],
        dependencies: vec![],
        account_name: None, // LocalSystem
        account_password: None,
    };

    let service = manager.create_service(&service_info, ServiceAccess::CHANGE_CONFIG)?;
    service.set_description(SERVICE_DESCRIPTION)?;
    info!(
        "installed Windows service '{}' -> {}",
        SERVICE_NAME,
        exe.display()
    );
    println!("Installed service '{SERVICE_NAME}'.");
    println!("Start it with:  sc start {SERVICE_NAME}");
    println!("Or from an elevated PowerShell: Start-Service {SERVICE_NAME}");
    Ok(())
}

pub fn uninstall_service() -> Result<()> {
    let manager =
        ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    let service = manager.open_service(
        SERVICE_NAME,
        ServiceAccess::STOP | ServiceAccess::DELETE | ServiceAccess::QUERY_STATUS,
    )?;

    let status = service.query_status()?;
    if status.current_state != ServiceState::Stopped {
        let _ = service.stop();
        // Give SCM a moment to settle.
        std::thread::sleep(Duration::from_secs(2));
    }

    service.delete()?;
    info!("uninstalled Windows service '{SERVICE_NAME}'");
    println!("Uninstalled service '{SERVICE_NAME}'.");
    Ok(())
}

pub fn start_service_cmd() -> Result<()> {
    let manager =
        ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    let service = manager.open_service(SERVICE_NAME, ServiceAccess::START | ServiceAccess::QUERY_STATUS)?;
    let status = service.query_status()?;
    if status.current_state == ServiceState::Running {
        println!("Service '{SERVICE_NAME}' is already running.");
        return Ok(());
    }
    service.start::<&str>(&[])?;
    println!("Started service '{SERVICE_NAME}'.");
    Ok(())
}

pub fn stop_service_cmd() -> Result<()> {
    let manager =
        ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    let service = manager.open_service(SERVICE_NAME, ServiceAccess::STOP | ServiceAccess::QUERY_STATUS)?;
    let status = service.query_status()?;
    if status.current_state == ServiceState::Stopped {
        println!("Service '{SERVICE_NAME}' is already stopped.");
        return Ok(());
    }
    service.stop()?;
    println!("Stopped service '{SERVICE_NAME}'.");
    Ok(())
}

pub fn service_status_cmd() -> Result<()> {
    let manager =
        ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    match manager.open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS) {
        Ok(service) => {
            let status = service.query_status()?;
            println!(
                "Service '{SERVICE_NAME}' state: {:?}",
                status.current_state
            );
            Ok(())
        }
        Err(_) => {
            bail!("Service '{SERVICE_NAME}' is not installed.");
        }
    }
}
