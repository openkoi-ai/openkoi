// src/auth/oauth.rs — Shared OAuth utilities: PKCE, device code polling, token refresh
//
// Provides the building blocks used by GitHub Copilot (device code) and
// OpenAI ChatGPT Plus/Pro (device code variant).

use anyhow::{bail, Result};

// ─── PKCE (RFC 7636) ────────────────────────────────────────────────────────

/// Generate a random code_verifier (43-128 URL-safe chars) and its S256 challenge.
pub fn pkce_challenge() -> (String, String) {
    let verifier = generate_random_string(64);
    let challenge = sha256_base64url(verifier.as_bytes());
    (verifier, challenge)
}

/// SHA-256 hash → base64url-encoded (no padding). Used for PKCE S256.
pub fn sha256_base64url(data: &[u8]) -> String {
    let hash = sha256(data);
    base64url_encode(&hash)
}

// ─── Random string ──────────────────────────────────────────────────────────

/// Generate a cryptographically random string of the given length.
/// Uses the `getrandom` crate for OS-provided CSPRNG on all platforms
/// (Unix, Windows, WASM). Applies rejection sampling to avoid modular bias.
fn generate_random_string(len: usize) -> String {
    // 66 characters — use rejection sampling since 256 % 66 != 0
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    // Largest multiple of CHARSET.len() that fits in a u8 (252 = 66 * 3 + 54).
    // Actually: floor(256 / 66) * 66 = 3 * 66 = 198. Reject bytes >= 198.
    const REJECT_THRESHOLD: u8 = (256 - (256 % CHARSET.len() as u16)) as u8; // 198

    let mut result = String::with_capacity(len);

    // Process in batches to minimize getrandom calls
    let mut buf = vec![0u8; len * 2]; // over-allocate to reduce iterations
    while result.len() < len {
        getrandom::getrandom(&mut buf).expect(
            "getrandom failed: OS CSPRNG unavailable — cannot generate secure PKCE verifier",
        );

        for &b in &buf {
            if result.len() >= len {
                break;
            }
            // Rejection sampling: discard bytes that would cause modular bias
            if b < REJECT_THRESHOLD {
                result.push(CHARSET[(b as usize) % CHARSET.len()] as char);
            }
        }
    }

    result
}

// ─── Base64url encoding (no padding) ────────────────────────────────────────

pub fn base64url_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

    let mut result = String::with_capacity((data.len() * 4).div_ceil(3));
    let mut i = 0;
    while i + 2 < data.len() {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8) | (data[i + 2] as u32);
        result.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
        result.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
        result.push(TABLE[((n >> 6) & 0x3F) as usize] as char);
        result.push(TABLE[(n & 0x3F) as usize] as char);
        i += 3;
    }
    let remaining = data.len() - i;
    if remaining == 2 {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8);
        result.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
        result.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
        result.push(TABLE[((n >> 6) & 0x3F) as usize] as char);
        // No padding
    } else if remaining == 1 {
        let n = (data[i] as u32) << 16;
        result.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
        result.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
        // No padding
    }
    result
}

/// Standard base64 encoding (with +/ and = padding) for form values.
pub fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 2 < data.len() {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8) | (data[i + 2] as u32);
        result.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
        result.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
        result.push(TABLE[((n >> 6) & 0x3F) as usize] as char);
        result.push(TABLE[(n & 0x3F) as usize] as char);
        i += 3;
    }
    let remaining = data.len() - i;
    if remaining == 2 {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8);
        result.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
        result.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
        result.push(TABLE[((n >> 6) & 0x3F) as usize] as char);
        result.push('=');
    } else if remaining == 1 {
        let n = (data[i] as u32) << 16;
        result.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
        result.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
        result.push('=');
        result.push('=');
    }
    result
}

/// Decode a base64url-encoded string (no padding) to bytes.
pub fn base64url_decode(input: &str) -> Result<Vec<u8>> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'-' => Some(62),
            b'_' => Some(63),
            _ => None,
        }
    }

    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity((bytes.len() * 3) / 4);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;

    for &b in bytes {
        if b == b'=' {
            continue;
        }
        let v =
            val(b).ok_or_else(|| anyhow::anyhow!("invalid base64url character: {}", b as char))?;
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

// ─── JWT payload extraction ─────────────────────────────────────────────────

/// Extract the JSON payload from a JWT (the middle part) without verifying
/// the signature. Used to read claims like `account_id`.
///
/// # Security: Trust Assumption
/// This function does NOT verify the JWT signature. It is ONLY safe to use on
/// tokens received directly over TLS from a trusted authorization server (e.g.,
/// the token endpoint response). Do NOT use on tokens received from untrusted
/// sources (e.g., client-supplied headers, query params) as the claims could
/// be forged.
pub fn decode_jwt_payload(jwt: &str) -> Result<serde_json::Value> {
    let parts: Vec<&str> = jwt.split('.').collect();
    if parts.len() != 3 {
        bail!("Invalid JWT: expected 3 parts, got {}", parts.len());
    }
    let payload_bytes = base64url_decode(parts[1])?;
    let payload: serde_json::Value = serde_json::from_slice(&payload_bytes)?;
    Ok(payload)
}

// ─── Device code polling ────────────────────────────────────────────────────

/// Result of a device code initiation.
#[derive(Debug, Clone)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
}

