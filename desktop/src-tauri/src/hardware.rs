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

    /// USB VID whitelist for auto-detection: FTDI, SiLabs CP210x, WCH CH34x,
    /// Espressif native USB-Serial/JTAG — the adapters every ESP32 board uses.
    const AUTO_CONNECT_VIDS: [u16; 4] = [0x0403, 0x10C4, 0x1A86, 0x303A];

    /// ALL USB serial ports whose adapter VID matches a known ESP32 bridge
    /// (FTDI, SiLabs CP210x, WCH CH34x, Espressif native). With 3+ USB
    /// devices plugged in, the auto-connect tries each and keeps the one
    /// that actually answers as a GymPOS controller.
    pub fn find_esp_ports(&self) -> Vec<String> {
        let mut out = Vec::new();
        for p in serialport::available_ports().unwrap_or_default() {
            if let SerialPortType::UsbPort(info) = &p.port_type {
                if Self::AUTO_CONNECT_VIDS.contains(&info.vid) {
                    out.push(p.port_name);
                }
            }
        }
        out
    }

    pub fn connect(&self, port_name: &str, baud_rate: u32) -> Result<String, String> {
        // Stop any previous EVT reader so a reconnect binds to the new port.
        if self.reader_running.load(Ordering::SeqCst) {
            self.stop_evt_reader();
            std::thread::sleep(Duration::from_millis(80));
        }
        let mut port_guard = self.port.lock();
        let mut name_guard = self.connected_port_name.lock();

        let mut serial = serialport::new(port_name, baud_rate)
            .timeout(Duration::from_millis(500))
            .open()
            .map_err(|e| format!("Failed to open COM port {}: {}", port_name, e))?;

        // The field FTDI adapter wires DTR → EN: an asserted DTR holds the
        // ESP32 in reset while the port is open (observed live — the board
        // booted only after DTR was released). Release both modem lines so
        // the chip runs and answers commands.
        let _ = serial.write_data_terminal_ready(false);
        let _ = serial.write_request_to_send(false);

        // Identity check: with several USB-serial devices plugged in, a
        // whitelisted port may belong to something else (second adapter,
        // RFID reader). A real GymPOS controller answers PING with
        // ACK:PONG in milliseconds; two attempts because the DTR edge
        // above can land during the chip's own boot.
        let mut verified = false;
        for _ in 0..2 {
            if serial.write_all(b"PING\n").and_then(|_| serial.flush()).is_err() {
                break;
            }
            let deadline = std::time::Instant::now() + Duration::from_millis(450);
            let mut buf = [0u8; 256];
            while std::time::Instant::now() < deadline {
                match serial.read(&mut buf) {
                    Ok(0) => std::thread::sleep(Duration::from_millis(20)),
                    Ok(n) => {
                        if String::from_utf8_lossy(&buf[..n]).contains("ACK:PONG") {
                            verified = true;
                            break;
                        }
                    }
                    Err(_) => {}
                }
            }
            if verified {
                break;
            }
        }
        if !verified {
            return Err(format!(
                "Port {} answered no PING — not a GymPOS controller (or still booting).",
                port_name
            ));
        }

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
    /// Retries twice on transient write errors (USB power sags during the
    /// siren/relay blast are exactly the "disconnects at a crucial time"
    /// failure) and only auto-clears the connection after the final failure.
    pub fn send_command(&self, cmd: &str) -> Result<(), String> {
        let formatted = format!("{}\n", cmd.trim());
        let mut last_err = String::new();
        for attempt in 0..3 {
            let mut port_guard = self.port.lock();
            match port_guard.as_mut() {
                Some(port) => {
                    match port.write_all(formatted.as_bytes()).and_then(|_| port.flush()) {
                        Ok(_) => return Ok(()),
                        Err(e) => {
                            last_err = format!("attempt {}: {}", attempt + 1, e);
                            tracing::warn!(
                                "ESP32 write failed ({}), {}",
                                e,
                                if attempt < 2 { "retrying" } else { "auto-clearing connection" }
                            );
                            std::thread::sleep(Duration::from_millis(80));
                        }
                    }
                }
                None => {
                    return Err("No active hardware COM port connection".to_string());
                }
            }
        }
        // All retries failed — device likely disconnected: auto-clear so the
        // UI shows NotConnected and the auto-detect loop can re-attach.
        *self.port.lock() = None;
        *self.connected_port_name.lock() = None;
        self.stop_evt_reader();
        Err(format!(
            "Hardware disconnected during write ({}). Port auto-cleared — reconnecting automatically.",
            last_err
        ))
    }

    /// Show "ACCESS DENIED / <reason>" on the gate LCD + triple beep
    /// (firmware DENY:<reason>). Reason is sanitized for the serial protocol
    /// and truncated to the LCD's 16-column width.
    pub fn deny(&self, reason: &str) -> Result<String, String> {
        let clean: String = reason
            .chars()
            .filter(|c| !matches!(c, '|' | ':' | '\n' | '\r'))
            .take(16)
            .collect();
        self.send_command(&format!("DENY:{}", clean))?;
        Ok(format!("Deny shown ({})", clean))
    }

    /// Push the owner-branded idle screen to the firmware LCD (v1.2.0+):
    /// line 1 = brand (sanitized, ≤14 chars beside the lock icon), line 2 =
    /// the call to action. The ESP32 persists it in NVS, so it survives
    /// power cycles without the exe re-sending it.
    pub fn set_idle_screen(&self, brand: &str) -> Result<String, String> {
        let clean: String = brand
            .chars()
            .filter(|c| !matches!(c, '|' | ':' | '\n' | '\r'))
            .take(14)
            .collect();
        let cmd = format!("IDLE:{}|Scan Face", clean);
        self.send_command(&cmd)?;
        Ok(format!("Idle screen set ({}).", clean))
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

    /// Trigger the tailgate alarm. The firmware (v1.1.0+) honors the duration
    /// (clamped 1s..15s); older firmware ignores it and plays its fixed ~9s
    /// pattern, so the siren length is safe on both.
    pub fn trigger_alarm(&self, duration_ms: u32) -> Result<String, String> {
        let ms = duration_ms.clamp(1000, 15000);
        let cmd = format!("ALERT_TAILGATE:{}", ms);
        self.send_command(&cmd)?;
        Ok(format!("Alarm strobe & buzzer triggered ({}ms)", ms))
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

