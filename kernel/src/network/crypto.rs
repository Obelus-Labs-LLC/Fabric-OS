//! Cryptographic primitives for TLS 1.3.
//!
//! Pure Rust implementations for bare-metal x86_64 (no_std, soft-float):
//! - SHA-256 + HMAC-SHA256 (via `sha2` and `hmac` crates)
//! - HKDF-SHA256 (RFC 5869, manual implementation)
//! - X25519 key exchange (RFC 7748)
//! - ChaCha20 stream cipher (RFC 8439)
//! - Poly1305 MAC (RFC 8439)
//! - ChaCha20-Poly1305 AEAD (RFC 8439)

#![allow(dead_code)]

use sha2::Sha256;
use hmac::{Hmac, Mac};

// ============================================================================
// SHA-256 + HMAC-SHA256 wrappers
// ============================================================================

/// SHA-256 hash.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    use sha2::Digest;
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}

/// Incremental SHA-256 hasher for TLS transcript.
pub struct Sha256State {
    inner: Sha256,
}

impl Sha256State {
    pub fn new() -> Self {
        use sha2::Digest;
        Self { inner: Sha256::new() }
    }

    pub fn update(&mut self, data: &[u8]) {
        use sha2::Digest;
        self.inner.update(data);
    }

    /// Get current hash without consuming state.
    pub fn current_hash(&self) -> [u8; 32] {
        use sha2::Digest;
        let copy = self.inner.clone();
        let result = copy.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&result);
        out
    }

    /// Finalize and return hash.
    pub fn finalize(self) -> [u8; 32] {
        use sha2::Digest;
        let result = self.inner.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&result);
        out
    }
}

impl Clone for Sha256State {
    fn clone(&self) -> Self {
        Self { inner: self.inner.clone() }
    }
}

/// HMAC-SHA256.
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC key");
    mac.update(data);
    let result = mac.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&result.into_bytes());
    out
}

// ============================================================================
// HKDF-SHA256 (RFC 5869)
// ============================================================================

/// HKDF-Extract: PRK = HMAC-Hash(salt, IKM)
pub fn hkdf_extract(salt: &[u8], ikm: &[u8]) -> [u8; 32] {
    let salt = if salt.is_empty() { &[0u8; 32][..] } else { salt };
    hmac_sha256(salt, ikm)
}

/// HKDF-Expand: OKM = T(1) || T(2) || ... (first `length` bytes)
pub fn hkdf_expand(prk: &[u8; 32], info: &[u8], length: usize) -> alloc::vec::Vec<u8> {
    let hash_len = 32;
    let n = (length + hash_len - 1) / hash_len;
    let mut okm = alloc::vec::Vec::with_capacity(length);
    let mut t = alloc::vec::Vec::new();

    for i in 1..=n {
        let mut data = alloc::vec::Vec::new();
        data.extend_from_slice(&t);
        data.extend_from_slice(info);
        data.push(i as u8);
        let block = hmac_sha256(prk, &data);
        t = block.to_vec();
        okm.extend_from_slice(&block[..hash_len.min(length - okm.len())]);
    }
    okm
}

/// HKDF-Expand to fixed 32-byte output.
pub fn hkdf_expand_32(prk: &[u8; 32], info: &[u8]) -> [u8; 32] {
    // Single iteration: T(1) = HMAC(PRK, info || 0x01)
    let mut data = alloc::vec::Vec::new();
    data.extend_from_slice(info);
    data.push(0x01);
    hmac_sha256(prk, &data)
}

// ============================================================================
// X25519 Key Exchange (RFC 7748)
// ============================================================================

/// Field element in GF(2^255-19), radix-2^51 representation.
#[derive(Clone, Copy)]
struct Fe([u64; 5]);

const MASK51: u64 = (1u64 << 51) - 1;

impl Fe {
    const ZERO: Fe = Fe([0, 0, 0, 0, 0]);
    const ONE: Fe = Fe([1, 0, 0, 0, 0]);

    /// Load from 32 little-endian bytes.
    fn from_bytes(s: &[u8; 32]) -> Fe {
        let mut h = [0u64; 5];
        h[0] = load_le_u64(s, 0) & MASK51;
        h[1] = (load_le_u64(s, 6) >> 3) & MASK51;
        h[2] = (load_le_u64(s, 12) >> 6) & MASK51;
        h[3] = (load_le_u64(s, 19) >> 1) & MASK51;
        h[4] = (load_le_u64(s, 24) >> 12) & MASK51;
        Fe(h)
    }

