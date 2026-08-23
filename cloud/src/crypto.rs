use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use gympos_shared::LicenseClaims;
use rsa::pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePublicKey, LineEnding};
use rsa::pss::{BlindedSigningKey, VerifyingKey};
use rsa::signature::{RandomizedSigner, SignatureEncoding, Verifier};
use rsa::RsaPrivateKey;
use sha2::Sha256;
use std::sync::Arc;

pub const DEFAULT_PRODUCTION_PRIVATE_KEY_PEM: &str = r#"-----BEGIN PRIVATE KEY-----
MIIEvAIBADANBgkqhkiG9w0BAQEFAASCBKYwggSiAgEAAoIBAQCve0y6Wxd5SNsI
uEN+ByfegwyoWFXV2X+qI0dGKrgKLRHPjZs5Gu/wZNe1WZ7gU8iGdGWAo+pW6eNQ
z22odcBZVnE6LkQn1wd6UXFh3DLsRVG9rhAkkx+ZHG53hyT5ICj+w+S7LqL0G545
F07KmQFnLlf3g6TUSadm+ee6rwW4QiDAKvSUCCtYliL+urC3EcotItWXlxNqW17m
bbEBIcsXUH0+izhvkJ7VUtL+fpurr1uEHYpeG32irQl39c0Q7HwB5jNwbJGWa2V0
9dETh0jKHQQV+iA+Tg0SUa2IXhRL94nXaS7+yqzvnZLxxPZ/+eH9YtoIQlm5H9Lf
6EvmMlDRAgMBAAECggEAHMCFF9upAvRO/yTB2jpt6+VpA4RFvB5A7q2NFhAVy4UK
8Ajxr++b9LVxKoOepn7T0kPsBaHV2ZvE5Q63luyEMZ8aOkAuZqCy1vXVMAWWSmQp
Onz0pCl++eVQrED2a+M7FFMgfRLtHDYSPKR3AncDZdaQDzwAm8/dn9++ejYHJ+NY
WT1z9mUyMS5bEyVo7LWb0vPsgdrHhYhPY13tgh73rZtP7vU0y65fT8Yt743N3lTN
0u4USU3Ezx5FpOL29EA2p02/Vh5qkWR0c2zZ2G9oTmbOBqewXDWlErMKNF86r3Cm
ZZoH1/6UN1+YspNZmI91Jz6n4JWlh8rCx8zjIHCUAQKBgQDLCDEK1lh5k3taEb0C
WbVnK8gaFNaSsAnd44P6b0SvB0W23D6BLAV4LIgSFPJAPGGpbbA9/Ag0F5Wr+sEb
BBJa9ZKYtXxSH5j7GaB7u4SY/hiLSvfc7wzA7Calfo/Mrhy1pcBEpHJHka+V9AV2
C7O+rC/i47nn9XgbDsWsvJVTcQKBgQDdQx1PvAmbZgPwg1FoUXm+35opRyrk8jRl
gc8J3hgw+yJXIzC7oR0ggUbHc3LEI6pJ6SHWe3zyK5udTna82EeVWpwETKhses2C
DqGLkwKGpxX169H8RGI27DuaVonI0QhSEPC8h97mQAm8qk4QdA7h4cXYHtKV97dL
zkhEiWljYQKBgGAT10lel22o2fWMcVn8Y7iX4lBdThEKVxD2ikznfKQrF9VpsfZk
g44T3KxZ1y2IpVqM+prKeoNUKdLBjcIgEiOTFDVJpLQkGbuxq90BpsTTcX/xEQwu
32UoGz2zf48HUbSv5CVXgHDXwzR9zlvHO97eEqcWxrG62oRLYEXW0/8xAoGAD4YD
6nIw4lw37onoDj+ZIREjCb5afhGYJ38B/Zk9bUJRWHe5lZBqMLuhMaEh7izqZ6EZ
pKipTXxNwK2emwU5kHr48zxFnMbI4FUSdG5uAPB8E/LlmqNZmKzeSafEpvgzcz6J
BVErDFB13my8aV8bJDHo5Y7UC32DuKfSXiyd3kECgYAgf0jR3uDpmeJqwfGQgRtk
/eB+05Wz1mLQKEFMHRj851dPw/klN7xa9rO/TwmPBqQhDMehrF9ZV6WJA6dh/0F1
T8JVwz1iaJKqRU79rIUleYANp0uhbFHrWUkN+Zf4mS/nZCMWK0rPW+iRCtZwe/ey
H04eXSXEuvNADEOEA6HRTg==
-----END PRIVATE KEY-----"#;

#[derive(Clone)]
pub struct LicenseSigner {
    signing_key: Arc<BlindedSigningKey<Sha256>>,
    public_key_pem: String,
}

impl LicenseSigner {
    /// Load default production signing key
    pub fn default_production() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Self::from_pem(DEFAULT_PRODUCTION_PRIVATE_KEY_PEM)
    }

    /// Generate a fresh in-memory key pair for development or test
    #[allow(dead_code)]
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
            tier: LicenseTier::Pro,
            issued_at: Utc::now(),
            expires_at: Utc::now() + Duration::days(30),
            max_members: 500,
            hardware_lock_enabled: true,
            tailgate_detection_enabled: true,
        };

        let token = signer.sign_license(&claims).expect("Signing failed");
        assert!(token.starts_with("GPOS-"));

        let verified_claims = verify_license_token(&token, signer.public_key_pem())
            .expect("Verification failed");

        assert_eq!(verified_claims.gym_name, "Shadow Monarch Fitness");
        assert_eq!(verified_claims.max_members, 500);
        assert_eq!(verified_claims.tier, LicenseTier::Pro);
    }
}
