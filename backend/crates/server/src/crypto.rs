#![allow(deprecated)] // aes-gcm still uses the generic-array 0.14 helpers (`Nonce::from_slice`)
//! Cryptographic helpers: key derivation, secret encryption at rest, signed session tokens,
//! password hashing.

use aes_gcm::aead::{Aead, KeyInit as _};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use base64::Engine;
use hkdf::Hkdf;
use hmac::{Hmac, KeyInit as _, Mac};
use rand::RngCore;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;
const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::URL_SAFE_NO_PAD;

#[derive(Clone)]
pub struct Keys {
    session: [u8; 32],
    enc: [u8; 32],
}

impl Keys {
    /// Derive independent sub-keys from the application secret (HKDF-SHA256).
    pub fn derive(app_secret: &str) -> Self {
        let hk = Hkdf::<Sha256>::new(Some(b"infra-architect/v1"), app_secret.as_bytes());
        let mut session = [0u8; 32];
        let mut enc = [0u8; 32];
        hk.expand(b"session-signing", &mut session).expect("32 bytes");
        hk.expand(b"settings-encryption", &mut enc).expect("32 bytes");
        Keys { session, enc }
    }

    pub fn encrypt(&self, plaintext: &str) -> String {
        let cipher = Aes256Gcm::new_from_slice(&self.enc).expect("key length");
        let mut nonce = [0u8; 12];
        OsRng.fill_bytes(&mut nonce);
        let ct = cipher.encrypt(Nonce::from_slice(&nonce), plaintext.as_bytes()).expect("encrypt");
        let mut out = nonce.to_vec();
        out.extend(ct);
        format!("v1:{}", B64.encode(out))
    }

    pub fn decrypt(&self, encoded: &str) -> Option<String> {
        let raw = B64.decode(encoded.strip_prefix("v1:")?).ok()?;
        if raw.len() < 13 {
            return None;
        }
        let (nonce, ct) = raw.split_at(12);
        let cipher = Aes256Gcm::new_from_slice(&self.enc).ok()?;
        let pt = cipher.decrypt(Nonce::from_slice(nonce), ct).ok()?;
        String::from_utf8(pt).ok()
    }

    /// `base64url(payload).base64url(hmac)`
    pub fn sign(&self, payload: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(&self.session).expect("hmac key");
        mac.update(payload);
        format!("{}.{}", B64.encode(payload), B64.encode(mac.finalize().into_bytes()))
    }

    pub fn verify(&self, token: &str) -> Option<Vec<u8>> {
        let (p, s) = token.split_once('.')?;
        let payload = B64.decode(p).ok()?;
        let sig = B64.decode(s).ok()?;
        let mut mac = HmacSha256::new_from_slice(&self.session).ok()?;
        mac.update(&payload);
        mac.verify_slice(&sig).ok()?; // constant time
        Some(payload)
    }
}

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default().hash_password(password.as_bytes(), &salt).map(|h| h.to_string()).map_err(|e| anyhow::anyhow!("hash error: {e}"))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(h) => Argon2::default().verify_password(password.as_bytes(), &h).is_ok(),
        Err(_) => false,
    }
}

/// A random password for bootstrap/reset flows (shown once).
pub fn random_password() -> String {
    let mut b = [0u8; 18];
    OsRng.fill_bytes(&mut b);
    B64.encode(b)
}

pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    let mut h = Sha256::new();
    h.update(data);
    h.finalize().iter().map(|b| format!("{:02x}", b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_roundtrip_and_tamper() {
        let k = Keys::derive("0123456789abcdef0123456789abcdef");
        let c = k.encrypt("sk-or-v1-secret");
        assert!(c.starts_with("v1:") && !c.contains("secret"));
        assert_eq!(k.decrypt(&c).as_deref(), Some("sk-or-v1-secret"));
        let other = Keys::derive("ffffffffffffffffffffffffffffffff");
        assert!(other.decrypt(&c).is_none(), "different secret must not decrypt");
        assert!(k.decrypt(&format!("{}A", c)).is_none(), "tampered ciphertext must fail");
    }

    #[test]
    fn signed_tokens_verify_only_untampered() {
        let k = Keys::derive("0123456789abcdef0123456789abcdef");
        let t = k.sign(b"{\"u\":1}");
        assert_eq!(k.verify(&t).as_deref(), Some(&b"{\"u\":1}"[..]));
        let forged = format!("{}.{}", B64.encode(b"{\"u\":2}"), t.split_once('.').unwrap().1);
        assert!(k.verify(&forged).is_none());
    }

    // Known answers, computed outside this code (Python's hmac/hashlib for the keys, the signature and the digest, the `cryptography` package for AES-GCM).
    // They are what keeps a dependency upgrade (the RustCrypto crates change generation together) from silently invalidating what is already stored:
    // every session cookie that was issued and every secret that was encrypted with the keys derived from APP_SECRET.
    const SECRET: &str = "0123456789abcdef0123456789abcdef";

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn keys_derive_to_the_known_values() {
        let k = Keys::derive(SECRET);
        assert_eq!(hex(&k.session), "4594c1950ccbe6c694d9bc0e22f91bd2fda7cb566ceb9bf2145fafaa3ca83f0b", "HKDF-SHA256, info \"session-signing\"");
        assert_eq!(hex(&k.enc), "6e07f673c8075fd1efd8192637f74f578ec40d19603a090bab3a3356d434fedb", "HKDF-SHA256, info \"settings-encryption\"");
    }

    #[test]
    fn a_session_token_issued_before_still_verifies_and_signing_is_unchanged() {
        let k = Keys::derive(SECRET);
        let issued = "eyJ1IjoxfQ.SyfAWSsqa4AtnH12H-1oHOAyszU-IhYGf3LJCEdJBB0";
        assert_eq!(k.verify(issued).as_deref(), Some(&b"{\"u\":1}"[..]));
        assert_eq!(k.sign(b"{\"u\":1}"), issued, "HMAC-SHA256 over the payload with the session key");
    }

    #[test]
    fn a_secret_encrypted_before_still_decrypts() {
        let k = Keys::derive(SECRET);
        assert_eq!(k.decrypt("v1:AQIDBAUGBwgJCgsMkRZQKoDVQzbYy5xl17SHMJg0_3ZRf-HbwVtaPnKncw").as_deref(), Some("sk-or-v1-secret"), "AES-256-GCM, 12-byte nonce first");
    }

    #[test]
    fn sha256_hex_is_the_standard_digest() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    #[test]
    fn passwords_hash_and_verify() {
        let h = hash_password("correct horse battery").unwrap();
        assert!(verify_password("correct horse battery", &h));
        assert!(!verify_password("wrong", &h));
        assert!(!verify_password("x", "not-a-hash"));
    }
}