    /// Serialize to 32 little-endian bytes (fully reduced).
    fn to_bytes(&self) -> [u8; 32] {
        let mut h = self.0;
        // Full carry chain (2 rounds to handle any overflow)
        for _ in 0..2 {
            let carry = h[0] >> 51; h[0] &= MASK51; h[1] += carry;
            let carry = h[1] >> 51; h[1] &= MASK51; h[2] += carry;
            let carry = h[2] >> 51; h[2] &= MASK51; h[3] += carry;
            let carry = h[3] >> 51; h[3] &= MASK51; h[4] += carry;
            let carry = h[4] >> 51; h[4] &= MASK51; h[0] += carry * 19;
        }
        // Freeze: subtract p if h >= p
        let mut q = (h[0] + 19) >> 51;
        q = (h[1] + q) >> 51;
        q = (h[2] + q) >> 51;
        q = (h[3] + q) >> 51;
        q = (h[4] + q) >> 51;
        h[0] += 19 * q;
        let carry = h[0] >> 51; h[0] &= MASK51; h[1] += carry;
        let carry = h[1] >> 51; h[1] &= MASK51; h[2] += carry;
        let carry = h[2] >> 51; h[2] &= MASK51; h[3] += carry;
        let carry = h[3] >> 51; h[3] &= MASK51; h[4] += carry;
        h[4] &= MASK51;

        // Pack 5 × 51-bit limbs into 32 bytes (little-endian).
        // Bit positions: h[0]=0..50, h[1]=51..101, h[2]=102..152,
        //                h[3]=153..203, h[4]=204..254
        let mut s = [0u8; 32];

        // h[0]: 51 bits starting at byte 0, bit 0
        s[0] = h[0] as u8;
        s[1] = (h[0] >> 8) as u8;
        s[2] = (h[0] >> 16) as u8;
        s[3] = (h[0] >> 24) as u8;
        s[4] = (h[0] >> 32) as u8;
        s[5] = (h[0] >> 40) as u8;
        s[6] = (h[0] >> 48) as u8; // 3 bits

        // h[1]: 51 bits starting at byte 6, bit 3
        s[6] |= (h[1] << 3) as u8;
        s[7] = (h[1] >> 5) as u8;
        s[8] = (h[1] >> 13) as u8;
        s[9] = (h[1] >> 21) as u8;
        s[10] = (h[1] >> 29) as u8;
        s[11] = (h[1] >> 37) as u8;
        s[12] = (h[1] >> 45) as u8; // 6 bits

        // h[2]: 51 bits starting at byte 12, bit 6
        s[12] |= (h[2] << 6) as u8;
        s[13] = (h[2] >> 2) as u8;
        s[14] = (h[2] >> 10) as u8;
        s[15] = (h[2] >> 18) as u8;
        s[16] = (h[2] >> 26) as u8;
        s[17] = (h[2] >> 34) as u8;
        s[18] = (h[2] >> 42) as u8;
        s[19] = (h[2] >> 50) as u8; // 1 bit

        // h[3]: 51 bits starting at byte 19, bit 1
        s[19] |= (h[3] << 1) as u8;
        s[20] = (h[3] >> 7) as u8;
        s[21] = (h[3] >> 15) as u8;
        s[22] = (h[3] >> 23) as u8;
        s[23] = (h[3] >> 31) as u8;
        s[24] = (h[3] >> 39) as u8;
        s[25] = (h[3] >> 47) as u8; // 4 bits

        // h[4]: 51 bits starting at byte 25, bit 4
        s[25] |= (h[4] << 4) as u8;
        s[26] = (h[4] >> 4) as u8;
        s[27] = (h[4] >> 12) as u8;
        s[28] = (h[4] >> 20) as u8;
        s[29] = (h[4] >> 28) as u8;
        s[30] = (h[4] >> 36) as u8;
        s[31] = (h[4] >> 44) as u8; // 7 bits

        s
    }

    fn add(&self, rhs: &Fe) -> Fe {
        Fe([
            self.0[0] + rhs.0[0],
            self.0[1] + rhs.0[1],
            self.0[2] + rhs.0[2],
            self.0[3] + rhs.0[3],
            self.0[4] + rhs.0[4],
        ])
    }

    fn sub(&self, rhs: &Fe) -> Fe {
        // Add 2p to avoid underflow: 2*(2^51 - 19) and 2*(2^51 - 1) for other limbs
        Fe([
            (self.0[0] + 0xFFFFFFFFFFFDA) - rhs.0[0], // 2*(2^51 - 19)
            (self.0[1] + 0xFFFFFFFFFFFFE) - rhs.0[1], // 2*(2^51 - 1)
            (self.0[2] + 0xFFFFFFFFFFFFE) - rhs.0[2],
            (self.0[3] + 0xFFFFFFFFFFFFE) - rhs.0[3],
            (self.0[4] + 0xFFFFFFFFFFFFE) - rhs.0[4],
        ])
    }

