//! Deprecated shim — canonical type is `gympos_shared::LicenseClaims`.
//! This file is kept for Gemini's directory scan parity but re-exports the shared type.
//! All logic lives in `cloud/src/routes.rs` + `cloud/src/crypto.rs`.

pub use gympos_shared::{LicenseClaims, LicenseStatus, LicenseTier};

// HWID-bound claims are: hwid, ip_hint, exp_unix, grace_until (see shared::LicenseClaims)
// issue:   sign claims with RSA_PRIVATE_KEY_PEM via crypto::LicenseSigner, store in SQLite cloud_licenses
// verify:  crypto::verify_license_token + claims.evaluate() + 3d grace + 7d heartbeat (db last_verify)
// revoke:  cloud_licenses.is_revoked=1, revoked_licenses RwLock, remote_disable push
