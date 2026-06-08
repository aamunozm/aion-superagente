//! Autenticación: hashing de contraseñas (Argon2id) y JWT de sesión.

use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

/// Hashea una contraseña con Argon2id. Devuelve el string PHC para almacenar.
pub fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| format!("hash falló: {e}"))
}

/// Verifica una contraseña contra su hash PHC almacenado.
pub fn verify_password(password: &str, phc: &str) -> bool {
    match PasswordHash::new(phc) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// Claims del JWT de acceso.
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // user id
    pub email: String,
    pub exp: usize, // expiración (unix)
}

/// Emite un JWT de acceso firmado con HS256, válido `ttl_secs` segundos.
pub fn issue_jwt(
    secret: &[u8],
    user_id: &str,
    email: &str,
    ttl_secs: i64,
) -> Result<String, String> {
    let exp = (chrono::Utc::now() + chrono::Duration::seconds(ttl_secs)).timestamp() as usize;
    let claims = Claims {
        sub: user_id.to_string(),
        email: email.to_string(),
        exp,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret),
    )
    .map_err(|e| format!("jwt encode falló: {e}"))
}

/// Verifica y decodifica un JWT de acceso.
pub fn verify_jwt(secret: &[u8], token: &str) -> Result<Claims, String> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret),
        &Validation::default(),
    )
    .map(|d| d.claims)
    .map_err(|e| format!("jwt inválido: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_roundtrip() {
        let phc = hash_password("S3cret!").unwrap();
        assert!(verify_password("S3cret!", &phc));
        assert!(!verify_password("wrong", &phc));
    }

    #[test]
    fn jwt_roundtrip() {
        let secret = b"test-secret-key";
        let token = issue_jwt(secret, "u1", "a@b.com", 3600).unwrap();
        let claims = verify_jwt(secret, &token).unwrap();
        assert_eq!(claims.sub, "u1");
        assert_eq!(claims.email, "a@b.com");
    }

    #[test]
    fn jwt_rejects_wrong_secret() {
        let token = issue_jwt(b"secret-a", "u1", "a@b.com", 3600).unwrap();
        assert!(verify_jwt(b"secret-b", &token).is_err());
    }
}
