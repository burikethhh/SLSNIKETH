use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use gympos_shared::LicenseClaims;
use sha2::Digest;
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
    let clean: String = token.chars().filter(|c| !c.is_whitespace()).collect();
    let stripped = clean.strip_prefix("GPOS-").ok_or("Invalid license prefix (expected GPOS-)")?;
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

/// Stateless HMAC-SHA256 session tokens for CEO/ownerBearer auth.
///
/// Format: `<kind>:<email>:<exp_unix>:<hex_hmac>` where
/// `hmac = HMAC_SHA256(secret, "<kind>|<email>|<exp_unix>")`.
/// This replaces the old forgeable `ceo:<email>` / `owner:<email>` strings:
/// knowing an email address is no longer enough — the HMAC must verify with
/// the server secret and the token must be unexpired.
///
/// The secret is `CEO_TOKEN_SECRET` env when set, otherwise SHA-256 of the
/// RSA private PEM in use (stable across restarts whenever the signing key
/// is stable, secret whenever the private key is secret). As a last resort
/// it is an ephemeral random value (sessions die on restart, loudly logged).
#[derive(Clone)]
pub struct SessionTokens {
    secret: Vec<u8>,
}

/// Session lifetimes: CEO tokens are short (12h, re-login daily),
/// owner tokens last 30 days (portal convenience).
pub const CEO_TOKEN_TTL_SECS: i64 = 12 * 3600;
pub const OWNER_TOKEN_TTL_SECS: i64 = 30 * 86400;

impl SessionTokens {
    pub fn new(secret: Vec<u8>) -> Self {
        Self { secret }
    }

    pub fn from_env_or_rsa_pem(rsa_private_pem: Option<&str>) -> Self {
        if let Ok(s) = std::env::var("CEO_TOKEN_SECRET") {
            if !s.trim().is_empty() {
                return Self::new(s.trim().as_bytes().to_vec());
            }
        }
        if let Some(pem) = rsa_private_pem {
            let mut h = Sha256::new();
            h.update(b"gympos-ceo-token-v1|");
            h.update(pem.as_bytes());
            return Self::new(h.finalize().to_vec());
        }
        tracing::warn!("CEO_TOKEN_SECRET unset and no RSA key available — session tokens will be ephemeral (all logins die on restart)");
        let mut rng = rand::thread_rng();
        let mut bytes = [0u8; 32];
        use rand::RngCore;
        rng.fill_bytes(&mut bytes);
        Self::new(bytes.to_vec())
    }

    fn hmac(&self, kind: &str, email: &str, exp_unix: i64) -> Vec<u8> {
        hmac_sha256(&self.secret, format!("{}|{}|{}", kind, email, exp_unix).as_bytes())
    }

    /// Mint a token valid for `ttl_secs` from now.
    pub fn mint(&self, kind: &str, email: &str, ttl_secs: i64, now_unix: i64) -> String {
        let email = email.trim().to_lowercase();
        let exp = now_unix + ttl_secs;
        let sig: String = self
            .hmac(kind, &email, exp)
            .iter()
            .map(|b| format!("{:02X}", b))
            .collect();
        format!("{}:{}:{}:{}", kind, email, exp, sig)
    }

    /// Verify kind, expiry and HMAC. Returns the email on success.
    /// `max_future_skew_secs` caps tokens minted with far-future expiries.
    pub fn verify(&self, kind: &str, token: &str, now_unix: i64, max_ttl_secs: i64) -> Option<String> {
        let mut parts = token.splitn(4, ':');
        let t_kind = parts.next()?;
        let email = parts.next()?;
        let exp_str = parts.next()?;
        let sig_hex = parts.next()?;
        if t_kind != kind || email.is_empty() || !email.contains('@') {
            return None;
        }
        let exp: i64 = exp_str.parse().ok()?;
        if exp <= now_unix || exp - now_unix > max_ttl_secs {
            return None;
        }
        let expected = self.hmac(kind, &email.to_lowercase(), exp);
        let presented = hex_decode(sig_hex)?;
        if presented.len() != expected.len() {
            return None;
        }
        use subtle::ConstantTimeEq;
        if presented.as_slice().ct_eq(expected.as_slice()).unwrap_u8() == 1 {
            Some(email.to_lowercase())
        } else {
            None
        }
    }
}

/// Minimal HMAC-SHA256 (avoids a new crate for 15 lines of standard code).
fn hmac_sha256(key: &[u8], msg: &[u8]) -> Vec<u8> {
    const BLOCK: usize = 64;
    let mut kpad = [0u8; BLOCK];
    if key.len() > BLOCK {
        let mut h = Sha256::new();
        h.update(key);
        let d = h.finalize();
        kpad[..d.len()].copy_from_slice(&d);
    } else {
        kpad[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; BLOCK];
    let mut opad = [0x5cu8; BLOCK];
    for i in 0..BLOCK {
        ipad[i] ^= kpad[i];
        opad[i] ^= kpad[i];
    }
    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(msg);
    let inner_digest = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_digest);
    outer.finalize().to_vec()
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let hi = (b[i] as char).to_digit(16)?;
        let lo = (b[i + 1] as char).to_digit(16)?;
        out.push((hi as u8) * 16 + (lo as u8));
        i += 2;
    }
    Some(out)
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

    #[test]
    fn session_tokens_roundtrip_and_reject() {
        let toks = SessionTokens::new(b"test-secret-32-bytes-long-enough!!".to_vec());
        let now = 1_700_000_000i64;
        let t = toks.mint("ceo", "CEO@Test.Ph", 3600, now);
        assert!(t.starts_with("ceo:ceo@test.ph:"));
        // valid
        assert_eq!(toks.verify("ceo", &t, now + 10, 7200), Some("ceo@test.ph".to_string()));
        // wrong kind
        assert!(toks.verify("owner", &t, now + 10, 7200).is_none());
        // expired
        assert!(toks.verify("ceo", &t, now + 3601, 7200).is_none());
        // over max TTL
        assert!(toks.verify("ceo", &t, now + 10, 60).is_none());
        // tampered email
        let bad = t.replacen("ceo@test.ph", "evil@x.ph", 1);
        assert!(toks.verify("ceo", &bad, now + 10, 7200).is_none());
        // tampered signature
        let mut bad2 = t.clone();
        bad2.pop();
        bad2.push('0');
        assert!(toks.verify("ceo", &bad2, now + 10, 7200).is_none());
        // legacy forgeable format rejected
        assert!(toks.verify("ceo", "ceo:ceo@test.ph", now + 10, 7200).is_none());
        // wrong secret
        let other = SessionTokens::new(b"other-secret-32-bytes-long-enough!".to_vec());
        assert!(other.verify("ceo", &t, now + 10, 7200).is_none());
    }

    #[test]
    fn hmac_matches_rfc4231_vector() {
        // RFC 4231 test case 1: key = 20x 0x0b, data = "Hi There"
        let key = vec![0x0bu8; 20];
        let got = hmac_sha256(&key, b"Hi There");
        let want = "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7";
        let got_hex: String = got.iter().map(|b| format!("{:02x}", b)).collect();
        assert_eq!(got_hex, want);
    }
}
