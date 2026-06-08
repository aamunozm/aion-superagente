//! Cifrado de extremo a extremo (XChaCha20-Poly1305) con clave derivada de la
//! passphrase del usuario (Argon2id). El relay en la nube solo ve el blob cifrado.

use argon2::Argon2;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::RngCore;

const NONCE_LEN: usize = 24;

/// Deriva una clave de 32 bytes desde la passphrase + salt (Argon2id).
/// El salt debe ser estable por usuario (p. ej. derivado del email) y ≥ 8 bytes.
pub fn derive_key(passphrase: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| format!("derivación de clave falló: {e}"))?;
    Ok(key)
}

/// Cifra `plaintext` con la clave. Devuelve `nonce(24) || ciphertext`.
pub fn encrypt(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, String> {
    let cipher = XChaCha20Poly1305::new(key.into());
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = XNonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| format!("cifrado falló: {e}"))?;
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Descifra un blob `nonce(24) || ciphertext`. Falla si la clave es incorrecta
/// o el blob fue manipulado (autenticación Poly1305).
pub fn decrypt(key: &[u8; 32], blob: &[u8]) -> Result<Vec<u8>, String> {
    if blob.len() < NONCE_LEN {
        return Err("blob demasiado corto".into());
    }
    let (nonce_bytes, ct) = blob.split_at(NONCE_LEN);
    let cipher = XChaCha20Poly1305::new(key.into());
    cipher
        .decrypt(XNonce::from_slice(nonce_bytes), ct)
        .map_err(|_| "descifrado falló (clave incorrecta o blob manipulado)".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let key = derive_key("Boston-cambiala", b"ariel@ceo-intelligence.com").unwrap();
        let blob = encrypt(&key, b"contenido cognitivo secreto").unwrap();
        assert_ne!(&blob[24..], b"contenido cognitivo secreto"); // es ciphertext
        let pt = decrypt(&key, &blob).unwrap();
        assert_eq!(pt, b"contenido cognitivo secreto");
    }

    #[test]
    fn wrong_key_or_tamper_fails() {
        let key = derive_key("clave-A", b"saltsalt").unwrap();
        let other = derive_key("clave-B", b"saltsalt").unwrap();
        let mut blob = encrypt(&key, b"datos").unwrap();
        assert!(decrypt(&other, &blob).is_err(), "clave incorrecta");
        let n = blob.len() - 1;
        blob[n] ^= 0xff; // manipular
        assert!(decrypt(&key, &blob).is_err(), "blob manipulado");
    }
}
