use anyhow::Result;
use if_addrs::{IfAddr, get_if_addrs};
use mdns_sd::{DaemonEvent, ServiceDaemon, ServiceInfo};
use std::net::IpAddr;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tokio::sync::RwLock;

use super::{LogCallback, log};

pub struct ServiceDiscovery {
    daemon: ServiceDaemon,
    service_fullname: Arc<RwLock<Option<String>>>,
    is_advertising: Arc<RwLock<bool>>,
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
            is_advertising: Arc::new(RwLock::new(false)),
            log_callback,
        })
    }

    pub async fn start_advertising(&self, port: u16) -> Result<()> {
        if *self.is_advertising.read().await {
            return Ok(());
        }

        let mut hostname = hostname::get()
            .map(|h| h.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "localhost".to_owned());
        if !hostname.ends_with(".local.") {
            hostname = format!("{}.local.", hostname.trim_end_matches('.'));
        }
        log(
            &self.log_callback,
            "ServiceDiscovery",
            format!("mDNS hostname: {}", hostname),
        );

        let service_type = "_http._tcp.local.";
        let instance_name = "TFCStreamServer";

        let mut ips = Vec::new();
        if let Ok(ifaces) = get_if_addrs() {
            for iface in ifaces {
                let ip: IpAddr = match iface.addr {
                    IfAddr::V4(addr) => IpAddr::V4(addr.ip),
                    IfAddr::V6(addr) => IpAddr::V6(addr.ip),
                };
                if !ip.is_loopback() && !ip.is_unspecified() {
                    ips.push(ip);
                }
            }
        }
        if ips.is_empty() {
            log(
                &self.log_callback,
                "ServiceDiscovery",
                "mDNS: No usable IPs found for service",
            );
        } else {
            log(
                &self.log_callback,
                "ServiceDiscovery",
                format!("mDNS IPs: {:?}", ips),
            );
        }

        let service_info = ServiceInfo::new(
            service_type,
            instance_name,
            &hostname,
            ips.as_slice(),
            port,
            None,
        )?
        .enable_addr_auto();

        let fullname = service_info.get_fullname().to_string();

        self.daemon.register(service_info)?;
        log(
            &self.log_callback,
            "ServiceDiscovery",
            format!("mDNS registered: {}", fullname),
        );

        {
            let mut name = self.service_fullname.write().await;
            *name = Some(fullname);
        }
        {
            let mut advertising = self.is_advertising.write().await;
            *advertising = true;
        }

        log(
            &self.log_callback,
            "ServiceDiscovery",
            format!("mDNS: Started advertising TFCStream on port {}", port),
        );
        Ok(())
    }

    pub async fn stop_advertising(&self) -> Result<()> {
        if !*self.is_advertising.read().await {
            return Ok(());
        }

        if let Some(fullname) = self.service_fullname.read().await.clone()
            && let Ok(receiver) = self.daemon.unregister(&fullname)
        {
            let _ = receiver.recv_timeout(Duration::from_secs(1));
        }

        {
            let mut name = self.service_fullname.write().await;
            *name = None;
        }
        {
            let mut advertising = self.is_advertising.write().await;
            *advertising = false;
        }

        log(
            &self.log_callback,
            "ServiceDiscovery",
            "mDNS: Stopped advertising",
        );
        Ok(())
    }
}