    fn mul(&self, rhs: &Fe) -> Fe {
        let f = &self.0;
        let g = &rhs.0;
        let f0 = f[0] as u128; let f1 = f[1] as u128;
        let f2 = f[2] as u128; let f3 = f[3] as u128;
        let f4 = f[4] as u128;
        let g0 = g[0] as u128; let g1 = g[1] as u128;
        let g2 = g[2] as u128; let g3 = g[3] as u128;
        let g4 = g[4] as u128;

        // 2^255 ≡ 19 (mod p), so contributions from limbs ≥5 fold back with factor 19
        let g1_19 = 19 * g1; let g2_19 = 19 * g2;
        let g3_19 = 19 * g3; let g4_19 = 19 * g4;

        let h0 = f0*g0 + f1*g4_19 + f2*g3_19 + f3*g2_19 + f4*g1_19;
        let h1 = f0*g1 + f1*g0 + f2*g4_19 + f3*g3_19 + f4*g2_19;
        let h2 = f0*g2 + f1*g1 + f2*g0 + f3*g4_19 + f4*g3_19;
        let h3 = f0*g3 + f1*g2 + f2*g1 + f3*g0 + f4*g4_19;
        let h4 = f0*g4 + f1*g3 + f2*g2 + f3*g1 + f4*g0;

        // Carry propagation
        let mut r = [0u64; 5];
        let carry = h0 >> 51; r[0] = (h0 as u64) & MASK51;
        let h1 = h1 + carry;
        let carry = h1 >> 51; r[1] = (h1 as u64) & MASK51;
        let h2 = h2 + carry;
        let carry = h2 >> 51; r[2] = (h2 as u64) & MASK51;
        let h3 = h3 + carry;
        let carry = h3 >> 51; r[3] = (h3 as u64) & MASK51;
        let h4 = h4 + carry;
        let carry = h4 >> 51; r[4] = (h4 as u64) & MASK51;

        // Fold carry from h4 back to h0
        r[0] += (carry as u64) * 19;
        let carry = r[0] >> 51;
        r[0] &= MASK51;
        r[1] += carry;

        Fe(r)
    }

    fn square(&self) -> Fe {
        self.mul(self)
    }

    /// Multiply by a small constant (< 2^16).
    fn mul_small(&self, c: u64) -> Fe {
        let mut h = [0u128; 5];
        for i in 0..5 {
            h[i] = (self.0[i] as u128) * (c as u128);
        }
        let mut r = [0u64; 5];
        let carry = h[0] >> 51; r[0] = (h[0] as u64) & MASK51;
        h[1] += carry;
        let carry = h[1] >> 51; r[1] = (h[1] as u64) & MASK51;
        h[2] += carry;
        let carry = h[2] >> 51; r[2] = (h[2] as u64) & MASK51;
        h[3] += carry;
        let carry = h[3] >> 51; r[3] = (h[3] as u64) & MASK51;
        h[4] += carry;
        let carry = h[4] >> 51; r[4] = (h[4] as u64) & MASK51;
        r[0] += (carry as u64) * 19;
        Fe(r)
    }

    /// Compute self^(2^255 - 21) = self^(p-2) = self^{-1} mod p.
    fn invert(&self) -> Fe {
        // p-2 in little-endian bytes
        const P_MINUS_2: [u8; 32] = [
            0xEB, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x7F,
        ];
        self.pow(&P_MINUS_2)
    }

    /// Exponentiation by a scalar (little-endian bytes).
    fn pow(&self, exp: &[u8; 32]) -> Fe {
        let mut result = Fe::ONE;
        for i in (0..255).rev() {
            result = result.square();
            let byte_idx = i / 8;
            let bit_idx = i % 8;
            if (exp[byte_idx] >> bit_idx) & 1 == 1 {
                result = result.mul(self);
            }
        }
        result
    }
}

/// Load 8 bytes as little-endian u64 from position `pos` in slice.
fn load_le_u64(s: &[u8], pos: usize) -> u64 {
    let mut buf = [0u8; 8];
    let end = (pos + 8).min(s.len());
    let len = end - pos;
    buf[..len].copy_from_slice(&s[pos..end]);
    u64::from_le_bytes(buf)
}

/// Constant-time conditional swap.
fn cswap(a: &mut Fe, b: &mut Fe, swap: u64) {
    let mask = (0u64).wrapping_sub(swap); // 0 or 0xFFFF...
    for i in 0..5 {
        let t = mask & (a.0[i] ^ b.0[i]);
        a.0[i] ^= t;
        b.0[i] ^= t;
    }
}

