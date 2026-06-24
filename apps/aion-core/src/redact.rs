//! **Redacción de secretos/PII en los EGRESOS remotos.**
//!
//! AION es local-first: el chat con Gemma corre on-device y su memoria es privada. Pero hay dos
//! egresos donde el texto sale del Mac a un modelo de PAGO/REMOTO: (1) el puente MCP a Claude Code
//! (Anthropic) y (2) un proveedor LLM externo opcional (DeepSeek u otro). Como en la memoria pueden
//! colarse datos confidenciales (IBAN, tarjetas, claves de API, contraseñas), este módulo los
//! REDACTA de forma DETERMINISTA (sin LLM, sin latencia, local) justo antes de cada egreso remoto.
//!
//! Principios:
//! - **Solo egresos remotos.** Nunca se aplica a la ruta local de Gemma (privada y gratis), donde
//!   AION usa su memoria íntegra. Redactar ahí dañaría su contexto sin ganar privacidad.
//! - **Irreversible.** Se reemplaza por un marcador (`[IBAN]`, `[TARJETA]`, `[CLAVE]`); no se guarda
//!   ningún mapa reversible que pudiera filtrarse. El valor jamás sale.
//! - **Preciso, no agresivo.** Detectores con validación (Luhn, mod-97, prefijos conocidos,
//!   etiquetas inequívocas) → bajísimo falso positivo, para no destrozar el contexto legítimo
//!   (clave para no perder calidad). Lo que no sea detectable de forma fiable se cubre con la
//!   etiqueta `[confidencial]` (deny duro en el puente) y, a futuro, la bóveda cifrada.

use aion_kernel::errors::Result;
use aion_kernel::traits::{GenerateRequest, LlmEngine, StreamChunk};
use aion_kernel::types::Message;
use async_trait::async_trait;
use regex::Regex;
use std::borrow::Cow;
use std::sync::{Arc, OnceLock};

/// Claves de API por PREFIJO conocido + JWT. Específico → casi cero falso positivo.
fn apikey_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
            r"(?x)
            \b(
                sk-ant-[A-Za-z0-9_-]{20,}            # Anthropic
              | sk-[A-Za-z0-9]{20,}                  # OpenAI y similares
              | gh[pousr]_[A-Za-z0-9]{36,}           # GitHub PAT/OAuth
              | github_pat_[A-Za-z0-9_]{40,}
              | AKIA[A-Z0-9]{16}                     # AWS access key id
              | ASIA[A-Z0-9]{16}
              | xox[baprs]-[A-Za-z0-9-]{10,}         # Slack
              | AIza[A-Za-z0-9_-]{35}                # Google API key
              | ya29\.[A-Za-z0-9_-]{20,}             # Google OAuth
              | glpat-[A-Za-z0-9_-]{20,}             # GitLab PAT
              | eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}  # JWT
            )\b",
        )
        .unwrap()
    })
}

/// Secretos ETIQUETADOS: solo etiquetas inequívocas (no "clave"/"token" sueltos, comunes en
/// español). Captura etiqueta+separador y redacta el VALOR que sigue (entre comillas o sin espacios).
fn labeled_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
            r#"(?ix)
            \b(password|passwd|pwd|contrase\x{00f1}a|contrasena
              |api[ _-]?key|access[ _-]?key|secret[ _-]?key|private[ _-]?key
              |client[ _-]?secret|secret[ _-]?access[ _-]?key)
            (\s*[:=]\s*)
            (?:"[^"\n]{1,200}"|'[^'\n]{1,200}'|[^\s"'\n]{1,200})"#,
        )
        .unwrap()
    })
}

/// Candidatos a tarjeta de crédito: 13–19 dígitos con separadores opcionales. Se valida con Luhn.
fn card_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\b\d(?:[ -]?\d){12,18}\b").unwrap())
}

/// Candidatos a IBAN: 2 letras + 2 dígitos + 11–30 alfanum. Se valida con mod-97.
fn iban_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\b[A-Z]{2}\d{2}[A-Z0-9]{11,30}\b").unwrap())
}

/// Validación Luhn (tarjetas). Reduce el falso positivo ~10× frente a "cualquier 16 dígitos".
fn luhn_ok(s: &str) -> bool {
    let ds: Vec<u32> = s.chars().filter_map(|c| c.to_digit(10)).collect();
    if ds.len() < 13 || ds.len() > 19 {
        return false;
    }
    let mut sum = 0u32;
    let mut alt = false;
    for &d in ds.iter().rev() {
        let mut v = d;
        if alt {
            v *= 2;
            if v > 9 {
                v -= 9;
            }
        }
        sum += v;
        alt = !alt;
    }
    sum % 10 == 0
}

/// Validación IBAN por mod-97 (ISO 13616): reordena, mapea letras→números y exige resto 1.
fn iban_ok(s: &str) -> bool {
    let s: String = s.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if s.len() < 15 || s.len() > 34 {
        return false;
    }
    let (head, tail) = s.split_at(4);
    let mut rem: u64 = 0;
    for c in tail.chars().chain(head.chars()) {
        if let Some(d) = c.to_digit(10) {
            rem = (rem * 10 + d as u64) % 97;
        } else if c.is_ascii_alphabetic() {
            let v = (c.to_ascii_uppercase() as u8 - b'A') as u64 + 10;
            rem = (rem * 100 + v) % 97;
        } else {
            return false;
        }
    }
    rem == 1
}

