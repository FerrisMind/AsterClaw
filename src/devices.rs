use crate::bus::{MessageBus, OutboundMessage};
use crate::constants;
use crate::state;
use async_trait::async_trait;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
#[derive(Debug, Clone, Copy)]
pub struct Config {
    pub enabled: bool,
    pub monitor_usb: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Add,
    Remove,
    Change,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Usb,
    Bluetooth,
    Pci,
    Generic,
}
#[derive(Debug, Clone)]
pub struct DeviceEvent {
    pub action: Action,
    pub kind: Kind,
    pub device_id: String,
    pub vendor: String,
    pub product: String,
    pub serial: String,
    pub capabilities: String,
    pub raw: HashMap<String, String>,
}
impl DeviceEvent {
    pub fn format_message(&self) -> String {
        let action_text = match self.action {
            Action::Add => "Connected",
            Action::Remove => "Disconnected",
            Action::Change => "Changed",
        };
        let mut msg = format!("🔌 Device {}\n\n", action_text);
        msg.push_str(&format!("Type: {}\n", kind_name(self.kind)));
        msg.push_str(&format!("Device: {} {}\n", self.vendor, self.product));
        if !self.device_id.is_empty() {
            msg.push_str(&format!("Device ID: {}\n", self.device_id));
        }
        if !self.capabilities.is_empty() {
            msg.push_str(&format!("Capabilities: {}\n", self.capabilities));
        }
        if !self.serial.is_empty() {
            msg.push_str(&format!("Serial: {}\n", self.serial));
        }
        if !self.raw.is_empty() {
            msg.push_str(&format!("Raw fields: {}\n", self.raw.len()));
        }
        msg
    }
}
fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Usb => "usb",
        Kind::Bluetooth => "bluetooth",
        Kind::Pci => "pci",
        Kind::Generic => "generic",
    }
}
#[async_trait]
pub trait EventSource: Send + Sync {
    fn kind(&self) -> Kind;
    async fn start(&self) -> anyhow::Result<mpsc::Receiver<DeviceEvent>>;
    async fn stop(&self) -> anyhow::Result<()>;
}
pub struct Service {
    bus: RwLock<Option<Arc<MessageBus>>>,
    state: state::Manager,
    sources: Vec<Arc<dyn EventSource>>,
    enabled: bool,
    handlers: Mutex<Vec<JoinHandle<()>>>,
}
impl Service {
    pub fn new(cfg: Config, workspace: std::path::PathBuf) -> Self {
        let _ = keep_enum_variants();
        let mut sources: Vec<Arc<dyn EventSource>> = Vec::new();
        if cfg.enabled && cfg.monitor_usb {
            sources.push(Arc::new(UsbMonitor::new()));
        }
        Self {
            bus: RwLock::new(None),
            state: state::Manager::new(workspace),
            sources,
            enabled: cfg.enabled,
            handlers: Mutex::new(Vec::new()),
        }
    }
    pub fn set_bus(&mut self, bus: Arc<MessageBus>) {
        *self.bus.write() = Some(bus);
    }
    pub async fn start(&self) -> anyhow::Result<()> {
        if !self.enabled || self.sources.is_empty() {
            tracing::info!("devices service disabled or no sources");
            return Ok(());
        }
        for src in &self.sources {
            let mut rx = match src.start().await {
                Ok(ch) => ch,
                Err(err) => {
                    tracing::error!("failed to start device source {:?}: {}", src.kind(), err);
                    continue;
                }
            };
            let bus = self.bus.read().clone();
            let state = self.state.clone();
            let handle = tokio::spawn(async move {
                while let Some(ev) = rx.recv().await {
                    send_notification(bus.as_ref(), &state, &ev).await;
                }
            });
            self.handlers.lock().push(handle);
        }
        tracing::info!("devices service started");
        Ok(())
    }
    pub fn stop(&self) {
        for src in &self.sources {
            let src = Arc::clone(src);
            tokio::spawn(async move {
                let _ = src.stop().await;
            });
        }
        let mut handles = self.handlers.lock();
        for h in handles.drain(..) {
            h.abort();
        }
        tracing::info!("devices service stopped");
    }
}
fn keep_enum_variants() -> usize {
    let actions = [Action::Add, Action::Remove, Action::Change];
    let kinds = [Kind::Usb, Kind::Bluetooth, Kind::Pci, Kind::Generic];
    actions.len() + kinds.len()
}
async fn send_notification(
    bus: Option<&Arc<MessageBus>>,
    state: &state::Manager,
    ev: &DeviceEvent,
) {
    let Some(bus) = bus else {
        return;
    };
    let last = state.get_last_channel();
    let Some((platform, user_id)) = state::parse_last_channel(&last) else {
        return;
    };
    if constants::is_internal_channel(platform) {
        return;
    }
    let _ = bus
        .publish_outbound(OutboundMessage {
            channel: platform.to_string(),
            chat_id: user_id.to_string(),
            content: ev.format_message(),
        })
        .await;
}
pub struct UsbMonitor {
    #[cfg(target_os = "linux")]
    child: Mutex<Option<std::process::Child>>,
}
impl UsbMonitor {
    pub fn new() -> Self {
        Self {
            #[cfg(target_os = "linux")]
            child: Mutex::new(None),
        }
    }
}
#[async_trait]
impl EventSource for UsbMonitor {
    fn kind(&self) -> Kind {
        Kind::Usb
    }
    async fn start(&self) -> anyhow::Result<mpsc::Receiver<DeviceEvent>> {
        #[cfg(not(target_os = "linux"))]
        {
            let (_tx, rx) = mpsc::channel(1);
            return Ok(rx);
        }
        #[cfg(target_os = "linux")]
        {
            use std::io::{BufRead, BufReader};
            use std::process::{Command, Stdio};
            let mut cmd = Command::new("udevadm");
            cmd.args(["monitor", "--property", "--subsystem-match=usb"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            let mut child = cmd.spawn()?;
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| anyhow::anyhow!("udevadm stdout not available"))?;
            *self.child.lock() = Some(child);
            let (tx, rx) = mpsc::channel(64);
            std::thread::spawn(move || {
                let mut scanner = BufReader::new(stdout);
                let mut line = String::new();
                let mut props: HashMap<String, String> = HashMap::new();
                let mut action = String::new();
                let mut is_udev = false;
                loop {
                    line.clear();
                    let read = scanner.read_line(&mut line).unwrap_or(0);
                    if read == 0 {
                        break;
                    }
                    let l = line.trim_end().to_string();
                    if l.is_empty() {
                        if is_udev
                            && (action == "add" || action == "remove")
                            && let Some(ev) = parse_usb_event(&action, &props)
                        {
                            let _ = tx.blocking_send(ev);
                        }
                        props.clear();
                        action.clear();
                        is_udev = false;
                        continue;
                    }
                    if !l.contains('=') {
                        is_udev = l.trim_start().starts_with("UDEV");
                        continue;
                    }
                    let mut split = l.splitn(2, '=');
                    let k = split.next().unwrap_or_default().to_string();
                    let v = split.next().unwrap_or_default().to_string();
                    if k == "ACTION" {
                        action = v.clone();
                    }
                    props.insert(k, v);
                }
            });
            Ok(rx)
        }
    }
    async fn stop(&self) -> anyhow::Result<()> {
        #[cfg(target_os = "linux")]
        {
            if let Some(mut child) = self.child.lock().take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        Ok(())
    }
}
#[cfg(target_os = "linux")]
fn parse_usb_event(action: &str, props: &HashMap<String, String>) -> Option<DeviceEvent> {
    let subsystem = props.get("SUBSYSTEM")?;
    if subsystem != "usb" {
        return None;
    }
    let dev_type = props.get("DEVTYPE").cloned().unwrap_or_default();
    if dev_type == "usb_interface" {
        return None;
    }
    if !dev_type.is_empty() && dev_type != "usb_device" {
        return None;
    }
    let action = match action {
        "add" => Action::Add,
        "remove" => Action::Remove,
        _ => return None,
    };
    let vendor = props
        .get("ID_VENDOR")
        .or_else(|| props.get("ID_VENDOR_ID"))
        .cloned()
        .unwrap_or_else(|| "Unknown Vendor".to_string());
    let product = props
        .get("ID_MODEL")
        .or_else(|| props.get("ID_MODEL_ID"))
        .cloned()
        .unwrap_or_else(|| "Unknown Device".to_string());
    let serial = props.get("ID_SERIAL_SHORT").cloned().unwrap_or_default();
    let mut device_id = props.get("DEVPATH").cloned().unwrap_or_default();
    if let (Some(busnum), Some(devnum)) = (props.get("BUSNUM"), props.get("DEVNUM")) {
        device_id = format!("{}:{}", busnum, devnum);
    }
    let capabilities = props
        .get("ID_USB_CLASS")
        .map(|class| usb_class_capability(class))
        .unwrap_or_else(|| "USB Device".to_string());
    Some(DeviceEvent {
        action,
        kind: Kind::Usb,
        device_id,
        vendor,
        product,
        serial,
        capabilities,
        raw: props.clone(),
    })
}
#[cfg(target_os = "linux")]
fn usb_class_capability(class: &str) -> String {
    match class.to_ascii_lowercase().as_str() {
        "00" => "Interface Definition (by interface)".to_string(),
        "01" => "Audio".to_string(),
        "02" => "CDC Communication (Network Card/Modem)".to_string(),
        "03" => "HID (Keyboard/Mouse/Gamepad)".to_string(),
        "05" => "Physical Interface".to_string(),
        "06" => "Image (Scanner/Camera)".to_string(),
        "07" => "Printer".to_string(),
        "08" => "Mass Storage (USB Flash Drive/Hard Disk)".to_string(),
        "09" => "USB Hub".to_string(),
        "0a" => "CDC Data".to_string(),
        "0b" => "Smart Card".to_string(),
        "0e" => "Video (Camera)".to_string(),
        "dc" => "Diagnostic Device".to_string(),
        "e0" => "Wireless Controller (Bluetooth)".to_string(),
        "ef" => "Miscellaneous".to_string(),
        "fe" => "Application Specific".to_string(),
        "ff" => "Vendor Specific".to_string(),
        _ => "USB Device".to_string(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn format_message_contains_core_fields() {
        let ev = DeviceEvent {
            action: Action::Add,
            kind: Kind::Usb,
            device_id: "1:2".to_string(),
            vendor: "ACME".to_string(),
            product: "Cam".to_string(),
            serial: "S1".to_string(),
            capabilities: "Video".to_string(),
            raw: HashMap::new(),
        };
        let msg = ev.format_message();
        assert!(msg.contains("Device Connected"));
        assert!(msg.contains("ACME Cam"));
        assert!(msg.contains("Capabilities: Video"));
    }
    #[test]
    fn parse_last_channel_works() {
        assert_eq!(
            state::parse_last_channel("telegram:123"),
            Some(("telegram", "123"))
        );
        assert_eq!(state::parse_last_channel(""), None);
        assert_eq!(state::parse_last_channel("telegram"), None);
    }
}