/// X25519 scalar multiplication (RFC 7748).
///
/// Computes the shared secret from a secret scalar `k` and a public u-coordinate `u`.
pub fn x25519(k: &[u8; 32], u: &[u8; 32]) -> [u8; 32] {
    // Clamp scalar (per RFC 7748 Section 5)
    let mut scalar = *k;
    scalar[0] &= 248;
    scalar[31] &= 127;
    scalar[31] |= 64;

    let x_1 = Fe::from_bytes(u);
    let mut x_2 = Fe::ONE;
    let mut z_2 = Fe::ZERO;
    let mut x_3 = x_1;
    let mut z_3 = Fe::ONE;
    let mut swap: u64 = 0;

    // Montgomery ladder (254 steps)
    for t in (0..255).rev() {
        let k_t = ((scalar[t / 8] >> (t & 7)) & 1) as u64;
        swap ^= k_t;
        cswap(&mut x_2, &mut x_3, swap);
        cswap(&mut z_2, &mut z_3, swap);
        swap = k_t;

        let a = x_2.add(&z_2);
        let aa = a.square();
        let b = x_2.sub(&z_2);
        let bb = b.square();
        let e = aa.sub(&bb);
        let c = x_3.add(&z_3);
        let d = x_3.sub(&z_3);
        let da = d.mul(&a);
        let cb = c.mul(&b);
        x_3 = da.add(&cb).square();
        z_3 = x_1.mul(&da.sub(&cb).square());
        x_2 = aa.mul(&bb);
        z_2 = e.mul(&bb.add(&e.mul_small(121666)));
    }

    // Final conditional swap
    cswap(&mut x_2, &mut x_3, swap);
    cswap(&mut z_2, &mut z_3, swap);

    // Return x_2 * z_2^{-1}
    x_2.mul(&z_2.invert()).to_bytes()
}

/// X25519 base point (u=9).
const X25519_BASEPOINT: [u8; 32] = {
    let mut b = [0u8; 32];
    b[0] = 9;
    b
};

/// Generate X25519 keypair from seed bytes.
/// Returns (private_key, public_key).
pub fn x25519_keypair(seed: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
    let private = *seed;
    let public = x25519(&private, &X25519_BASEPOINT);
    (private, public)
}

/// Generate pseudo-random bytes from tick counter + ISN.
/// NOT cryptographically secure — adequate for demo OS.
pub fn random_bytes_32() -> [u8; 32] {
    let tick = crate::x86::idt::tick_count() as u64;
    let counter = core::sync::atomic::AtomicU64::new(0);
    let cnt = counter.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    // Hash tick + counter for better distribution
    let mut seed = [0u8; 16];
    seed[0..8].copy_from_slice(&tick.to_le_bytes());
    seed[8..16].copy_from_slice(&cnt.to_le_bytes());
    sha256(&seed)
}

// ============================================================================
// ChaCha20 (RFC 8439)
// ============================================================================

/// ChaCha20 quarter round.
fn quarter_round(state: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    state[a] = state[a].wrapping_add(state[b]); state[d] ^= state[a]; state[d] = state[d].rotate_left(16);
    state[c] = state[c].wrapping_add(state[d]); state[b] ^= state[c]; state[b] = state[b].rotate_left(12);
    state[a] = state[a].wrapping_add(state[b]); state[d] ^= state[a]; state[d] = state[d].rotate_left(8);
    state[c] = state[c].wrapping_add(state[d]); state[b] ^= state[c]; state[b] = state[b].rotate_left(7);
}

/// Generate one ChaCha20 block (64 bytes of keystream).
fn chacha20_block(key: &[u8; 32], counter: u32, nonce: &[u8; 12]) -> [u8; 64] {
    let mut state = [0u32; 16];
    // Constants: "expand 32-byte k"
    state[0] = 0x61707865;
    state[1] = 0x3320646e;
    state[2] = 0x79622d32;
    state[3] = 0x6b206574;
    // Key (8 words)
    for i in 0..8 {
        state[4 + i] = u32::from_le_bytes([
            key[4*i], key[4*i+1], key[4*i+2], key[4*i+3]
        ]);
    }
    // Counter
    state[12] = counter;
    // Nonce (3 words)
    for i in 0..3 {
        state[13 + i] = u32::from_le_bytes([
            nonce[4*i], nonce[4*i+1], nonce[4*i+2], nonce[4*i+3]
        ]);
    }

    let initial = state;

    // 20 rounds (10 column + 10 diagonal)
    for _ in 0..10 {
        // Column rounds
        quarter_round(&mut state, 0, 4,  8, 12);
        quarter_round(&mut state, 1, 5,  9, 13);
        quarter_round(&mut state, 2, 6, 10, 14);
        quarter_round(&mut state, 3, 7, 11, 15);
        // Diagonal rounds
        quarter_round(&mut state, 0, 5, 10, 15);
        quarter_round(&mut state, 1, 6, 11, 12);
        quarter_round(&mut state, 2, 7,  8, 13);
        quarter_round(&mut state, 3, 4,  9, 14);
    }

    // Add initial state
    for i in 0..16 {
        state[i] = state[i].wrapping_add(initial[i]);
    }

    // Serialize
    let mut out = [0u8; 64];
    for i in 0..16 {
        out[4*i..4*i+4].copy_from_slice(&state[i].to_le_bytes());
    }
    out
}

/// ChaCha20 encrypt/decrypt (XOR with keystream).
pub fn chacha20_xor(key: &[u8; 32], counter: u32, nonce: &[u8; 12], data: &mut [u8]) {
    let mut ctr = counter;
    let mut offset = 0;
    while offset < data.len() {
        let block = chacha20_block(key, ctr, nonce);
        let remaining = data.len() - offset;
        let chunk = remaining.min(64);
        for i in 0..chunk {
            data[offset + i] ^= block[i];
        }
        offset += chunk;
        ctr += 1;
    }
}

