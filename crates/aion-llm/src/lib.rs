//! # aion-llm
//!
//! Capa de inferencia LLM de AION. Implementa el trait [`aion_kernel::LlmEngine`].
//!
//! - F1: [`OllamaEngine`] — reusa el modelo `gemma4-reason` (Gemma 4 12B abliterated)
//!   servido por Ollama en `:11434`. Soporta razonamiento (thinking) en streaming.
//! - F2: `MistralRsEngine` (embebido) — pendiente.
//! - F6: motores móviles (MLX/Candle) — pendiente.

mod ollama;
mod openai;

pub use ollama::OllamaEngine;
pub use openai::OpenAiEngine;

/// Extrae la siguiente línea COMPLETA (hasta `\n`) de un búfer de bytes de streaming,
/// devolviéndola como `String` y consumiéndola del búfer (incluido el `\n`).
///
/// Por qué bytes y no `String`: decodificar CADA chunk del stream por separado con
/// `from_utf8_lossy` corrompe los caracteres multibyte (ñ, é, emojis) que quedan PARTIDOS
/// entre dos chunks — cada mitad se vuelve «�». Bufferizando bytes crudos y decodificando
/// solo líneas completas se evita: el delimitador `\n` (0x0A) nunca aparece dentro de una
/// secuencia UTF-8 multibyte, y una línea completa SIEMPRE es UTF-8 válido. Devuelve `None`
/// si aún no hay un `\n` en el búfer (línea parcial: esperar más bytes).
pub(crate) fn take_line(buf: &mut Vec<u8>) -> Option<String> {
    let nl = buf.iter().position(|&b| b == b'\n')?;
    let line = String::from_utf8_lossy(&buf[..nl]).trim().to_string();
    buf.drain(..=nl);
    Some(line)
}

#[cfg(test)]
mod tests {
    use super::take_line;

    #[test]
    fn take_line_handles_multibyte_split_across_chunks() {
        // "café\n" en UTF-8: 'é' son 2 bytes (0xC3 0xA9). Simulamos que el chunk se corta
        // JUSTO en medio de 'é'. Con from_utf8_lossy por-chunk saldría "caf��"; con bytes
        // crudos + línea completa, sale "café" intacto.
        let mut buf: Vec<u8> = Vec::new();
        let full = "café\n".as_bytes();
        let cut = full.len() - 3; // parte dentro de 'é'
        buf.extend_from_slice(&full[..cut]);
        assert!(take_line(&mut buf).is_none(), "línea parcial: aún sin \\n");
        buf.extend_from_slice(&full[cut..]); // llega el resto
        assert_eq!(take_line(&mut buf).as_deref(), Some("café"));
        assert!(buf.is_empty());
    }

    #[test]
    fn take_line_multiple_and_remainder() {
        let mut buf: Vec<u8> = b"uno\ndos\ntre".to_vec();
        assert_eq!(take_line(&mut buf).as_deref(), Some("uno"));
        assert_eq!(take_line(&mut buf).as_deref(), Some("dos"));
        assert!(take_line(&mut buf).is_none()); // "tre" sin \n queda en el búfer
        assert_eq!(buf, b"tre");
    }
}
