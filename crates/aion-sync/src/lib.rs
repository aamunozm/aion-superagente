//! # aion-sync
//!
//! Sincronización **local-first** entre dispositivos: un CRDT mergeable
//! ([`LwwMap`]) + **cifrado de extremo a extremo** ([`crypto`]). El relay en la
//! nube solo transporta **ciphertext opaco**; nunca ve el contenido.
//!
//! Flujo: cada dispositivo edita su [`LwwMap`] → lo serializa y **cifra** con la
//! clave derivada de la passphrase del usuario → sube el blob → otro dispositivo
//! lo **descifra y fusiona** (convergen sin conflictos, last-write-wins por clave).
//!
//! Nota: el plan menciona Automerge; aquí usamos un CRDT LWW propio (suficiente y
//! sin dependencias pesadas). El blob cifrado es intercambiable por uno de Automerge
//! tras la misma interfaz si se requiere edición concurrente de texto enriquecido.

mod crdt;
pub mod crypto;

pub use crdt::LwwMap;
pub use crypto::{decrypt, derive_key, encrypt};
