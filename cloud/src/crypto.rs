use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use gympos_shared::LicenseClaims;
use rsa::pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePublicKey, LineEnding};
use rsa::pss::{BlindedSigningKey, VerifyingKey};
use rsa::signature::{RandomizedSigner, SignatureEncoding, Verifier};
use rsa::RsaPrivateKey;
use sha2::Sha256;
use std::sync::Arc;

// NOTE: A hardcoded "default production" RSA private key previously lived here.
// It was checked into source control, which means it was never actually secret —
// anyone with repository or binary access could extract it and self-sign
// unlimited GymPOS license tokens, completely defeating the CEO-only licensing
// model described in the architecture doc. It has been removed. The signer now
// REQUIRES `RSA_PRIVATE_KEY_PEM` to be set for a stable production identity,
// falling back to a fresh in-memory ephemeral key (with a loud warning) so the
// server can still boot in local/dev environments. Generate a real key pair with:
//   cargo run --bin gen_keys -p gympos-cloud
// and set the resulting private PEM as the `RSA_PRIVATE_KEY_PEM` secret.

#[derive(Clone)]
pub struct LicenseSigner {
    signing_key: Arc<BlindedSigningKey<Sha256>>,
    public_key_pem: String,
}

impl LicenseSigner {
    /// Generate a fresh in-memory key pair. Used for tests, and as the dev-mode
    /// fallback in `main.rs` when `RSA_PRIVATE_KEY_PEM` is not configured.
    /// NOT suitable for production: the key is lost (and all previously issued
    /// licenses become unverifiable) on every process restart.
    pub fn generate_ephemeral() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let mut rng = rand::thread_rng();
        let private_key = RsaPrivateKey::new(&mut rng, 2048)?;
        let public_key = private_key.to_public_key();
        let public_key_pem = public_key.to_public_key_pem(LineEnding::LF)?;
        let signing_key = BlindedSigningKey::<Sha256>::new(private_key);

        Ok(Self {
            signing_key: Arc::new(signing_key),
            public_key_pem,
        })
    }

    /// Load from PKCS#8 PEM string
    pub fn from_pem(private_pem: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let private_key = RsaPrivateKey::from_pkcs8_pem(private_pem)?;
        let public_key = private_key.to_public_key();
        let public_key_pem = public_key.to_public_key_pem(LineEnding::LF)?;
        let signing_key = BlindedSigningKey::<Sha256>::new(private_key);

        Ok(Self {
            signing_key: Arc::new(signing_key),
            public_key_pem,
        })
    }

    pub fn public_key_pem(&self) -> &str {
        &self.public_key_pem
    }

    /// Sign a LicenseClaims struct and return formatted license token
    /// Format: `GPOS-<base64_json_claims>.<base64_signature>`
    pub fn sign_license(&self, claims: &LicenseClaims) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let json_bytes = serde_json::to_vec(claims)?;
        let mut rng = rand::thread_rng();
        let signature = self.signing_key.sign_with_rng(&mut rng, &json_bytes);

        let claims_b64 = URL_SAFE_NO_PAD.encode(&json_bytes);
        let sig_b64 = URL_SAFE_NO_PAD.encode(signature.to_bytes());

        Ok(format!("GPOS-{}.{}", claims_b64, sig_b64))
    }
}

/// Standalone verifier (used by both Cloud and Desktop client)
pub fn verify_license_token(
    token: &str,
    public_key_pem: &str,
) -> Result<LicenseClaims, Box<dyn std::error::Error + Send + Sync>> {
    let stripped = token.trim().strip_prefix("GPOS-").ok_or("Invalid license prefix (expected GPOS-)")?;
    let parts: Vec<&str> = stripped.split('.').collect();
    if parts.len() != 2 {
        return Err("Malformed license token format".into());
    }

    let claims_bytes = URL_SAFE_NO_PAD.decode(parts[0])?;
    let sig_bytes = URL_SAFE_NO_PAD.decode(parts[1])?;

    let public_key = rsa::RsaPublicKey::from_public_key_pem(public_key_pem)?;
    let verifying_key = VerifyingKey::<Sha256>::new(public_key);
    let signature = rsa::pss::Signature::try_from(sig_bytes.as_slice())
        .map_err(|e| format!("Invalid signature structure: {}", e))?;

    verifying_key.verify(&claims_bytes, &signature)?;

    let claims: LicenseClaims = serde_json::from_slice(&claims_bytes)?;
    Ok(claims)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    use gympos_shared::LicenseTier;
    use uuid::Uuid;

    #[test]
    fn test_license_signing_and_verification() {
        let signer = LicenseSigner::generate_ephemeral().expect("Failed to generate ephemeral key");
        let claims = LicenseClaims {
            license_id: Uuid::new_v4(),
            gym_id: Uuid::new_v4(),
            gym_name: "Shadow Monarch Fitness".to_string(),
            owner_email: "shadow@monarch.com".to_string(),
            tier: LicenseTier::Pro,
            issued_at: Utc::now(),
            expires_at: Utc::now() + Duration::days(30),
            max_members: 500,
            hardware_lock_enabled: true,
            tailgate_detection_enabled: true,
            hwid: "deadbeef".to_string(),
            ip_hint: "203.0.113.9".to_string(),
            exp_unix: (Utc::now() + Duration::days(30)).timestamp(),
            grace_until: (Utc::now() + Duration::days(33)).timestamp(),
        };

        let token = signer.sign_license(&claims).expect("Signing failed");
        assert!(token.starts_with("GPOS-"));

        let verified_claims = verify_license_token(&token, signer.public_key_pem())
            .expect("Verification failed");

        assert_eq!(verified_claims.gym_name, "Shadow Monarch Fitness");
        assert_eq!(verified_claims.owner_email, "shadow@monarch.com");
        assert_eq!(verified_claims.max_members, 500);
        assert_eq!(verified_claims.tier, LicenseTier::Pro);
    }
}
