use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::Utc;
use gympos_shared::{LicenseClaims, LicenseStatus};
use rsa::pkcs8::DecodePublicKey;
use rsa::pss::VerifyingKey;
use rsa::signature::Verifier;
use sha2::Sha256;

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
