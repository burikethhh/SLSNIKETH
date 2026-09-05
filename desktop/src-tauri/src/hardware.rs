use parking_lot::Mutex;
use serialport::{SerialPort, SerialPortType};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// A button-press event pushed by the ESP32 firmware (`EVT:ENTRY_BTN` /
/// `EVT:EXIT_BTN`, pins.jfif field map). The webview polls these and arms the
/// matching camera + auto face scan; the tailgate path stays fully automatic.
#[derive(Debug, Clone, serde::Serialize)]
pub struct HardwareButtonEvent {
    pub kind: String,
    pub timestamp_ms: u64,
}

#[derive(Clone)]
pub struct HardwareManager {
    port: Arc<Mutex<Option<Box<dyn SerialPort>>>>,
    connected_port_name: Arc<Mutex<Option<String>>>,
    button_events: Arc<Mutex<Vec<HardwareButtonEvent>>>,
    reader_running: Arc<AtomicBool>,
}

impl HardwareManager {
    pub fn new() -> Self {
        Self {
            port: Arc::new(Mutex::new(None)),
            connected_port_name: Arc::new(Mutex::new(None)),
            button_events: Arc::new(Mutex::new(Vec::new())),
            reader_running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn list_available_ports() -> Vec<String> {
        serialport::available_ports()
            .unwrap_or_default()
            .into_iter()
            .map(|p| match p.port_type {
                SerialPortType::UsbPort(info) => {
                    format!("{} (USB: {:04x}:{:04x})", p.port_name, info.vid, info.pid)
                }
                _ => p.port_name,
            })
            .collect()
    }

    pub fn connect(&self, port_name: &str, baud_rate: u32) -> Result<String, String> {
        // Stop any previous EVT reader so a reconnect binds to the new port.
        if self.reader_running.load(Ordering::SeqCst) {
            self.stop_evt_reader();
            std::thread::sleep(Duration::from_millis(80));
        }
        let mut port_guard = self.port.lock();
        let mut name_guard = self.connected_port_name.lock();

        let serial = serialport::new(port_name, baud_rate)
            .timeout(Duration::from_millis(500))
            .open()
            .map_err(|e| format!("Failed to open COM port {}: {}", port_name, e))?;

        // Clone the handle for the background EVT reader so line reads never
        // block command writes (which keep using the primary handle).
        let mut reader_port = serial
            .try_clone()
            .map_err(|e| format!("Failed to clone COM port {}: {}", port_name, e))?;
        reader_port
            .set_timeout(Duration::from_millis(200))
            .map_err(|e| format!("Failed to set COM read timeout: {}", e))?;

        *port_guard = Some(serial);
        *name_guard = Some(port_name.to_string());
        drop(port_guard);
        drop(name_guard);

        self.start_evt_reader(reader_port);

        Ok(format!("Connected to ESP32 on {}", port_name))
    }

    /// Background thread: reads `EVT:*` lines from the ESP32 (button presses)
    /// into a small queue drained by `poll_hardware_buttons`. Exits on
    /// disconnect or persistent read failure.
    fn start_evt_reader(&self, reader_port: Box<dyn SerialPort>) {
        // Guard against a race where a previous reader hasn't fully exited yet.
        if self.reader_running.swap(true, Ordering::SeqCst) {
            tracing::warn!("EVT reader already running — previous reader will exit shortly");
            self.reader_running.store(true, Ordering::SeqCst);
        }
        let events = self.button_events.clone();
        let running = self.reader_running.clone();
        let port_name = self.connected_port_name.clone();
        let port_handle = self.port.clone();
        std::thread::spawn(move || {
            let mut reader_port = reader_port;
            let mut line = Vec::<u8>::with_capacity(64);
            let mut buf = [0u8; 64];
            let mut failures: u32 = 0;
            while running.load(Ordering::SeqCst) {
                match reader_port.read(&mut buf) {
                    Ok(0) => {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    Ok(n) => {
                        failures = 0;
                        for &b in &buf[..n] {
                            if b == b'\n' {
                                let text = String::from_utf8_lossy(&line).trim().to_string();
                                line.clear();
                                let kind = if text == "EVT:ENTRY_BTN" {
                                    Some("entry_btn")
                                } else if text == "EVT:EXIT_BTN" {
                                    Some("exit_btn")
                                } else {
                                    None
                                };
                                if let Some(kind) = kind {
                                    let mut q = events.lock();
                                    // Cap the queue: a stuck webview shouldn't grow memory
                                    if q.len() < 16 {
                                        q.push(HardwareButtonEvent {
                                            kind: kind.to_string(),
                                            timestamp_ms: std::time::SystemTime::now()
                                                .duration_since(std::time::UNIX_EPOCH)
                                                .map(|d| d.as_millis() as u64)
                                                .unwrap_or(0),
                                        });
                                    }
                                    tracing::info!("ESP32 button event: {}", kind);
                                }
                            } else if line.len() < 256 {
                                line.push(b);
                            } else {
                                line.clear(); // overlong line: resync
                            }
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                        // No data within timeout — normal idle, keep polling
                    }
                    Err(e) => {
                        failures += 1;
                        tracing::warn!("ESP32 serial read error ({}): {}", failures, e);
                        if failures >= 5 {
                            // Device likely disconnected — mirror send_command's
                            // auto-clear so the UI shows NotConnected.
                            *port_handle.lock() = None;
                            *port_name.lock() = None;
                            break;
                        }
                    }
                }
            }
            running.store(false, Ordering::SeqCst);
        });
    }

    fn stop_evt_reader(&self) {
        self.reader_running.store(false, Ordering::SeqCst);
    }

    /// Drains queued button events (called by the webview poll loop).
    pub fn drain_button_events(&self) -> Vec<HardwareButtonEvent> {
        std::mem::take(&mut *self.button_events.lock())
    }

    /// Send a serial command with connection health checking.
    /// Auto-clears the connection on write failure (broken pipe, device disconnected).
    pub fn send_command(&self, cmd: &str) -> Result<(), String> {
        let mut port_guard = self.port.lock();
        if let Some(port) = port_guard.as_mut() {
            let formatted = format!("{}\n", cmd.trim());
            match port.write_all(formatted.as_bytes()) {
                Ok(_) => {
                    if let Err(e) = port.flush() {
                        // Flush failure indicates connection degradation
                        tracing::warn!("ESP32 flush error (connection may be degraded): {}", e);
                    }
                    Ok(())
                }
                Err(e) => {
                    // Write failure: device likely disconnected — auto-clear connection
                    tracing::warn!("ESP32 write failed, auto-disconnecting: {}", e);
                    *port_guard = None;
                    drop(port_guard);
                    let mut name_guard = self.connected_port_name.lock();
                    *name_guard = None;
                    drop(name_guard);
                    self.stop_evt_reader();
                    Err(format!("Hardware disconnected during write: {}. Port auto-cleared.", e))
                }
            }
        } else {
            Err("No active hardware COM port connection".to_string())
        }
    }

    pub fn unlock_door(&self, duration_ms: u32) -> Result<String, String> {
        let secs = std::cmp::max(1, duration_ms / 1000);
        let cmd = format!("UNLOCK:{}", secs);
        self.send_command(&cmd)?;
        Ok(format!("Unlock command sent ({}s)", secs))
    }

    pub fn grant_entry(&self, member_name: &str, duration_ms: u32) -> Result<String, String> {
        let secs = std::cmp::max(1, duration_ms / 1000);
        let clean_name: String = member_name.chars().take(16).collect();
        let cmd = format!("WELCOME:{}|{}", clean_name, secs);
        self.send_command(&cmd)?;
        Ok(format!("Welcome sent for {} ({}s)", clean_name, secs))
    }

    pub fn grant_exit(&self, member_name: &str, duration_ms: u32) -> Result<String, String> {
        let secs = std::cmp::max(1, duration_ms / 1000);
        let clean_name: String = member_name.chars().take(16).collect();
        let cmd = format!("BYE:{}|{}", clean_name, secs);
        self.send_command(&cmd)?;
        Ok(format!("Bye sent for {} ({}s)", clean_name, secs))
    }

    pub fn trigger_alarm(&self, duration_ms: u32) -> Result<String, String> {
        let cmd = "ALERT_TAILGATE";
        self.send_command(cmd)?;
        Ok(format!("Alarm strobe & buzzer triggered ({}ms)", duration_ms))
    }

    /// Returns (is_connected, port_name)
    pub fn get_status(&self) -> (bool, Option<String>) {
        let name = self.connected_port_name.lock().clone();
        let connected = name.is_some();
        (connected, name)
    }

    /// Convenience method for quick boolean connection check
    pub fn is_connected(&self) -> bool {
        self.connected_port_name.lock().is_some()
    }
}

