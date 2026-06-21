use mdns_sd::{ServiceDaemon, ServiceInfo};
use std::collections::HashMap;

const SERVICE_TYPE: &str = "_lantype._tcp.local.";

pub struct MdnsService {
    daemon: Option<ServiceDaemon>,
    device_name: String,
    port: u16,
}

impl MdnsService {
    pub fn new(device_name: String, port: u16) -> Self {
        Self {
            daemon: None,
            device_name,
            port,
        }
    }

    pub fn start(&mut self) -> Result<(), String> {
        let daemon = ServiceDaemon::new().map_err(|e| format!("mdns daemon: {e}"))?;

        let mut properties = HashMap::new();
        properties.insert("device".to_string(), self.device_name.clone());

        let service_info = ServiceInfo::new(
            SERVICE_TYPE,
            &self.device_name,
            &format!("{}.local.", self.device_name),
            "",
            self.port,
            properties,
        )
        .map_err(|e| format!("service info: {e}"))?;

        daemon
            .register(service_info)
            .map_err(|e| format!("register: {e}"))?;

        self.daemon = Some(daemon);
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(daemon) = self.daemon.take() {
            drop(daemon);
        }
    }
}

impl Drop for MdnsService {
    fn drop(&mut self) {
        self.stop();
    }
}