//! Licencias firmadas Ed25519: emisión (servidor) y validación offline (cliente).

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

/// Licencia firmada: claims + firma (hex) + clave pública (hex).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedLicense {
    pub claims: LicenseClaims,
    pub signature_hex: String,
    pub public_key_hex: String,
}

/// Emisor de licencias (posee la clave privada). Lado servidor.
pub struct LicenseIssuer {
    signing: SigningKey,
}

impl LicenseIssuer {
    /// Genera un emisor con clave nueva.
    pub fn generate() -> Self {
        let mut csprng = rand::rngs::OsRng;
        Self {
            signing: SigningKey::generate(&mut csprng),
        }
    }

    /// Reconstruye el emisor desde la clave privada en hex (persistencia).
    pub fn from_hex(sk_hex: &str) -> Result<Self, String> {
        let bytes: [u8; 32] = hex::decode(sk_hex.trim())
            .map_err(|e| e.to_string())?
            .try_into()
            .map_err(|_| "clave privada inválida (se esperan 32 bytes)".to_string())?;
        Ok(Self {
            signing: SigningKey::from_bytes(&bytes),
        })
    }

    /// Clave privada en hex (para persistir de forma segura).
    pub fn signing_key_hex(&self) -> String {
        hex::encode(self.signing.to_bytes())
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

/// Estado de validación de una licencia (lado cliente).
#[derive(Debug, Clone, PartialEq)]
pub struct LicenseStatus {
    /// ¿La app puede funcionar? (válida o dentro de la gracia).
    pub usable: bool,
    /// ¿Está en periodo de gracia (vencida pero aún tolerada)?
    pub in_grace: bool,
    pub tier: String,
    /// Días restantes (negativo si vencida).
    pub days_left: i64,
    pub reason: String,
}

/// Validador offline: embebe la clave pública de confianza + periodo de gracia.
pub struct LicenseValidator {
    trusted_pub_hex: String,
    grace_days: i64,
}

impl LicenseValidator {
    pub fn new(trusted_pub_hex: impl Into<String>, grace_days: i64) -> Self {
        Self {
            trusted_pub_hex: trusted_pub_hex.into(),
            grace_days,
        }
    }

    /// Valida la firma contra la clave de confianza y evalúa la caducidad con gracia.
    pub fn validate(&self, lic: &SignedLicense, now_ts: i64) -> LicenseStatus {
        let invalid = |reason: &str| LicenseStatus {
            usable: false,
            in_grace: false,
            tier: lic.claims.tier.clone(),
            days_left: 0,
            reason: reason.to_string(),
        };

        // 1) La clave del blob debe ser la de confianza (anti-suplantación).
        if lic.public_key_hex != self.trusted_pub_hex {
            return invalid("clave pública no confiable (licencia de otro emisor)");
        }
        // 2) Verificar firma criptográfica.
        if !signature_ok(lic) {
            return invalid("firma inválida o manipulada");
        }
        // 3) Caducidad + gracia.
        let days_left = (lic.claims.valid_until - now_ts) / 86_400;
        if now_ts <= lic.claims.valid_until {
            LicenseStatus {
                usable: true,
                in_grace: false,
                tier: lic.claims.tier.clone(),
                days_left,
                reason: "licencia válida".into(),
            }
        } else if now_ts <= lic.claims.valid_until + self.grace_days * 86_400 {
            LicenseStatus {
                usable: true,
                in_grace: true,
                tier: lic.claims.tier.clone(),
                days_left,
                reason: "vencida pero dentro del periodo de gracia".into(),
            }
        } else {
            invalid("licencia vencida (fuera de gracia)")
        }
    }
}

fn signature_ok(lic: &SignedLicense) -> bool {
    let Ok(pk) = hex::decode(&lic.public_key_hex) else {
        return false;
    };
    let Ok(pk): Result<[u8; 32], _> = pk.try_into() else {
        return false;
    };
    let Ok(vk) = VerifyingKey::from_bytes(&pk) else {
        return false;
    };
    let Ok(sig) = hex::decode(&lic.signature_hex) else {
        return false;
    };
    let Ok(sig): Result<[u8; 64], _> = sig.try_into() else {
        return false;
    };
    let sig = Signature::from_bytes(&sig);
    let Ok(payload) = serde_json::to_vec(&lic.claims) else {
        return false;
    };
    vk.verify(&payload, &sig).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(valid_until: i64) -> LicenseClaims {
        LicenseClaims {
            user_id: "u1".into(),
            tier: "pro".into(),
            seats: 1,
            valid_until,
        }
    }

    #[test]
    fn issuer_persists_and_reloads_same_key() {
        let issuer = LicenseIssuer::generate();
        let sk = issuer.signing_key_hex();
        let reloaded = LicenseIssuer::from_hex(&sk).unwrap();
        assert_eq!(issuer.public_key_hex(), reloaded.public_key_hex());
    }

    #[test]
    fn valid_license_passes_offline() {
        let issuer = LicenseIssuer::generate();
        let lic = issuer.issue(claims(2_000_000_000)).unwrap();
        let v = LicenseValidator::new(issuer.public_key_hex(), 7);
        let st = v.validate(&lic, 1_000_000_000);
        assert!(st.usable && !st.in_grace);
    }

    #[test]
    fn expired_within_grace_is_usable() {
        let issuer = LicenseIssuer::generate();
        let lic = issuer.issue(claims(1_000_000_000)).unwrap();
        let v = LicenseValidator::new(issuer.public_key_hex(), 7);
        // 3 días después de vencer (gracia 7) → usable en gracia.
        let st = v.validate(&lic, 1_000_000_000 + 3 * 86_400);
        assert!(st.usable && st.in_grace);
    }

    #[test]
    fn expired_beyond_grace_is_rejected() {
        let issuer = LicenseIssuer::generate();
        let lic = issuer.issue(claims(1_000_000_000)).unwrap();
        let v = LicenseValidator::new(issuer.public_key_hex(), 7);
        let st = v.validate(&lic, 1_000_000_000 + 30 * 86_400);
        assert!(!st.usable);
    }

    #[test]
    fn tampered_or_wrong_issuer_rejected() {
        let issuer = LicenseIssuer::generate();
        let other = LicenseIssuer::generate();
        // Manipulación del tier:
        let mut lic = issuer.issue(claims(2_000_000_000)).unwrap();
        lic.claims.tier = "team".into();
        let v = LicenseValidator::new(issuer.public_key_hex(), 7);
        assert!(!v.validate(&lic, 1_000_000_000).usable, "firma manipulada");
        // Licencia firmada por otro emisor:
        let lic2 = other.issue(claims(2_000_000_000)).unwrap();
        assert!(
            !v.validate(&lic2, 1_000_000_000).usable,
            "emisor no confiable"
        );
    }
}
