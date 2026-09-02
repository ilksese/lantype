use mdns_sd::{ServiceDaemon, ServiceInfo};
use std::collections::HashMap;

const SERVICE_TYPE: &str = "_lantype._tcp.local.";

pub struct MdnsService {
    daemon: Option<ServiceDaemon>,
    device_name: String,
    port: u16,
    discovery_enabled: bool,
    addresses: Vec<String>,
    service_fullname: Option<String>,
}

impl MdnsService {
    pub fn new(device_name: String, port: u16) -> Self {
        Self {
            daemon: None,
            device_name,
            port,
            discovery_enabled: true,
            addresses: Vec::new(),
            service_fullname: None,
        }
    }

    pub fn start(&mut self, addresses: Vec<String>) -> Result<(), String> {
        self.addresses = addresses;
        if !self.discovery_enabled {
            return Ok(());
        }

        self.stop();
        let daemon = ServiceDaemon::new().map_err(|e| format!("mdns daemon: {e}"))?;

        let mut properties = HashMap::new();
        properties.insert("device".to_string(), self.device_name.clone());

        let service_info = ServiceInfo::new(
            SERVICE_TYPE,
            &self.device_name,
            &format!("{}.local.", self.device_name),
            self.addresses.as_slice(),
            self.port,
            properties,
        )
        .map_err(|e| format!("service info: {e}"))?;
        self.service_fullname = Some(service_info.get_fullname().to_string());

        daemon
            .register(service_info)
            .map_err(|e| format!("register: {e}"))?;

        self.daemon = Some(daemon);
        Ok(())
    }

    pub fn set_discovery_enabled(
        &mut self,
        enabled: bool,
        addresses: Vec<String>,
    ) -> Result<(), String> {
        self.discovery_enabled = enabled;
        self.addresses = addresses;
        self.stop();
        if enabled {
            self.start(self.addresses.clone())?;
        }
        Ok(())
    }

    pub fn stop(&mut self) {
        let fullname = self.service_fullname.take();
        if let Some(daemon) = self.daemon.take() {
            if let Some(fullname) = fullname {
                if let Ok(status) = daemon.unregister(&fullname) {
                    let _ = status.recv_timeout(std::time::Duration::from_secs(1));
                }
            }
            if let Ok(status) = daemon.shutdown() {
                let _ = status.recv_timeout(std::time::Duration::from_secs(1));
            }
        }
    }
}

impl Drop for MdnsService {
    fn drop(&mut self) {
        self.stop();
    }
}
