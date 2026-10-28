//! openGauss password authentication (DBLens patch, see `vendor/PATCHES.md`).
//!
//! openGauss reuses authentication request code 10 (`AUTH_REQ_SHA256`, the same code PostgreSQL uses for
//! SASL) for its own RFC 5802-style exchange. The request body starts with a 4-byte password storage
//! method (a small integer, so its first byte is 0), while a PostgreSQL SASL body starts with a mechanism
//! name; that first byte tells them apart.
//!
//! Body layout for the SHA256 storage method (`2`):
//! - protocol 3.0 (what tokio-postgres sends): 64 hex chars of salt, 8 hex chars of token and the 64-hex
//!   server signature; the iteration count is not sent, so it is found by checking the server signature;
//! - protocol 3.51 and later: salt, token and a 4-byte iteration count.
//!
//! Algorithm (openGauss `RFC5802Algorithm`): K = PBKDF2-HMAC-SHA1(password, salt, iterations, 32);
//! ServerKey = HMAC-SHA256(K, "Sever Key") (sic); ClientKey = HMAC-SHA256(K, "Client Key");
//! StoredKey = SHA256(ClientKey); proof = HMAC-SHA256(StoredKey, token) XOR ClientKey, sent as lowercase
//! hex in a password message. The server signature is HMAC-SHA256(ServerKey, token).
use hmac::{Hmac, KeyInit, Mac};
use sha1::Sha1;
use sha2::{Digest, Sha256};

/// Password stored as plain text: handled like SHA256.
const PLAIN_PASSWORD: i32 = 0;
/// Password stored as MD5: the body carries a 4-byte MD5 salt.
const MD5_PASSWORD: i32 = 1;
/// Password stored as SHA256.
const SHA256_PASSWORD: i32 = 2;

/// Iteration counts tried, in order, when the server does not send one (protocol 3.0): the current
/// openGauss default, then the default of older releases.
const ITERATIONS: [u32; 2] = [10000, 2048];

/// A parsed openGauss authentication request (code 10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Password stored as SHA256 (or plain text).
    Sha256 {
        /// 64 hex chars.
        salt: String,
        /// 8 hex chars.
        token: String,
        /// Sent by servers speaking protocol 3.0.
        server_signature: Option<String>,
        /// Sent by servers speaking protocol 3.51 and later.
        iterations: Option<u32>,
    },
    /// Password stored as MD5: answered like a PostgreSQL MD5 request.
    Md5 {
        /// The MD5 salt.
        salt: [u8; 4],
    },
}

/// Parses the body of an authentication request with code 10 when it is an openGauss request; returns
/// `None` for a PostgreSQL SASL body (which starts with a mechanism name) or an unknown storage method.
pub fn parse_request(body: &[u8]) -> Option<Request> {
    if body.len() < 4 || body[0] != 0 {
        return None;
    }
    let method = i32::from_be_bytes([body[0], body[1], body[2], body[3]]);
    let rest = &body[4..];
    match method {
        PLAIN_PASSWORD | SHA256_PASSWORD if rest.len() >= 72 => {
            let salt = std::str::from_utf8(&rest[..64]).ok()?.to_string();
            let token = std::str::from_utf8(&rest[64..72]).ok()?.to_string();
            let tail = &rest[72..];
            let (server_signature, iterations) = if tail.len() >= 64 {
                (Some(std::str::from_utf8(&tail[..64]).ok()?.to_string()), None)
            } else if tail.len() >= 4 {
                (None, Some(u32::from_be_bytes([tail[0], tail[1], tail[2], tail[3]])))
            } else {
                (None, None)
            };
            Some(Request::Sha256 { salt, token, server_signature, iterations })
        }
        MD5_PASSWORD if rest.len() >= 4 => Some(Request::Md5 { salt: [rest[0], rest[1], rest[2], rest[3]] }),
        _ => None,
    }
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC is able to accept all key sizes");
    mac.update(data);
    mac.finalize().into_bytes().into()
}

/// PBKDF2-HMAC-SHA1 producing 32 bytes (two SHA1 blocks).
fn pbkdf2_sha1(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
    let prf = Hmac::<Sha1>::new_from_slice(password).expect("HMAC is able to accept all key sizes");
    let mut out = [0u8; 32];
    for (block, chunk) in out.chunks_mut(20).enumerate() {
        let mut mac = prf.clone();
        mac.update(salt);
        mac.update(&(block as u32 + 1).to_be_bytes());
        let mut u: [u8; 20] = mac.finalize().into_bytes().into();
        let mut t = u;
        for _ in 1..iterations {
            let mut mac = prf.clone();
            mac.update(&u);
            u = mac.finalize().into_bytes().into();
            t.iter_mut().zip(u.iter()).for_each(|(a, b)| *a ^= b);
        }
        chunk.copy_from_slice(&t[..chunk.len()]);
    }
    out
}

