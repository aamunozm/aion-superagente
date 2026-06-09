//! Captura de pantalla para que AION **vea** (los PNG se envían a Gemma visión).
//! En macOS usa `screencapture` (sin dependencias extra, requiere permiso de
//! "Grabación de pantalla"). Otras plataformas: pendiente (Windows/Linux).

use crate::ControlError;
use base64::Engine;

/// Captura la pantalla principal y devuelve los bytes PNG.
pub fn capture_png() -> Result<Vec<u8>, ControlError> {
    let tmp = std::env::temp_dir().join(format!("aion-screen-{}.png", uuid_like()));

    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("screencapture")
            .args(["-x", "-t", "png"])
            .arg(&tmp)
            .status()
            .map_err(|e| ControlError::Screen(format!("no se pudo ejecutar screencapture: {e}")))?;
        if !status.success() {
            return Err(ControlError::Screen(
                "screencapture falló (¿falta permiso de Grabación de pantalla?)".into(),
            ));
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        return Err(ControlError::Screen(
            "captura de pantalla aún no implementada en esta plataforma".into(),
        ));
    }

    #[allow(unreachable_code)]
    {
        let bytes = std::fs::read(&tmp)
            .map_err(|e| ControlError::Screen(format!("no se pudo leer la captura: {e}")))?;
        let _ = std::fs::remove_file(&tmp);
        Ok(bytes)
    }
}

/// Captura la pantalla y la devuelve como base64 (para la API de visión de Ollama).
pub fn capture_base64() -> Result<String, ControlError> {
    Ok(base64::engine::general_purpose::STANDARD.encode(capture_png()?))
}

/// Id corto y único para el nombre del archivo temporal.
fn uuid_like() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}
