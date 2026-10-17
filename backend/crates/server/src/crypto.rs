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
use prometheus::IntCounter;
use rand::RngCore;
use sha2::Sha256;
use std::sync::{Arc, OnceLock};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

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

/// A hash with the parameters of a real one, for a password nobody knows: what a sign-in verifies against when the account is not there, so that
/// an account that does not exist costs what one that does exist costs. Made from [`hash_password`], so it follows the parameters of the day: a literal
/// in the source would one day (when the parameters are raised) be a cheaper hash than the real ones, or one that does not parse and costs nothing.
pub fn dummy_hash() -> &'static str {
    static DUMMY: OnceLock<String> = OnceLock::new();
    DUMMY.get_or_init(|| hash_password(&random_password()).expect("the system can hash a password"))
}

/// Where password hashing runs: on the blocking pool (an Argon2 hash is tens of milliseconds of processor and 19 MiB of memory, and on an async worker it
/// would hold the thread, with every other request that thread serves), through a semaphore, so that the number of hashes at the same moment is bounded
/// by what the process has memory and cores for instead of by how many requests came. The permit is taken **before** the blocking pool is asked: the pool
/// has hundreds of threads, and asking first would make the bound meaningless. It travels with the work, so a client that goes away does not free a slot
/// for a hash that is still running.
pub struct PasswordGate {
    slots: Arc<Semaphore>,
    checks: IntCounter,
}

impl PasswordGate {
    /// A gate of `slots` hashes at a time (at least one); `checks` counts every verification (`auth_password_checks_total`).
    pub fn new(slots: usize, checks: IntCounter) -> Self {
        PasswordGate { slots: Arc::new(Semaphore::new(slots.max(1))), checks }
    }

    /// Run `work` on the blocking pool when a slot is free. `Err` if the work panicked.
    pub async fn run<T: Send + 'static>(&self, work: impl FnOnce() -> T + Send + 'static) -> anyhow::Result<T> {
        let permit = self.slots.clone().acquire_owned().await.map_err(|_| anyhow::anyhow!("the password gate is closed"))?;
        tokio::task::spawn_blocking(move || {
            let out = work();
            drop(permit);
            out
        })
        .await
        .map_err(|e| anyhow::anyhow!("password work failed: {e}"))
    }

    /// [`verify_password`] through the gate. A hash that does not parse, or work that failed, is a password that does not match.
    pub async fn verify(&self, password: String, hash: String) -> bool {
        let checks = self.checks.clone();
        self.run(move || {
            checks.inc();
            verify_password(&password, &hash)
        })
        .await
        .unwrap_or(false)
    }

    /// [`hash_password`] through the gate.
    pub async fn hash(&self, password: String) -> anyhow::Result<String> {
        self.run(move || hash_password(&password)).await?
    }

    /// Take a slot and keep it until the permit is dropped: the gate is full for everybody else meanwhile.
    pub async fn reserve(&self) -> OwnedSemaphorePermit {
        self.slots.clone().acquire_owned().await.expect("the password gate is open")
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

    fn argon2_parameters(hash: &str) -> (String, Option<argon2::password_hash::Decimal>, argon2::Params, usize) {
        let h = PasswordHash::new(hash).expect("a hash that parses");
        (h.algorithm.to_string(), h.version, argon2::Params::try_from(&h).expect("argon2 parameters"), h.hash.expect("an output").len())
    }

    #[test]
    fn dummy_hash_has_the_parameters_of_a_real_one() {
        // what makes an unknown account cost what a known one costs is that the hash it is checked against is as expensive, which is a matter of its
        // parameters (algorithm, version, memory, passes, lanes, length of the output): compared, not timed
        let real = hash_password("a real password").unwrap();
        let dummy = dummy_hash();
        assert_eq!(argon2_parameters(dummy), argon2_parameters(&real));
        assert_eq!(dummy, dummy_hash(), "made once");
        assert!(!verify_password("a real password", dummy) && !verify_password("", dummy), "and nobody's password");
    }

    fn gate(slots: usize) -> PasswordGate {
        PasswordGate::new(slots, IntCounter::new("checks", "checks").unwrap())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn password_gate_never_runs_more_than_n_hashes() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let gate = Arc::new(gate(2));
        let (now, peak) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let tasks: Vec<_> = (0..6)
            .map(|_| {
                let (gate, now, peak) = (gate.clone(), now.clone(), peak.clone());
                tokio::spawn(async move {
                    gate.run(move || {
                        let running = now.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(running, Ordering::SeqCst);
                        std::thread::sleep(std::time::Duration::from_millis(40));
                        now.fetch_sub(1, Ordering::SeqCst);
                    })
                    .await
                    .unwrap()
                })
            })
            .collect();
        for t in tasks {
            t.await.unwrap();
        }
        assert_eq!(peak.load(Ordering::SeqCst), 2, "six at once through a gate of two: two at a time, and two really at the same time");
    }

    #[tokio::test]
    async fn password_gate_work_runs_off_the_runtime_thread() {
        // the runtime of this test has one thread. The work waits for a task of the runtime to run: it can only see it run if the work is not on that thread
        use std::sync::atomic::{AtomicBool, Ordering};
        let flag = Arc::new(AtomicBool::new(false));
        let setter = flag.clone();
        tokio::spawn(async move { setter.store(true, Ordering::SeqCst) });
        let seen = gate(1)
            .run(move || {
                let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
                while !flag.load(Ordering::SeqCst) && std::time::Instant::now() < until {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                flag.load(Ordering::SeqCst)
            })
            .await
            .unwrap();
        assert!(seen, "hashing blocked the thread the runtime runs its tasks on");
    }

    #[tokio::test]
    async fn password_gate_checks_count_and_a_held_slot_holds_the_work() {
        let g = Arc::new(gate(1));
        let hash = g.hash("right password".into()).await.unwrap();
        assert_eq!(g.checks.get(), 0, "hashing is not a check");
        assert!(g.verify("right password".into(), hash.clone()).await);
        assert!(!g.verify("wrong password".into(), hash.clone()).await);
        assert!(!g.verify("x".into(), "not-a-hash".into()).await, "a hash that does not parse is a password that does not match");
        assert_eq!(g.checks.get(), 3);
        // while somebody holds the only slot, a check waits for it
        let held = g.reserve().await;
        let waiting = tokio::spawn({
            let g = g.clone();
            async move { g.verify("right password".into(), hash).await }
        });
        for _ in 0..50 {
            tokio::task::yield_now().await;
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        assert!(!waiting.is_finished() && g.checks.get() == 3, "the check did not start without a slot");
        drop(held);
        assert!(waiting.await.unwrap());
        assert_eq!(g.checks.get(), 4);
    }

    #[test]
    fn passwords_hash_and_verify() {
        let h = hash_password("correct horse battery").unwrap();
        assert!(verify_password("correct horse battery", &h));
        assert!(!verify_password("wrong", &h));
        assert!(!verify_password("x", "not-a-hash"));
    }
}