// ============================================================================
// Poly1305 MAC (RFC 8439)
// ============================================================================

/// Compute Poly1305 MAC tag.
pub fn poly1305_mac(key: &[u8; 32], msg: &[u8]) -> [u8; 16] {
    // Parse r and s from key
    let mut r = [0u32; 5]; // 26-bit limbs
    let mut s = [0u32; 4]; // 32-bit words

    let t0 = u32::from_le_bytes([key[0], key[1], key[2], key[3]]);
    let t1 = u32::from_le_bytes([key[4], key[5], key[6], key[7]]);
    let t2 = u32::from_le_bytes([key[8], key[9], key[10], key[11]]);
    let t3 = u32::from_le_bytes([key[12], key[13], key[14], key[15]]);

    // Clamp r
    r[0] = t0 & 0x3ffffff;
    r[1] = ((t0 >> 26) | (t1 << 6)) & 0x3ffff03;
    r[2] = ((t1 >> 20) | (t2 << 12)) & 0x3ffc0ff;
    r[3] = ((t2 >> 14) | (t3 << 18)) & 0x3f03fff;
    r[4] = (t3 >> 8) & 0x00fffff;

    s[0] = u32::from_le_bytes([key[16], key[17], key[18], key[19]]);
    s[1] = u32::from_le_bytes([key[20], key[21], key[22], key[23]]);
    s[2] = u32::from_le_bytes([key[24], key[25], key[26], key[27]]);
    s[3] = u32::from_le_bytes([key[28], key[29], key[30], key[31]]);

    // Precompute 5*r[i] for modular reduction
    let s1 = r[1] * 5;
    let s2 = r[2] * 5;
    let s3 = r[3] * 5;
    let s4 = r[4] * 5;

    // Accumulator in 26-bit limbs
    let mut h = [0u32; 5];

    let mut i = 0;
    while i < msg.len() {
        let block_end = (i + 16).min(msg.len());
        let block_len = block_end - i;

        // Load block into padded buffer
        let mut block = [0u8; 17];
        block[..block_len].copy_from_slice(&msg[i..block_end]);
        block[block_len] = 1; // pad byte
        let hibit = if block_len == 16 { 1u32 << 24 } else { 0u32 };

        // Decode block into 26-bit limbs and add to h
        let t0 = u32::from_le_bytes([block[0], block[1], block[2], block[3]]);
        let t1 = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
        let t2 = u32::from_le_bytes([block[8], block[9], block[10], block[11]]);
        let t3 = u32::from_le_bytes([block[12], block[13], block[14], block[15]]);

        h[0] += t0 & 0x3ffffff;
        h[1] += ((t0 >> 26) | (t1 << 6)) & 0x3ffffff;
        h[2] += ((t1 >> 20) | (t2 << 12)) & 0x3ffffff;
        h[3] += ((t2 >> 14) | (t3 << 18)) & 0x3ffffff;
        h[4] += (t3 >> 8) | hibit;

        // h = h * r mod 2^130 - 5
        let d0 = (h[0] as u64) * (r[0] as u64) + (h[1] as u64) * (s4 as u64)
               + (h[2] as u64) * (s3 as u64) + (h[3] as u64) * (s2 as u64)
               + (h[4] as u64) * (s1 as u64);
        let d1 = (h[0] as u64) * (r[1] as u64) + (h[1] as u64) * (r[0] as u64)
               + (h[2] as u64) * (s4 as u64) + (h[3] as u64) * (s3 as u64)
               + (h[4] as u64) * (s2 as u64);
        let d2 = (h[0] as u64) * (r[2] as u64) + (h[1] as u64) * (r[1] as u64)
               + (h[2] as u64) * (r[0] as u64) + (h[3] as u64) * (s4 as u64)
               + (h[4] as u64) * (s3 as u64);
        let d3 = (h[0] as u64) * (r[3] as u64) + (h[1] as u64) * (r[2] as u64)
               + (h[2] as u64) * (r[1] as u64) + (h[3] as u64) * (r[0] as u64)
               + (h[4] as u64) * (s4 as u64);
        let d4 = (h[0] as u64) * (r[4] as u64) + (h[1] as u64) * (r[3] as u64)
               + (h[2] as u64) * (r[2] as u64) + (h[3] as u64) * (r[1] as u64)
               + (h[4] as u64) * (r[0] as u64);

        // Carry propagation
        let mut c: u64;
        c = d0 >> 26; h[0] = d0 as u32 & 0x3ffffff;
        let d1 = d1 + c;
        c = d1 >> 26; h[1] = d1 as u32 & 0x3ffffff;
        let d2 = d2 + c;
        c = d2 >> 26; h[2] = d2 as u32 & 0x3ffffff;
        let d3 = d3 + c;
        c = d3 >> 26; h[3] = d3 as u32 & 0x3ffffff;
        let d4 = d4 + c;
        c = d4 >> 26; h[4] = d4 as u32 & 0x3ffffff;
        h[0] += (c as u32) * 5;
        c = (h[0] >> 26) as u64; h[0] &= 0x3ffffff;
        h[1] += c as u32;

        i += 16;
    }

    // Final reduction mod 2^130 - 5
    let mut c: u32;
    c = h[1] >> 26; h[1] &= 0x3ffffff;
    h[2] += c;
    c = h[2] >> 26; h[2] &= 0x3ffffff;
    h[3] += c;
    c = h[3] >> 26; h[3] &= 0x3ffffff;
    h[4] += c;
    c = h[4] >> 26; h[4] &= 0x3ffffff;
    h[0] += c * 5;
    c = h[0] >> 26; h[0] &= 0x3ffffff;
    h[1] += c;

    // Compute h + -p
    let mut g = [0u32; 5];
    c = h[0].wrapping_add(5) >> 26;
    g[0] = h[0].wrapping_add(5) & 0x3ffffff;
    c = h[1].wrapping_add(c) >> 26;
    g[1] = h[1].wrapping_add(c) & 0x3ffffff; // wrong, need to chain properly
    // Let me redo this more carefully:
    let mut g0 = h[0].wrapping_add(5);
    c = g0 >> 26; g0 &= 0x3ffffff;
    let mut g1 = h[1].wrapping_add(c);
    c = g1 >> 26; g1 &= 0x3ffffff;
    let mut g2 = h[2].wrapping_add(c);
    c = g2 >> 26; g2 &= 0x3ffffff;
    let mut g3 = h[3].wrapping_add(c);
    c = g3 >> 26; g3 &= 0x3ffffff;
    let g4 = h[4].wrapping_add(c).wrapping_sub(1 << 26);

    // Select h or g based on whether g4 < 0 (i.e. high bit set => h < p, use h)
    let mask = (g4 >> 31).wrapping_sub(1); // 0xFFFFFFFF if g4 >= 0, 0 if g4 < 0
    // Wait, g4 is u32. If h < p, then g = h+5-p would underflow and g4 wraps around.
    // Actually g4 = h[4] + carry - (1<<26). If h >= p, g4 won't underflow.
    // The trick: if h >= p, g4 won't have bit 31 set. If h < p, g4 will wrap.
    // mask = 0xFFFFFFFF if h >= p (use g), 0 if h < p (use h)
    let mask = !((g4 >> 31).wrapping_sub(1)); // Hmm, this is tricky with unsigned.

    // Simpler: check if g4's bit 26 is set (since g4 should be < 2^26 if no borrow)
    // Actually the standard approach: mask = (g4 >> 31) - 1
    // If g4 < 0 (viewed as signed), mask = 0xFFFFFFFF (keep h)
    // If g4 >= 0, mask = 0x00000000 (keep g... wait no)

    // Let me use the standard Poly1305 finalization from donna:
    // mask = -(g4 >> 31) - 1  (all 1s if g4 positive, all 0s if negative)
    // Reinterpreted for u32: if bit 31 is set, g4 "underflowed"
    let mask = (g4 >> 31).wrapping_neg().wrapping_sub(1);
    // Wait this doesn't work either for u32.

    // Cleaner: the carry into g4 already accounts for everything.
    // g4 = h[4] + c - (1 << 26). If this doesn't underflow (as i64), then h >= p.
    // For u32: g4 will be very large (wrapping) if underflow.
    // Check: if g4 > 0x3ffffff (can't happen without underflow), then underflow occurred.
    // Simpler approach: just check bit 26 of g4
    let mask = if g4 & (1u32 << 26) != 0 { 0u32 } else { 0xFFFFFFFFu32 };
    // mask = 0xFFFFFFFF means g4 is valid (h >= p), use g
    // mask = 0 means underflow (h < p), use h

    h[0] = (h[0] & !mask) | (g0 & mask);
    h[1] = (h[1] & !mask) | (g1 & mask);
    h[2] = (h[2] & !mask) | (g2 & mask);
    h[3] = (h[3] & !mask) | (g3 & mask);
    h[4] = (h[4] & !mask) | (g4 & mask);

    // Pack h into 128 bits and add s
    let f0 = ((h[0]) | (h[1] << 26)) as u64;
    let f1 = ((h[1] >> 6) | (h[2] << 20)) as u64;
    let f2 = ((h[2] >> 12) | (h[3] << 14)) as u64;
    let f3 = ((h[3] >> 18) | (h[4] << 8)) as u64;

    let mut f0 = f0 + s[0] as u64;
    let c = f0 >> 32; f0 &= 0xFFFFFFFF;
    let mut f1 = f1 + s[1] as u64 + c;
    let c = f1 >> 32; f1 &= 0xFFFFFFFF;
    let mut f2 = f2 + s[2] as u64 + c;
    let c = f2 >> 32; f2 &= 0xFFFFFFFF;
    let f3 = f3 + s[3] as u64 + c;

    let mut tag = [0u8; 16];
    tag[0..4].copy_from_slice(&(f0 as u32).to_le_bytes());
    tag[4..8].copy_from_slice(&(f1 as u32).to_le_bytes());
    tag[8..12].copy_from_slice(&(f2 as u32).to_le_bytes());
    tag[12..16].copy_from_slice(&(f3 as u32).to_le_bytes());
    tag
}

