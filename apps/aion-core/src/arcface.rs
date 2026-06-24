//! **ArcFace (InsightFace w600k_r50) vía onnxruntime (`ort`)** — el faceprint POTENTE (512 dim).
//!
//! Reemplaza el descriptor genérico de Vision por embeddings de identidad de ArcFace, mucho más
//! discriminativos para "quién es quién". El helper Swift entrega la cara ALINEADA 112×112; aquí se
//! pasa por el modelo ONNX y se normaliza L2. Se enchufa a `faces::observe` igual que antes.
//!
//! El modelo (~166MB) va como recurso del bundle (`arcface.onnx`); en dev, `AION_ARCFACE` lo apunta.
//!
//! **Portabilidad**: `ort` (onnxruntime) solo tiene binarios prebuilt para `aarch64-apple-darwin`
//! (no para `x86_64-apple-darwin`), y romper esto impedía construir el `.app` UNIVERSAL (arm64 +
//! Intel). Como el reconocimiento facial es solo-macOS-Silicon de todos modos, la implementación
//! REAL (con `ort`) se compila solo en arm64-mac; en cualquier otro target (Intel-mac, Windows,
//! Linux) se compila el STUB de abajo, que deshabilita la cara SIN arrastrar la dependencia `ort`.
//! `faces.rs` y sus llamantes no cambian: `embed` devuelve `None` y el pipeline degrada limpio.

#![allow(dead_code)]

// ── Implementación REAL (onnxruntime/ort) — solo arm64-apple-darwin ──────────────
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod imp {
    use ort::session::Session;
    use ort::value::Tensor;
    use std::sync::{Mutex, OnceLock};

    /// Ruta del modelo: `AION_ARCFACE` (dev) o el recurso del bundle (Contents/Resources/arcface.onnx).
    fn model_path() -> std::path::PathBuf {
        if let Ok(p) = std::env::var("AION_ARCFACE") {
            return std::path::PathBuf::from(p);
        }
        let exe = std::env::current_exe().ok();
        let dir = exe.as_ref().and_then(|e| e.parent());
        let candidates = [
            dir.map(|d| d.join("../Resources/arcface.onnx")),
            dir.map(|d| d.join("../Resources/_up_/arcface.onnx")),
            dir.map(|d| d.join("arcface.onnx")),
        ];
        for c in candidates.into_iter().flatten() {
            if c.exists() {
                return c;
            }
        }
        std::path::PathBuf::from("arcface.onnx")
    }

    /// Sesión cargada perezosamente (una vez). None si el modelo no está o falla el runtime.
    fn session() -> Option<&'static Mutex<Session>> {
        static S: OnceLock<Option<Mutex<Session>>> = OnceLock::new();
        S.get_or_init(|| {
            let path = model_path();
            let mut builder = match Session::builder() {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!("arcface: no pude crear el builder: {e}");
                    return None;
                }
            };
            match builder.commit_from_file(&path) {
                Ok(s) => {
                    tracing::info!("arcface: modelo cargado ({})", path.display());
                    Some(Mutex::new(s))
                }
                Err(e) => {
                    tracing::warn!("arcface: sin modelo en {}: {e}", path.display());
                    None
                }
            }
        })
        .as_ref()
    }

    /// ¿Está disponible el reconocimiento facial potente (modelo + runtime)?
    pub fn available() -> bool {
        session().is_some()
    }

    /// Faceprint ArcFace (512 dim, L2-normalizado) de una cara alineada 112×112 RGB ya en NCHW
    /// normalizado a [-1,1] (vec de 1·3·112·112). None si no hay modelo o falla la inferencia.
    pub fn embed(nchw: Vec<f32>) -> Option<Vec<f32>> {
        if nchw.len() != 3 * 112 * 112 {
            return None;
        }
        let mu = session()?;
        let mut s = mu.lock().unwrap_or_else(|e| e.into_inner());
        let input = Tensor::from_array(([1usize, 3, 112, 112], nchw)).ok()?;
        let outputs = s.run(ort::inputs![input]).ok()?;
        let (_shape, data) = outputs[0].try_extract_tensor::<f32>().ok()?;
        let mut v: Vec<f32> = data.to_vec();
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in &mut v {
                *x /= norm;
            }
        }
        Some(v)
    }
}

// ── STUB sin `ort` — Intel-mac, Windows, Linux ──────────────────────────────────
// El reconocimiento facial queda deshabilitado (no hay onnxruntime), pero todo COMPILA y el
// resto del pipeline (faces.rs) degrada con `None` sin ninguna rama especial en los llamantes.
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
mod imp {
    pub fn available() -> bool {
        false
    }
    pub fn embed(_nchw: Vec<f32>) -> Option<Vec<f32>> {
        None
    }
}

// `available` no se usa en todos los targets (en el stub la cara está deshabilitada): permitido.
#[allow(unused_imports)]
pub use imp::{available, embed};

// Los tests de inferencia solo aplican donde existe la implementación real (arm64-mac).
#[cfg(all(test, target_os = "macos", target_arch = "aarch64"))]
mod tests {
    use super::*;
    #[test]
    fn embed_produce_512_si_hay_modelo() {
        // Solo corre si AION_ARCFACE apunta a un modelo (valida runtime + inferencia e2e).
        if std::env::var("AION_ARCFACE").is_err() {
            return;
        }
        let dummy = vec![0.0f32; 3 * 112 * 112];
        let e = embed(dummy).expect("embed debe producir un faceprint");
        assert_eq!(e.len(), 512, "ArcFace debe dar 512 dim");
    }
}
