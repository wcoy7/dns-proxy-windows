//! Thin wrappers around Windows service control for the UI buttons.

#[cfg(windows)]
mod win {
    use std::ffi::OsString;
    use std::time::Duration;

    use dns_proxy_core::{SERVICE_DESCRIPTION, SERVICE_DISPLAY_NAME, SERVICE_NAME};
    use windows_service::service::{
        ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType, ServiceState,
        ServiceType,
    };
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

    pub fn query_status_string() -> String {
        match try_query() {
            Ok(state) => format!("Service '{SERVICE_NAME}': {state:?}"),
            Err(_) => format!("Service '{SERVICE_NAME}' is not installed."),
        }
    }

    fn try_query() -> windows_service::Result<ServiceState> {
        let manager =
            ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
        let service = manager.open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS)?;
        Ok(service.query_status()?.current_state)
    }

    pub fn install() -> String {
        match try_install() {
            Ok(msg) => msg,
            Err(err) => format!("Install failed: {err} (run UI as Administrator)"),
        }
    }

    fn try_install() -> anyhow::Result<String> {
        let service_exe = locate_service_exe()?;
        let manager =
            ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CREATE_SERVICE)?;
        let info = ServiceInfo {
            name: OsString::from(SERVICE_NAME),
            display_name: OsString::from(SERVICE_DISPLAY_NAME),
            service_type: ServiceType::OWN_PROCESS,
            start_type: ServiceStartType::AutoStart,
            error_control: ServiceErrorControl::Normal,
            executable_path: service_exe.clone(),
            launch_arguments: vec![],
            dependencies: vec![],
            account_name: None,
            account_password: None,
        };
        let service = manager.create_service(&info, ServiceAccess::CHANGE_CONFIG)?;
        service.set_description(SERVICE_DESCRIPTION)?;
        Ok(format!(
            "Installed service using {}",
            service_exe.display()
        ))
    }

    fn locate_service_exe() -> anyhow::Result<std::path::PathBuf> {
        // Prefer sibling binary next to the UI, then target/release, then target/debug.
        let mut candidates = Vec::new();
        if let Ok(ui_exe) = std::env::current_exe() {
            if let Some(dir) = ui_exe.parent() {
                candidates.push(dir.join("dns-proxy-service.exe"));
            }
        }
        let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        candidates.push(manifest.join("../../target/release/dns-proxy-service.exe"));
        candidates.push(manifest.join("../../target/debug/dns-proxy-service.exe"));

        for c in candidates {
            if c.exists() {
                return Ok(dunce_canonicalize(c));
            }
        }
        anyhow::bail!(
            "could not find dns-proxy-service.exe — build the workspace first (`cargo build --release`)"
        )
    }

    fn dunce_canonicalize(path: std::path::PathBuf) -> std::path::PathBuf {
        std::fs::canonicalize(&path).unwrap_or(path)
    }

    pub fn uninstall() -> String {
        match try_uninstall() {
            Ok(()) => format!("Uninstalled service '{SERVICE_NAME}'."),
            Err(err) => format!("Uninstall failed: {err}"),
        }
    }

    fn try_uninstall() -> anyhow::Result<()> {
        let manager =
            ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
        let service = manager.open_service(
            SERVICE_NAME,
            ServiceAccess::STOP | ServiceAccess::DELETE | ServiceAccess::QUERY_STATUS,
        )?;
        if service.query_status()?.current_state != ServiceState::Stopped {
            let _ = service.stop();
            std::thread::sleep(Duration::from_secs(2));
        }
        service.delete()?;
        Ok(())
    }

    pub fn start() -> String {
        match try_start() {
            Ok(()) => format!("Started service '{SERVICE_NAME}'."),
            Err(err) => format!("Start failed: {err}"),
        }
    }

    fn try_start() -> anyhow::Result<()> {
        let manager =
            ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
        let service =
            manager.open_service(SERVICE_NAME, ServiceAccess::START | ServiceAccess::QUERY_STATUS)?;
        if service.query_status()?.current_state == ServiceState::Running {
            return Ok(());
        }
        service.start::<&str>(&[])?;
        Ok(())
    }

    pub fn stop() -> String {
        match try_stop() {
            Ok(()) => format!("Stopped service '{SERVICE_NAME}'."),
            Err(err) => format!("Stop failed: {err}"),
        }
    }

    fn try_stop() -> anyhow::Result<()> {
        let manager =
            ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
        let service =
            manager.open_service(SERVICE_NAME, ServiceAccess::STOP | ServiceAccess::QUERY_STATUS)?;
        if service.query_status()?.current_state == ServiceState::Stopped {
            return Ok(());
        }
        service.stop()?;
        Ok(())
    }
}

#[cfg(windows)]
pub use win::*;

#[cfg(not(windows))]
pub fn query_status_string() -> String {
    format!(
        "Service '{}' control is only available on Windows.",
        dns_proxy_core::SERVICE_NAME
    )
}

#[cfg(not(windows))]
pub fn install() -> String {
    "Windows only".into()
}

#[cfg(not(windows))]
pub fn uninstall() -> String {
    "Windows only".into()
}

#[cfg(not(windows))]
pub fn start() -> String {
    "Windows only".into()
}

#[cfg(not(windows))]
pub fn stop() -> String {
    "Windows only".into()
}