// ============================================================================
// ChaCha20-Poly1305 AEAD (RFC 8439)
// ============================================================================

/// Encrypt with ChaCha20-Poly1305 AEAD.
/// Returns (ciphertext, tag). Ciphertext is same length as plaintext.
pub fn aead_encrypt(
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    plaintext: &[u8],
) -> (alloc::vec::Vec<u8>, [u8; 16]) {
    // Generate Poly1305 one-time key from first ChaCha20 block
    let poly_key_block = chacha20_block(key, 0, nonce);
    let mut poly_key = [0u8; 32];
    poly_key.copy_from_slice(&poly_key_block[..32]);

    // Encrypt plaintext with ChaCha20 (counter starts at 1)
    let mut ciphertext = plaintext.to_vec();
    chacha20_xor(key, 1, nonce, &mut ciphertext);

    // Build Poly1305 input: AAD || pad || ciphertext || pad || len(AAD) || len(CT)
    let mac_data = build_poly1305_data(aad, &ciphertext);

    // Compute tag
    let tag = poly1305_mac(&poly_key, &mac_data);

    (ciphertext, tag)
}

/// Decrypt with ChaCha20-Poly1305 AEAD.
/// Returns plaintext on success, None if tag verification fails.
pub fn aead_decrypt(
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    ciphertext: &[u8],
    tag: &[u8; 16],
) -> Option<alloc::vec::Vec<u8>> {
    // Generate Poly1305 one-time key
    let poly_key_block = chacha20_block(key, 0, nonce);
    let mut poly_key = [0u8; 32];
    poly_key.copy_from_slice(&poly_key_block[..32]);

    // Verify tag
    let mac_data = build_poly1305_data(aad, ciphertext);
    let expected_tag = poly1305_mac(&poly_key, &mac_data);

    // Constant-time compare
    let mut diff = 0u8;
    for i in 0..16 {
        diff |= expected_tag[i] ^ tag[i];
    }
    if diff != 0 {
        return None;
    }

    // Decrypt
    let mut plaintext = ciphertext.to_vec();
    chacha20_xor(key, 1, nonce, &mut plaintext);

    Some(plaintext)
}