fn from_hex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok()).collect()
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The server and client keys for a password, salt and iteration count.
fn keys(password: &[u8], salt: &[u8], iterations: u32) -> ([u8; 32], [u8; 32]) {
    let k = pbkdf2_sha1(password, salt, iterations);
    (hmac_sha256(&k, b"Sever Key"), hmac_sha256(&k, b"Client Key"))
}

/// The proof to send for an SHA256 request, as lowercase hex. Without an iteration count the candidates in
/// `ITERATIONS` are checked against the server signature. Returns `None` when the salt or token is not hex
/// or no candidate matches the server signature.
pub fn sha256_proof(
    password: &[u8],
    salt: &str,
    token: &str,
    server_signature: Option<&str>,
    iterations: Option<u32>,
) -> Option<String> {
    let salt = from_hex(salt)?;
    let token = from_hex(token)?;
    let (_, client_key) = match (iterations, server_signature) {
        (Some(n), _) => keys(password, &salt, n),
        (None, Some(signature)) => ITERATIONS.iter().find_map(|&n| {
            let (server_key, client_key) = keys(password, &salt, n);
            (to_hex(&hmac_sha256(&server_key, &token)) == signature.to_ascii_lowercase())
                .then_some((server_key, client_key))
        })?,
        (None, None) => keys(password, &salt, ITERATIONS[0]),
    };
    let stored_key: [u8; 32] = Sha256::digest(client_key).into();
    let signature = hmac_sha256(&stored_key, &token);
    let proof: Vec<u8> = signature.iter().zip(client_key.iter()).map(|(a, b)| a ^ b).collect();
    Some(to_hex(&proof))
}

#[cfg(test)]
mod test {
    use super::*;

    const SALT: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
    // Expected values computed with the reference algorithm and checked against openGauss 7.0.0-RC1.
    const SIG_10000: &str = "b01a9f7346d6a37e43b4e1e976de4750ee7c8d394bfa329287aed4d6cce189fd";
    const PROOF_10000: &str = "48447ffe6e19554775a9a7c1e7e2bd8ef1b656b11ee2e4c790e23cc7a693469e";
    const SIG_2048: &str = "fd51d5b00e8cd9927d44dd6391180b54aadcab0c224da9fd25afb49c2ce31d38";
    const PROOF_2048: &str = "b4d07ffa1ae0c5143768f8bed55079f4ec7ef7fc95954839c661a02a97f80694";

    #[test]
    fn proof_with_iterations_or_server_signature() {
        let pw = b"Gauss-Test-123";
        assert_eq!(sha256_proof(pw, SALT, "0a1b2c3d", None, Some(10000)).as_deref(), Some(PROOF_10000));
        assert_eq!(sha256_proof(pw, SALT, "0a1b2c3d", Some(SIG_10000), None).as_deref(), Some(PROOF_10000));
        assert_eq!(sha256_proof(pw, SALT, "0a1b2c3d", Some(SIG_2048), None).as_deref(), Some(PROOF_2048));
        assert_eq!(sha256_proof(b"wrong", SALT, "0a1b2c3d", Some(SIG_10000), None), None);
        assert_eq!(sha256_proof(pw, "zz", "0a1b2c3d", None, Some(10000)), None);
    }

    #[test]
    fn parses_requests_and_leaves_sasl_alone() {
        let mut v30 = vec![0, 0, 0, 2];
        v30.extend_from_slice(SALT.as_bytes());
        v30.extend_from_slice(b"0a1b2c3d");
        v30.extend_from_slice(SIG_10000.as_bytes());
        assert_eq!(
            parse_request(&v30),
            Some(Request::Sha256 {
                salt: SALT.into(),
                token: "0a1b2c3d".into(),
                server_signature: Some(SIG_10000.into()),
                iterations: None,
            })
        );
        let mut v351 = vec![0, 0, 0, 2];
        v351.extend_from_slice(SALT.as_bytes());
        v351.extend_from_slice(b"0a1b2c3d");
        v351.extend_from_slice(&10000u32.to_be_bytes());
        assert!(matches!(parse_request(&v351), Some(Request::Sha256 { iterations: Some(10000), .. })));
        assert_eq!(parse_request(&[0, 0, 0, 1, 9, 8, 7, 6]), Some(Request::Md5 { salt: [9, 8, 7, 6] }));
        assert_eq!(parse_request(b"SCRAM-SHA-256\0\0"), None);
        assert_eq!(parse_request(&[0, 0, 0, 3, 1]), None);
    }
}
