//! # aion-control-client
//!
//! Tipos y lógica de **licencias** compartidos entre el control-plane (que las
//! emite/firma) y el cliente (que las **valida offline**). Firma Ed25519.
//!
//! El cliente embebe la clave pública de confianza y valida la licencia sin red,
//! con un **periodo de gracia** configurable: la app sigue funcionando aunque el
//! control-plane esté caído, hasta `valid_until + gracia`.

mod license;

pub use license::{LicenseClaims, LicenseIssuer, LicenseStatus, LicenseValidator, SignedLicense};