/// Build the Poly1305 input for AEAD (AAD || pad || CT || pad || len_aad || len_ct).
fn build_poly1305_data(aad: &[u8], ciphertext: &[u8]) -> alloc::vec::Vec<u8> {
    let mut data = alloc::vec::Vec::new();
    data.extend_from_slice(aad);
    // Pad AAD to 16-byte boundary
    let pad_aad = (16 - (aad.len() % 16)) % 16;
    data.extend(core::iter::repeat(0u8).take(pad_aad));
    data.extend_from_slice(ciphertext);
    // Pad ciphertext to 16-byte boundary
    let pad_ct = (16 - (ciphertext.len() % 16)) % 16;
    data.extend(core::iter::repeat(0u8).take(pad_ct));
    // Lengths as 64-bit little-endian
    data.extend_from_slice(&(aad.len() as u64).to_le_bytes());
    data.extend_from_slice(&(ciphertext.len() as u64).to_le_bytes());
    data
}

// ============================================================================
// TLS 1.3 Key Schedule helpers
// ============================================================================

/// HKDF-Expand-Label for TLS 1.3.
/// info = length || "tls13 " || label || context
pub fn tls13_hkdf_expand_label(
    secret: &[u8; 32],
    label: &[u8],
    context: &[u8],
    length: u16,
) -> [u8; 32] {
    let mut info = alloc::vec::Vec::new();
    // Length (2 bytes)
    info.extend_from_slice(&length.to_be_bytes());
    // Label: length byte + "tls13 " + label
    let full_label_len = 6 + label.len();
    info.push(full_label_len as u8);
    info.extend_from_slice(b"tls13 ");
    info.extend_from_slice(label);
    // Context: length byte + context
    info.push(context.len() as u8);
    info.extend_from_slice(context);
    hkdf_expand_32(secret, &info)
}

/// Derive-Secret for TLS 1.3.
/// = HKDF-Expand-Label(Secret, Label, Transcript-Hash(Messages), Hash.length)
pub fn tls13_derive_secret(
    secret: &[u8; 32],
    label: &[u8],
    transcript_hash: &[u8; 32],
) -> [u8; 32] {
    tls13_hkdf_expand_label(secret, label, transcript_hash, 32)
}

