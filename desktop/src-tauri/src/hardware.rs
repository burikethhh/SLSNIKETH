use parking_lot::Mutex;
use serialport::{SerialPort, SerialPortType};
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct HardwareManager {
    port: Arc<Mutex<Option<Box<dyn SerialPort>>>>,
    connected_port_name: Arc<Mutex<Option<String>>>,
}

impl HardwareManager {
    pub fn new() -> Self {
        Self {
            port: Arc::new(Mutex::new(None)),
            connected_port_name: Arc::new(Mutex::new(None)),
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
        let mut port_guard = self.port.lock();
        let mut name_guard = self.connected_port_name.lock();

        let serial = serialport::new(port_name, baud_rate)
            .timeout(Duration::from_millis(500))
            .open()
            .map_err(|e| format!("Failed to open COM port {}: {}", port_name, e))?;

        *port_guard = Some(serial);
        *name_guard = Some(port_name.to_string());

        Ok(format!("Connected to ESP32 on {}", port_name))
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
                    let mut name_guard = self.connected_port_name.lock();
                    *name_guard = None;
                    Err(format!("Hardware disconnected during write: {}. Port auto-cleared.", e))
                }
            }
        } else {
            Err("No active hardware COM port connection".to_string())
        }
    }

    pub fn unlock_door(&self, duration_ms: u32) -> Result<String, String> {
        let cmd = format!("UNLOCK:{}", duration_ms);
        self.send_command(&cmd)?;
        Ok(format!("Unlock command sent ({}ms)", duration_ms))
    }

    pub fn trigger_alarm(&self, duration_ms: u32) -> Result<String, String> {
        let cmd = format!("ALARM:{}", duration_ms);
        self.send_command(&cmd)?;
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

