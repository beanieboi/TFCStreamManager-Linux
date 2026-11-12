use anyhow::Result;
use if_addrs::{IfAddr, get_if_addrs};
use mdns_sd::{DaemonEvent, ServiceDaemon, ServiceInfo};
use std::net::IpAddr;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tokio::sync::RwLock;

use super::{LogCallback, log};

const SERVICE_TYPE: &str = "_http._tcp.local.";
const INSTANCE_NAME: &str = "TFCStreamServer";
/// TXT key the tablet reads to tell a started app from a running server.
pub const TXT_STATE_KEY: &str = "state";

/// What the tablet sees: the app is open (`started`), or the web server is
/// accepting requests (`running`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerState {
    Started,
    Running,
}

impl ServerState {
    pub fn as_str(self) -> &'static str {
        match self {
            ServerState::Started => "started",
            ServerState::Running => "running",
        }
    }
}

fn usable_ips() -> Vec<IpAddr> {
    get_if_addrs()
        .map(|ifaces| {
            ifaces
                .into_iter()
                .map(|iface| match iface.addr {
                    IfAddr::V4(addr) => IpAddr::V4(addr.ip),
                    IfAddr::V6(addr) => IpAddr::V6(addr.ip),
                })
                .filter(|ip| !ip.is_loopback() && !ip.is_unspecified())
                .collect()
        })
        .unwrap_or_default()
}

pub struct ServiceDiscovery {
    daemon: ServiceDaemon,
    service_fullname: Arc<RwLock<Option<String>>>,
    log_callback: LogCallback,
}

impl ServiceDiscovery {
    pub fn new(log_callback: LogCallback) -> Result<Self> {
        let daemon = ServiceDaemon::new()?;
        if let Ok(receiver) = daemon.monitor() {
            let log_callback = Arc::clone(&log_callback);
            thread::spawn(move || {
                while let Ok(event) = receiver.recv() {
                    match event {
                        DaemonEvent::Announce(service, instance) => {
                            log(
                                &log_callback,
                                "ServiceDiscovery",
                                format!("mDNS announce: {} ({})", instance, service),
                            );
                        }
                        DaemonEvent::Error(err) => {
                            log(
                                &log_callback,
                                "ServiceDiscovery",
                                format!("mDNS error: {}", err),
                            );
                        }
                        DaemonEvent::IpAdd(ip) => {
                            log(
                                &log_callback,
                                "ServiceDiscovery",
                                format!("mDNS IP add: {}", ip),
                            );
                        }
                        DaemonEvent::IpDel(ip) => {
                            log(
                                &log_callback,
                                "ServiceDiscovery",
                                format!("mDNS IP del: {}", ip),
                            );
                        }
                        _ => {}
                    }
                }
            });
        }
        Ok(Self {
            daemon,
            service_fullname: Arc::new(RwLock::new(None)),
            log_callback,
        })
    }

    /// Registers the service, or re-announces it with a new state. mdns-sd
    /// replaces a registration with the same fullname, so no unregister is
    /// needed between states.
    pub async fn advertise(&self, port: u16, state: ServerState) -> Result<()> {
        let mut hostname = hostname::get()
            .map(|h| h.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "localhost".to_owned());
        if !hostname.ends_with(".local.") {
            hostname = format!("{}.local.", hostname.trim_end_matches('.'));
        }

        let ips = usable_ips();
        if ips.is_empty() {
            log(
                &self.log_callback,
                "ServiceDiscovery",
                "mDNS: No usable IPs found for service",
            );
        }

        let properties = [(TXT_STATE_KEY, state.as_str())];
        let service_info = ServiceInfo::new(
            SERVICE_TYPE,
            INSTANCE_NAME,
            &hostname,
            ips.as_slice(),
            port,
            &properties[..],
        )?
        .enable_addr_auto();

        let fullname = service_info.get_fullname().to_string();
        self.daemon.register(service_info)?;
        *self.service_fullname.write().await = Some(fullname.clone());

        log(
            &self.log_callback,
            "ServiceDiscovery",
            format!(
                "mDNS: advertising {} on {}:{} as {}",
                fullname,
                hostname,
                port,
                state.as_str()
            ),
        );
        Ok(())
    }

    pub async fn stop_advertising(&self) -> Result<()> {
        let Some(fullname) = self.service_fullname.write().await.take() else {
            return Ok(());
        };
        if let Ok(receiver) = self.daemon.unregister(&fullname) {
            let _ = receiver.recv_timeout(Duration::from_secs(1));
        }
        log(
            &self.log_callback,
            "ServiceDiscovery",
            "mDNS: Stopped advertising",
        );
        Ok(())
    }
}