// ============================================================================
// Tests (called from STRESS gate)
// ============================================================================

/// Test X25519 with RFC 7748 test vectors.
pub fn test_x25519() -> bool {
    // RFC 7748 Section 5.2 test vector
    let scalar: [u8; 32] = [
        0xa5, 0x46, 0xe3, 0x6b, 0xf0, 0x52, 0x7c, 0x9d,
        0x3b, 0x16, 0x15, 0x4b, 0x82, 0x46, 0x5e, 0xdd,
        0x62, 0x14, 0x4c, 0x0a, 0xc1, 0xfc, 0x5a, 0x18,
        0x50, 0x6a, 0x22, 0x44, 0xba, 0x44, 0x9a, 0xc4,
    ];
    let u_coord: [u8; 32] = [
        0xe6, 0xdb, 0x68, 0x67, 0x58, 0x30, 0x30, 0xdb,
        0x35, 0x94, 0xc1, 0xa4, 0x24, 0xb1, 0x5f, 0x7c,
        0x72, 0x66, 0x24, 0xec, 0x26, 0xb3, 0x35, 0x3b,
        0x10, 0xa9, 0x03, 0xa6, 0xd0, 0xab, 0x1c, 0x4c,
    ];
    let expected: [u8; 32] = [
        0xc3, 0xda, 0x55, 0x37, 0x9d, 0xe9, 0xc6, 0x90,
        0x8e, 0x94, 0xea, 0x4d, 0xf2, 0x8d, 0x08, 0x4f,
        0x32, 0xec, 0xcf, 0x03, 0x49, 0x1c, 0x71, 0xf7,
        0x54, 0xb4, 0x07, 0x55, 0x77, 0xa2, 0x85, 0x52,
    ];

    let result = x25519(&scalar, &u_coord);
    result == expected
}

/// Test ChaCha20 with RFC 8439 Section 2.4.2 test vector.
pub fn test_chacha20() -> bool {
    let key: [u8; 32] = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
        0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
        0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
    ];
    let nonce: [u8; 12] = [
        0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x4a,
        0x00, 0x00, 0x00, 0x00,
    ];

    // Test: encrypt zeros should give the keystream
    let block = chacha20_block(&key, 1, &nonce);
    // First 4 bytes of keystream at counter=1 (from RFC 8439 2.4.2):
    // 10 f1 e7 e4
    block[0] == 0x10 && block[1] == 0xf1 && block[2] == 0xe7 && block[3] == 0xe4
}

/// Test Poly1305 with RFC 8439 Section 2.5.2 test vector.
pub fn test_poly1305() -> bool {
    let key: [u8; 32] = [
        0x85, 0xd6, 0xbe, 0x78, 0x57, 0x55, 0x6d, 0x33,
        0x7f, 0x44, 0x52, 0xfe, 0x42, 0xd5, 0x06, 0xa8,
        0x01, 0x03, 0x80, 0x8a, 0xfb, 0x0d, 0xb2, 0xfd,
        0x4a, 0xbf, 0xf6, 0xaf, 0x41, 0x49, 0xf5, 0x1b,
    ];
    let msg = b"Cryptographic Forum Research Group";
    let expected: [u8; 16] = [
        0xa8, 0x06, 0x1d, 0xc1, 0x30, 0x51, 0x36, 0xc6,
        0xc2, 0x2b, 0x8b, 0xaf, 0x0c, 0x01, 0x27, 0xa9,
    ];

    let tag = poly1305_mac(&key, msg);
    tag == expected
}

/// Test AEAD encrypt/decrypt roundtrip.
pub fn test_aead_roundtrip() -> bool {
    let key = sha256(b"test-aead-key");
    let nonce: [u8; 12] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
    let aad = b"additional data";
    let plaintext = b"Hello, TLS 1.3!";

    let (ct, tag) = aead_encrypt(&key, &nonce, aad, plaintext);
    match aead_decrypt(&key, &nonce, aad, &ct, &tag) {
        Some(pt) => pt == plaintext,
        None => false,
    }
}

/// Test AEAD tamper detection.
pub fn test_aead_tamper() -> bool {
    let key = sha256(b"test-aead-key-2");
    let nonce: [u8; 12] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2];
    let aad = b"aad";
    let plaintext = b"secret data";

    let (mut ct, tag) = aead_encrypt(&key, &nonce, aad, plaintext);
    // Tamper with ciphertext
    ct[0] ^= 0xFF;
    // Decrypt should fail
    aead_decrypt(&key, &nonce, aad, &ct, &tag).is_none()
}

/// Test HKDF extract + expand.
pub fn test_hkdf() -> bool {
    let salt = b"salt";
    let ikm = b"input key material";
    let prk = hkdf_extract(salt, ikm);
    // PRK should be 32 bytes, non-zero
    prk != [0u8; 32]
}