/// Open a URL in the user's browser. Prints a warning if the browser can't be opened.
pub fn open_browser(url: &str) {
    let result;
    #[cfg(target_os = "macos")]
    {
        result = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        result = std::process::Command::new("xdg-open").arg(url).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        result = std::process::Command::new("cmd")
            .args(["/C", "start", url])
            .spawn();
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        result = Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "unsupported platform",
        ));
    }

    if let Err(e) = result {
        eprintln!("  Could not open browser automatically: {e}");
        eprintln!("  Please open the URL above manually.");
    }
}

// ─── URL encoding ───────────────────────────────────────────────────────────

/// Simple percent-encoding for URL query parameters and form values.
pub fn urlencoding(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(b as char);
            }
            _ => {
                result.push('%');
                result.push(char::from(b"0123456789ABCDEF"[(b >> 4) as usize]));
                result.push(char::from(b"0123456789ABCDEF"[(b & 0x0F) as usize]));
            }
        }
    }
    result
}

// ─── SHA-256 (zero-dependency implementation) ───────────────────────────────
// NOTE: This is a standalone SHA-256 to avoid pulling in a crypto crate just
// for PKCE S256 hashing. If a crypto crate (e.g. `sha2`) is added to the
// project for other reasons, this should be replaced with it.

/// Compute the SHA-256 hash of the input data (FIPS 180-4).
pub fn sha256(data: &[u8]) -> [u8; 32] {
    use std::num::Wrapping;

    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let mut h: [Wrapping<u32>; 8] = [
        Wrapping(0x6a09e667),
        Wrapping(0xbb67ae85),
        Wrapping(0x3c6ef372),
        Wrapping(0xa54ff53a),
        Wrapping(0x510e527f),
        Wrapping(0x9b05688c),
        Wrapping(0x1f83d9ab),
        Wrapping(0x5be0cd19),
    ];

    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [Wrapping(0u32); 64];
        for i in 0..16 {
            w[i] = Wrapping(u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]));
        }
        for i in 16..64 {
            let s0 =
                (w[i - 15].0.rotate_right(7)) ^ (w[i - 15].0.rotate_right(18)) ^ (w[i - 15].0 >> 3);
            let s1 =
                (w[i - 2].0.rotate_right(17)) ^ (w[i - 2].0.rotate_right(19)) ^ (w[i - 2].0 >> 10);
            w[i] = w[i - 16] + Wrapping(s0) + w[i - 7] + Wrapping(s1);
        }

        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);

        for i in 0..64 {
            let s1 = Wrapping(e.0.rotate_right(6) ^ e.0.rotate_right(11) ^ e.0.rotate_right(25));
            let ch = Wrapping((e.0 & f.0) ^ ((!e.0) & g.0));
            let temp1 = hh + s1 + ch + Wrapping(K[i]) + w[i];
            let s0 = Wrapping(a.0.rotate_right(2) ^ a.0.rotate_right(13) ^ a.0.rotate_right(22));
            let maj = Wrapping((a.0 & b.0) ^ (a.0 & c.0) ^ (b.0 & c.0));
            let temp2 = s0 + maj;

            hh = g;
            g = f;
            f = e;
            e = d + temp1;
            d = c;
            c = b;
            b = a;
            a = temp1 + temp2;
        }

        h[0] += a;
        h[1] += b;
        h[2] += c;
        h[3] += d;
        h[4] += e;
        h[5] += f;
        h[6] += g;
        h[7] += hh;
    }

    let mut result = [0u8; 32];
    for (i, val) in h.iter().enumerate() {
        result[i * 4..i * 4 + 4].copy_from_slice(&val.0.to_be_bytes());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_empty() {
        let hash = sha256(b"");
        let hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
        assert_eq!(
            hex,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_base64url_encode() {
        assert_eq!(base64url_encode(b"hello"), "aGVsbG8");
        assert_eq!(base64url_encode(b""), "");
    }

    #[test]
    fn test_base64url_roundtrip() {
        let data = b"test data for roundtrip";
        let encoded = base64url_encode(data);
        let decoded = base64url_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_pkce_challenge() {
        let (verifier, challenge) = pkce_challenge();
        assert!(verifier.len() >= 43);
        assert!(!challenge.is_empty());
        // Verify the challenge matches the verifier
        let expected = sha256_base64url(verifier.as_bytes());
        assert_eq!(challenge, expected);
    }

    #[test]
    fn test_decode_jwt_payload() {
        // A minimal JWT with payload {"sub":"1234567890","name":"John"}
        // Header: {"alg":"none","typ":"JWT"} = eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0
        // Payload: {"sub":"1234567890","name":"John"} = eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4ifQ
        let jwt = "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4ifQ.signature";
        let payload = decode_jwt_payload(jwt).unwrap();
        assert_eq!(payload["sub"].as_str(), Some("1234567890"));
        assert_eq!(payload["name"].as_str(), Some("John"));
    }

    #[test]
    fn test_decode_jwt_invalid() {
        assert!(decode_jwt_payload("not.a.jwt.token").is_err());
        assert!(decode_jwt_payload("only-one-part").is_err());
    }
}
