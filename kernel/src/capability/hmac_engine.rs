//! HMAC-SHA3-256 engine for capability token signing and verification.
//!
//! The HMAC key is derived at boot from a fixed seed XORed with RDTSC entropy.
//! The key NEVER leaves Ring 0. Wire tokens are validated by ID lookup in the
//! capability store, which holds the HMAC alongside each token.

#![allow(dead_code)]

use hmac::{Hmac, Mac};
use sha3::Sha3_256;
use spin::Mutex;

type HmacSha3 = Hmac<Sha3_256>;

/// Kernel-secret HMAC key (32 bytes).
/// Initialized once at boot via `init()`. Zero until then.
static HMAC_KEY: Mutex<[u8; 32]> = Mutex::new([0u8; 32]);

/// Read the CPU timestamp counter for boot-time entropy.
fn rdtsc() -> u64 {
    let lo: u32;
    let hi: u32;
    unsafe {
        core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi, options(nostack, nomem));
    }
    ((hi as u64) << 32) | (lo as u64)
}

/// Initialize the HMAC key from a fixed seed XORed with RDTSC entropy.
/// Must be called once during boot, after serial init.
pub fn init() {
    let tsc = rdtsc();
    let seed: [u8; 32] = *b"FabricOS_CapKey_Phase1_v1.0!!!!_";
    let tsc_bytes = tsc.to_le_bytes();

    let mut key = [0u8; 32];
    for i in 0..32 {
        key[i] = seed[i] ^ tsc_bytes[i % 8];
    }

    *HMAC_KEY.lock() = key;
}

/// Sign a byte slice, returning 32-byte HMAC-SHA3-256.
pub fn sign(data: &[u8]) -> [u8; 32] {
    let key = HMAC_KEY.lock();
    let mut mac = HmacSha3::new_from_slice(&*key).expect("HMAC key length invalid");
    mac.update(data);
    let result = mac.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&result.into_bytes());
    out
}

/// Verify a byte slice against an expected 32-byte HMAC.
/// Returns true if the HMAC matches, false if tampered.
pub fn verify(data: &[u8], expected: &[u8; 32]) -> bool {
    let key = HMAC_KEY.lock();
    let mut mac = HmacSha3::new_from_slice(&*key).expect("HMAC key length invalid");
    mac.update(data);
    mac.verify_slice(expected).is_ok()
}
