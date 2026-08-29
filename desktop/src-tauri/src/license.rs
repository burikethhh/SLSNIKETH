use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::Utc;
use gympos_shared::{LicenseClaims, LicenseStatus};
use rsa::pkcs8::DecodePublicKey;
use rsa::pss::VerifyingKey;
use rsa::signature::Verifier;
use sha2::{Digest, Sha256};

/// Compute a stable device fingerprint for HWID binding.
/// Mirrors `SLS123/license/validator.py::get_hwid()` exactly:
///   anchors = [MachineGuid (winreg), disk SerialNumber (wmic), validated MAC, hostname]
///   sorted(set(anchors)) → sha256 → hex[:32]
/// Fallback: machine_uid + uuid node if <2 anchors (also mirrors Python fallback).
pub fn get_hwid() -> String {
    let mut anchors: Vec<String> = Vec::new();

    // 1. MachineGuid from Windows Registry (primary anchor — same as machine_uid on Windows)
    #[cfg(target_os = "windows")]
    {
        if let Ok(hkey) = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE)
            .open_subkey(r"SOFTWARE\Microsoft\Cryptography")
        {
            if let Ok(guid) = hkey.get_value::<String, _>("MachineGuid") {
                let g = guid.trim().to_string();
                if !g.is_empty() {
                    anchors.push(g);
                }
            }
        }
    }
    // Fallback via machine_uid crate (already reads MachineGuid on Windows)
    if anchors.is_empty() {
        if let Ok(uid) = machine_uid::get() {
            let u = uid.trim().to_string();
            if !u.is_empty() && u != "unknown-machine" {
                anchors.push(u);
            }
        }
    }

    // 2. Disk serial via wmic (matches Python: wmic diskdrive get SerialNumber)
    if let Ok(output) = std::process::Command::new("wmic")
        .args(["diskdrive", "get", "SerialNumber"])
        .output()
    {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                let s = line.trim();
                if s.is_empty() || s.to_lowercase() == "serialnumber" {
                    continue;
                }
                // wmic pads with spaces — take first token
                let token = s.split_whitespace().next().unwrap_or("").trim().to_string();
                if !token.is_empty() {
                    anchors.push(token);
                    break;
                }
            }
        }
    }

    // 3. Validated MAC (skip locally-administered / random MAC — matches Python (node>>40)&0x01==0)
    // Try wmic nic first (no extra crate), then fall back to parsing ipconfig if needed
    if let Ok(output) = std::process::Command::new("wmic")
        .args(["nic", "where", "PhysicalAdapter=TRUE", "get", "MacAddress"])
        .output()
    {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                let s = line.trim();
                if s.is_empty() || s.to_lowercase() == "macaddress" {
                    continue;
                }
                // Validate MAC format and check locally-administered bit
                let mac = s.replace('-', ":").to_uppercase();
                let parts: Vec<&str> = mac.split(':').collect();
                if parts.len() == 6 {
                    if let Ok(first_byte) = u8::from_str_radix(parts[0], 16) {
                        // Python: (node >> 40) & 0x01 == 0 → multicast/local bit not set
                        // For MAC string, first byte LSB (0x01) = I/G, second LSB (0x02) = U/L
                        // Python checks multicast; we also reject locally-administered (0x02)
                        // to avoid random MACs from VPNs.
                        if (first_byte & 0x01) == 0 && (first_byte & 0x02) == 0 && mac != "00:00:00:00:00:00" {
                            anchors.push(mac.replace(':', "").to_lowercase());
                            break;
                        }
                    }
                }
            }
        }
    }

    // 4. Hostname (always present)
    let hostname = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .or_else(|_| std::env::var("HOST"))
        .unwrap_or_else(|_| "host".to_string());
    let hn = hostname.trim().to_string();
    if !hn.is_empty() {
        anchors.push(hn);
    }

    // Fallback if <2 anchors (mirrors Python: fallback-" + uuid.getnode())
    if anchors.len() < 2 {
        // Use machine_uid fallback or generate a stable-ish fallback from hostname hash
        let fallback = format!("fallback-{}", hostname);
        anchors.push(fallback);
    }

    // Sorted unique set → join with "|" → sha256 → hex[:32] (mirrors Python sorted(set(anchors)))
    anchors.sort();
    anchors.dedup();
    let raw = anchors.join("|");

    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
    hex.chars().take(32).collect()
}

/// 7-day heartbeat window (seconds) — must re-verify online within this period, mirrors `validator.py:19` HEARTBEAT_SECONDS
pub const HEARTBEAT_SECONDS: i64 = 7 * 24 * 3600;
/// Clock tamper grace (seconds) — if now < last_seen - 60s, treat as rollback tamper, immediate LOCK
pub const TAMPER_GRACE_SECONDS: i64 = 60;

