//! Licencias firmadas con Ed25519, verificables **offline** por el cliente.
//!
//! El control-plane firma la licencia con su clave privada; el cliente
//! (`aion-control-client`) la valida con la clave pública embebida, sin red.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

/// Contenido de una licencia (lo que se firma).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LicenseClaims {
    pub user_id: String,
    pub tier: String, // free | pro | team
    pub seats: u32,
    pub valid_until: i64, // unix timestamp
}

/// Licencia firmada: claims + firma (hex) + clave pública (hex) para verificación.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedLicense {
    pub claims: LicenseClaims,
    pub signature_hex: String,
    pub public_key_hex: String,
}

/// Emisor de licencias (posee la clave privada).
pub struct LicenseIssuer {
    signing: SigningKey,
}

impl LicenseIssuer {
    /// Genera un emisor con una clave nueva (dev). En prod la clave se persiste/HSM.
    pub fn generate() -> Self {
        let mut csprng = rand::rngs::OsRng;
        Self {
            signing: SigningKey::generate(&mut csprng),
        }
    }

    /// Clave pública en hex (para embeber en el cliente).
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.signing.verifying_key().to_bytes())
    }

    /// Firma unos claims y devuelve la licencia firmada.
    pub fn issue(&self, claims: LicenseClaims) -> Result<SignedLicense, String> {
        let payload = serde_json::to_vec(&claims).map_err(|e| e.to_string())?;
        let sig: Signature = self.signing.sign(&payload);
        Ok(SignedLicense {
            claims,
            signature_hex: hex::encode(sig.to_bytes()),
            public_key_hex: self.public_key_hex(),
        })
    }
}

/// Verifica una licencia firmada de forma independiente (lo que hará el cliente
/// `aion-control-client` offline). Reside junto al emisor y se cubre por tests;
/// el consumo en producción es del lado cliente.
#[allow(dead_code)]
pub fn verify_license(lic: &SignedLicense) -> Result<(), String> {
    let pk_bytes: [u8; 32] = hex::decode(&lic.public_key_hex)
        .map_err(|e| e.to_string())?
        .try_into()
        .map_err(|_| "clave pública inválida".to_string())?;
    let vk = VerifyingKey::from_bytes(&pk_bytes).map_err(|e| e.to_string())?;
    let sig_bytes: [u8; 64] = hex::decode(&lic.signature_hex)
        .map_err(|e| e.to_string())?
        .try_into()
        .map_err(|_| "firma inválida".to_string())?;
    let sig = Signature::from_bytes(&sig_bytes);
    let payload = serde_json::to_vec(&lic.claims).map_err(|e| e.to_string())?;
    vk.verify(&payload, &sig)
        .map_err(|e| format!("firma no válida: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims() -> LicenseClaims {
        LicenseClaims {
            user_id: "u1".into(),
            tier: "pro".into(),
            seats: 1,
            valid_until: 9_999_999_999,
        }
    }

    #[test]
    fn license_signs_and_verifies() {
        let issuer = LicenseIssuer::generate();
        let lic = issuer.issue(claims()).unwrap();
        assert!(verify_license(&lic).is_ok());
    }

    #[test]
    fn tampered_license_fails() {
        let issuer = LicenseIssuer::generate();
        let mut lic = issuer.issue(claims()).unwrap();
        lic.claims.tier = "team".into(); // manipulación
        assert!(verify_license(&lic).is_err());
    }
}
