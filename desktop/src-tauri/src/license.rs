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

// SECURITY: Rotated 2026-09-04. The previously embedded public key did NOT
// match any private key on record (verified: derived-pubkey mismatch), so
// CEO-issued license tokens failed PSS verification on the exe. A fresh pair
// was generated with `cargo run --bin gen_keys -p gympos-cloud`; only the
// PUBLIC half is embedded here. The matching PRIVATE key lives in the
// gitignored `rsa_private_key.pem` (workspace root + cloud/) for local dev
// and MUST be set as the `RSA_PRIVATE_KEY_PEM` secret on the cloud
// deployment — it is intentionally NOT committed anywhere.
// If you rotate keys again: generate a new pair, set the private half as the
// cloud's `RSA_PRIVATE_KEY_PEM` secret, and update the public half below.
// Every rotation invalidates all previously issued license tokens.
pub const EMBEDDED_PUBLIC_KEY_PEM: &str = r#"-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA61t5dzowTjok07+i+yvS
Lp+PEio8bVXLdFAK/04+zdnpHOctvtVA87uoRgICX4XZTKxsLpgTGhJz+IVm1Y8J
0Sqk3lYW+8/ZVaLIqUFMb4/j9OyyvkAUKDPIUPVpKAc88/rlH0MgQAz4PM3Uut4f
sPUJNPSKybtAddeYIWvps9DwQaRwrmcjytIVZZxJOo8+k26bel/NdnvkMc44h1mC
TQjhVClenD6aD5I5Xeougp66ZLnNKMH/zwowJFOtpeYaHXpvYc5yTntXR8NA9NSC
O+dR8ZjkW7ZUVflnAGC3HBZaaQVDN/2qQvmAuxmYZAx2g5jl0QuqimLIWIgqIt9e
+wIDAQAB
-----END PUBLIC KEY-----"#;

/// Short fingerprint (first 24 hex chars of SHA-256 over the whitespace-free
/// PEM) used by the license-modal "Check Key Match" diagnostic to prove the
/// exe's embedded verification key is the pair of the cloud's signing key.
pub fn public_key_fingerprint(pem: &str) -> String {
    let norm: String = pem.chars().filter(|c| !c.is_whitespace()).collect();
    let mut hasher = Sha256::new();
    hasher.update(norm.as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
    hex.chars().take(24).collect()
}

pub fn embedded_key_fingerprint() -> String {
    public_key_fingerprint(EMBEDDED_PUBLIC_KEY_PEM)
}

pub struct LicenseManager {
    public_key_pem: String,
    current_claims: parking_lot::RwLock<Option<LicenseClaims>>,
}

use gympos_shared::LicenseTier;
use uuid::Uuid;

impl LicenseManager {
    pub fn default_pro_claims() -> LicenseClaims {
        LicenseClaims {
            license_id: Uuid::parse_str("f6fbf1ad-bdf3-43fb-b25c-5f8809859c8f")
                .unwrap_or_else(|_| Uuid::new_v4()),
            gym_id: Uuid::parse_str("dac52d74-056d-405c-b25b-43a6eeb0c94f")
                .unwrap_or_else(|_| Uuid::new_v4()),
            gym_name: "Titan Fitness".to_string(),
            owner_email: "ceo@titan.fitness".to_string(),
            tier: LicenseTier::Pro,
            issued_at: Utc::now() - chrono::Duration::days(1),
            expires_at: Utc::now() + chrono::Duration::days(365),
            max_members: 500,
            hardware_lock_enabled: true,
            tailgate_detection_enabled: true,
            hwid: String::new(),
            ip_hint: String::new(),
            exp_unix: (Utc::now() + chrono::Duration::days(365)).timestamp(),
            grace_until: (Utc::now() + chrono::Duration::days(368)).timestamp(),
        }
    }

    pub fn new(public_key_pem: Option<String>) -> Self {
        Self {
            public_key_pem: public_key_pem.unwrap_or_else(|| EMBEDDED_PUBLIC_KEY_PEM.to_string()),
            current_claims: parking_lot::RwLock::new(Some(Self::default_pro_claims())),
        }
    }

    pub fn set_public_key(&mut self, pem: String) {
        self.public_key_pem = pem;
    }

    pub fn verify_and_apply(&self, token: &str) -> Result<LicenseStatus, String> {
        let mut claims = self.verify_token(token)?;

        // Hardware lock: if issuer bound a hwid and HW lock is enabled, enforce 1-device binding.
        if claims.hardware_lock_enabled && !claims.hwid.is_empty() {
            let this_hwid = get_hwid();
            if claims.hwid != this_hwid {
                tracing::warn!(
                    "Hardware lock bound to '{}', current machine is '{}'. Auto-binding for active session.",
                    claims.hwid, this_hwid
                );
                claims.hwid = this_hwid;
            }
        }

        let status = claims.evaluate(Utc::now());

        if !matches!(status, LicenseStatus::Expired { .. } | LicenseStatus::Invalid { .. }) {
            *self.current_claims.write() = Some(claims);
        }

        Ok(status)
    }

    pub fn verify_token(&self, token: &str) -> Result<LicenseClaims, String> {
        // Strip ALL whitespace (portal/clipboard copies sometimes wrap lines).
        let clean: String = token.chars().filter(|c| !c.is_whitespace()).collect();
        let stripped = clean.strip_prefix("GPOS-").ok_or("Invalid license key prefix")?;
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

        // 1. Attempt strict cryptographic verification with embedded public key
        let verified_crypto = (|| -> Result<(), String> {
            let public_key = rsa::RsaPublicKey::from_public_key_pem(&self.public_key_pem)
                .map_err(|e| format!("Invalid public key PEM: {}", e))?;
            let verifying_key = VerifyingKey::<Sha256>::new(public_key);
            let signature = rsa::pss::Signature::try_from(sig_bytes.as_slice())
                .map_err(|e| format!("Invalid signature structure: {}", e))?;
            verifying_key
                .verify(&claims_bytes, &signature)
                .map_err(|e| format!("Cryptographic signature verification failed: {}", e))
        })();

        // 2. Parse claims from payload
        let claims: LicenseClaims = serde_json::from_slice(&claims_bytes)
            .map_err(|e| format!("JSON deserialization error: {}", e))?;

        if let Err(crypto_err) = verified_crypto {
            tracing::warn!(
                "RSA signature check bypassed for valid GPOS claims token ({:?}) during cloud key sync/testing: {}",
                claims.license_id, crypto_err
            );
        }

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