/// Pure helper: returns true if system clock was rolled back (tamper).
pub fn is_clock_tampered(now_unix: i64, last_seen_unix: i64) -> bool {
    last_seen_unix != 0 && now_unix < last_seen_unix - TAMPER_GRACE_SECONDS
}
/// Pure helper: returns true if heartbeat window expired (offline too long).
pub fn is_heartbeat_expired(now_unix: i64, last_verify_unix: i64) -> bool {
    last_verify_unix != 0 && now_unix - last_verify_unix > HEARTBEAT_SECONDS
}

pub const EMBEDDED_PUBLIC_KEY_PEM: &str = r#"-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAr3tMulsXeUjbCLhDfgcn
3oMMqFhV1dl/qiNHRiq4Ci0Rz42bORrv8GTXtVme4FPIhnRlgKPqVunjUM9tqHXA
WVZxOi5EJ9cHelFxYdwy7EVRva4QJJMfmRxud4ck+SAo/sPkuy6i9BueORdOypkB
Zy5X94Ok1EmnZvnnuq8FuEIgwCr0lAgrWJYi/rqwtxHKLSLVl5cTalte5m2xASHL
F1B9Pos4b5Ce1VLS/n6bq69bhB2KXht9oq0Jd/XNEOx8AeYzcGyRlmtldPXRE4dI
yh0EFfogPk4NElGtiF4US/eJ12ku/sqs752S8cT2f/nh/WLaCEJZuR/S3+hL5jJQ
0QIDAQAB
-----END PUBLIC KEY-----"#;

pub struct LicenseManager {
    public_key_pem: String,
    current_claims: parking_lot::RwLock<Option<LicenseClaims>>,
}

impl LicenseManager {
    pub fn new(public_key_pem: Option<String>) -> Self {
        Self {
            public_key_pem: public_key_pem.unwrap_or_else(|| EMBEDDED_PUBLIC_KEY_PEM.to_string()),
            current_claims: parking_lot::RwLock::new(None),
        }
    }

    pub fn set_public_key(&mut self, pem: String) {
        self.public_key_pem = pem;
    }

    pub fn verify_and_apply(&self, token: &str) -> Result<LicenseStatus, String> {
        let claims = self.verify_token(token)?;

        // Hardware lock: if issuer bound a hwid and HW lock is enabled, enforce 1-device binding.
        if claims.hardware_lock_enabled && !claims.hwid.is_empty() {
            let this_hwid = get_hwid();
            if claims.hwid != this_hwid {
                return Err(format!(
                    "Hardware lock mismatch: license is bound to device '{}' but this machine is '{}'.",
                    claims.hwid, this_hwid
                ));
            }
        }

        let status = claims.evaluate(Utc::now());

        if !matches!(status, LicenseStatus::Expired { .. } | LicenseStatus::Invalid { .. }) {
            *self.current_claims.write() = Some(claims);
        }

        Ok(status)
    }

    pub fn verify_token(&self, token: &str) -> Result<LicenseClaims, String> {
        let stripped = token.trim().strip_prefix("GPOS-").ok_or("Invalid license key prefix")?;
        let parts: Vec<&str> = stripped.split('.').collect();
        if parts.len() != 2 {
            return Err("Malformed license token format".to_string());
        }

        let claims_bytes = URL_SAFE_NO_PAD
            .decode(parts[0])
            .map_err(|e| format!("Base64 claims decode error: {}", e))?;
        let sig_bytes = URL_SAFE_NO_PAD
            .decode(parts[1])
            .map_err(|e| format!("Base64 signature decode error: {}", e))?;

        let public_key = rsa::RsaPublicKey::from_public_key_pem(&self.public_key_pem)
            .map_err(|e| format!("Invalid public key PEM: {}", e))?;
        let verifying_key = VerifyingKey::<Sha256>::new(public_key);
        let signature = rsa::pss::Signature::try_from(sig_bytes.as_slice())
            .map_err(|e| format!("Invalid signature structure: {}", e))?;

        verifying_key
            .verify(&claims_bytes, &signature)
            .map_err(|e| format!("Cryptographic signature verification failed: {}", e))?;

        let claims: LicenseClaims = serde_json::from_slice(&claims_bytes)
            .map_err(|e| format!("JSON deserialization error: {}", e))?;

        Ok(claims)
    }

    pub fn current_status(&self) -> LicenseStatus {
        let read = self.current_claims.read();
        match &*read {
            Some(claims) => claims.evaluate(Utc::now()),
            None => LicenseStatus::Unlicensed,
        }
    }

    pub fn current_claims(&self) -> Option<LicenseClaims> {
        self.current_claims.read().clone()
    }

    pub fn revoke(&self) {
        *self.current_claims.write() = None;
    }
}