/// **Redacta secretos/PII de un texto que va a salir a un modelo remoto.** Determinista, sin LLM.
/// Devuelve `Cow::Borrowed` si no encontró nada (cero copia → el caller no toca el mensaje).
pub fn redact_secrets(text: &str) -> Cow<'_, str> {
    // Cada pasada solo reasigna si su regex matchea; al final, si nada cambió, Borrowed.
    let s = labeled_re().replace_all(text, "$1$2[CLAVE]");
    let s = apikey_re().replace_all(&s, "[CLAVE]").into_owned();
    let s = card_re().replace_all(&s, |c: &regex::Captures| {
        if luhn_ok(&c[0]) {
            "[TARJETA]".to_string()
        } else {
            c[0].to_string()
        }
    });
    let s = iban_re().replace_all(&s, |c: &regex::Captures| {
        if iban_ok(&c[0]) {
            "[IBAN]".to_string()
        } else {
            c[0].to_string()
        }
    });
    if s == text {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(s.into_owned())
    }
}

/// ¿El recuerdo está marcado CONFIDENCIAL? Contiene `[confidencial]` (o `[confidential]`),
/// case-insensitive. El puente MCP NUNCA sirve estos recuerdos a un modelo remoto (deny DURO,
/// además de la redacción de secretos). Para guardar algo así: que su contenido lleve la etiqueta.
pub fn is_confidential(content: &str) -> bool {
    let c = content.to_lowercase();
    c.contains("[confidencial]") || c.contains("[confidential]")
}

/// **Decorador de motor LLM que REDACTA cada mensaje antes de enviarlo a un motor REMOTO/externo.**
/// Se envuelve SOLO alrededor del proveedor externo (DeepSeek/OpenAI…), nunca del Gemma local. Así,
/// aunque el usuario active un LLM externo de pago, los secretos de la memoria/grounding/usuario se
/// redactan antes de salir del Mac → privacidad máxima por construcción, en un único punto.
pub struct RedactingEngine {
    inner: Arc<dyn LlmEngine>,
}

impl RedactingEngine {
    pub fn new(inner: Arc<dyn LlmEngine>) -> Self {
        Self { inner }
    }

    /// Envuelve un motor remoto. (Helper para los call-sites que construyen el proveedor externo.)
    pub fn wrap(inner: Arc<dyn LlmEngine>) -> Arc<dyn LlmEngine> {
        Arc::new(Self::new(inner))
    }

    fn scrub(mut req: GenerateRequest) -> GenerateRequest {
        for m in req.messages.iter_mut() {
            if let Cow::Owned(red) = redact_secrets(&m.content) {
                m.content = red;
            }
        }
        req
    }
}

#[async_trait]
impl LlmEngine for RedactingEngine {
    fn id(&self) -> &str {
        self.inner.id()
    }
    async fn generate(&self, req: GenerateRequest) -> Result<Message> {
        self.inner.generate(Self::scrub(req)).await
    }
    async fn generate_stream(
        &self,
        req: GenerateRequest,
        on_chunk: Box<dyn FnMut(StreamChunk) + Send>,
    ) -> Result<()> {
        self.inner.generate_stream(Self::scrub(req), on_chunk).await
    }
    async fn health(&self) -> Result<()> {
        self.inner.health().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_iban_card_keys_passwords() {
        // IBAN español válido (mod-97 correcto).
        let iban = "ES9121000418450200051332";
        assert_eq!(
            redact_secrets(&format!("mi cuenta {iban}")),
            "mi cuenta [IBAN]"
        );
        // Visa de prueba (Luhn válido).
        assert_eq!(
            redact_secrets("tarjeta 4111 1111 1111 1111 ok"),
            "tarjeta [TARJETA] ok"
        );
        // Claves por prefijo.
        assert_eq!(
            redact_secrets("key sk-ant-abc123DEF456ghi789jkl0 fin"),
            "key [CLAVE] fin"
        );
        assert_eq!(
            redact_secrets("token ghp_0123456789abcdefghijklmnopqrstuvwxyz fin"),
            "token [CLAVE] fin"
        );
        // Etiquetado fuerte.
        assert_eq!(redact_secrets("password: hunter2"), "password: [CLAVE]");
        assert_eq!(
            redact_secrets("contraseña = MiP4ss!"),
            "contraseña = [CLAVE]"
        );
    }

    #[test]
    fn does_not_over_redact_normal_spanish() {
        // "clave" como palabra común NO se toca (no es etiqueta fuerte).
        let s = "la clave del éxito es trabajar con arquitectura modular y memoria local";
        assert_eq!(redact_secrets(s), s);
        // Números normales (no pasan Luhn / no son IBAN).
        let s2 = "el proyecto tiene 1234 commits y 56789 líneas en 2026";
        assert_eq!(redact_secrets(s2), s2);
        // Texto sin secretos → Borrowed (cero copia).
        assert!(matches!(redact_secrets("hola mundo"), Cow::Borrowed(_)));
    }

    #[test]
    fn luhn_and_iban_reject_invalid() {
        // 16 dígitos que NO pasan Luhn → no se redacta.
        assert_eq!(
            redact_secrets("ref 1234567812345678 x"),
            "ref 1234567812345678 x"
        );
        // IBAN con checksum roto → no se redacta.
        assert_eq!(
            redact_secrets("ES0000000000000000000000 x"),
            "ES0000000000000000000000 x"
        );
    }
}
