//! **Enrutado SEMÁNTICO de «skills de documento»** (meaning-first) — decide qué skill aplica a un
//! mensaje del chat de proyecto: **auditoría SEO**, **propuesta analítica** u **oferta→documento**.
//!
//! El problema que resuelve (auditoría 2026-06-25): la cascada anterior decidía por LISTAS DE
//! PALABRAS solapadas con un orden de precedencia equivocado — `is_proposta` (el detector más
//! amplio: «análisis» + «quiero») iba PRIMERO y SECUESTRABA las peticiones de auditoría SEO
//! («quiero un análisis SEO» → generaba una *propuesta* en vez de la auditoría). Eso va contra la
//! doctrina meaning-first del proyecto (memoria `aion-routing-meaning-first`: corregir misruteos
//! con PROTOTIPOS semánticos, no con keywords).
//!
//! Diseño (igual que [`crate::intent`], el router charla/tarea): embebe el mensaje (BGE-M3) y lo
//! compara con PROTOTIPOS de cada skill + un conjunto NEUTRO (charla/otras tareas). Decide la skill
//! ganadora SOLO con confianza alta y margen claro sobre la 2ª skill y sobre lo neutro; en cualquier
//! duda devuelve `None` y deja que la **red de seguridad léxica** del handler (orden ya corregido:
//! SEO → oferta → propuesta) decida. Es CONSERVADOR a propósito: el semántico capta fraseos que el
//! léxico no cubre (p. ej. «en qué posición sale en Google» — sin la palabra «SEO»), y el léxico
//! cubre el caso si el embebedor no está disponible. Doble red, fail-soft.
//!
//! Coste: 1 embedding (~100-300 ms local) por mensaje de tarea; los prototipos se embeben UNA vez
//! (cacheados) y se precalientan al arrancar con [`warm`].

use aion_memory::cosine;
use tokio::sync::OnceCell;

/// Qué skill de documento pide el mensaje.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocSkill {
    /// Auditoría SEO de la web del cliente: lee la página, la puntúa y entrega un informe/PDF.
    Seo,
    /// Propuesta analítica de consultor (analiza el sitio + razona + redacta a medida).
    Proposta,
    /// Oferta ya hablada → renderizada como documento (PDF/Word).
    Offerta,
}

/// Prototipos de AUDITORÍA SEO: pedir el diagnóstico/puntuación/posición en buscadores. Fraseos
/// variados (con y sin la palabra «SEO») para captar el SIGNIFICADO, no una keyword.
const SEO_PROTOS: &[&str] = &[
    "haz una auditoría SEO de la página y dame la puntuación",
    "en qué posición aparece esta web en Google",
    "qué puntaje o score SEO tiene el sitio del cliente",
    "revisa el posicionamiento en buscadores de esta página",
    "verificaste el SEO de la página del cliente",
    "quiero un análisis SEO profesional con la posición en Google y la puntuación",
    "analiza el SEO on-page de esta URL y dime qué mejorar",
    "cómo está posicionada la web en los resultados de búsqueda",
];

/// Prototipos de PROPUESTA ANALÍTICA (consultor): redactar un documento comercial a medida.
const PROPOSTA_PROTOS: &[&str] = &[
    "redacta una propuesta analítica a medida para el cliente",
    "hazme un preventivo de consultor con la propuesta de servicios",
    "prepara una propuesta comercial completa y argumentada para el cliente",
    "necesito una propuesta de servicios con la inversión y las condiciones",
    "genera un preventivo profesional tipo consultor para este cliente",
];

/// Prototipos de OFERTA → DOCUMENTO: convertir en archivo una oferta ya conversada.
const OFFERTA_PROTOS: &[&str] = &[
    "pásame la oferta que hablamos en un PDF",
    "haz la oferta en un documento word",
    "convierte la oferta en un archivo descargable",
    "genérame la oferta en pdf con lo que acordamos",
    "exporta la oferta a un documento",
];

/// Prototipos NEUTROS: charla, preguntas generales y otras tareas que NO son estas tres skills.
/// Sirven de «suelo»: una skill solo gana si se parece MÁS al mensaje que cualquiera de estos.
const NEUTRO_PROTOS: &[&str] = &[
    "hola, ¿cómo estás hoy?",
    "cuéntame qué opinas de esto",
    "gracias, lo has hecho muy bien",
    "qué tiempo hace hoy en Milán",
    "resume el contenido de este documento",
    "qué dice esta fuente del proyecto",
    "busca en internet información sobre este tema",
    "añade una nota a este proyecto",
];

/// Similitud mínima absoluta para que un mensaje «sea» una skill (por debajo, no se fuerza).
const ABS_MIN: f32 = 0.52;
/// La skill debe ganar a lo NEUTRO por al menos esto (si no, se parece tanto a charla → no forzar).
const NEUTRO_MARGIN: f32 = 0.02;
/// Margen entre las DOS mejores skills para decidir sin ambigüedad (si empatan → léxico decide).
const SKILL_MARGIN: f32 = 0.05;

struct Protos {
    seo: Vec<Vec<f32>>,
    proposta: Vec<Vec<f32>>,
    offerta: Vec<Vec<f32>>,
    neutro: Vec<Vec<f32>>,
}

static PROTOS: OnceCell<Protos> = OnceCell::const_new();

async fn embed(text: &str) -> Vec<f32> {
    aion_memory::OllamaEmbedder::default_local()
        .embed(text)
        .await
        .unwrap_or_default()
}

async fn embed_all(texts: &[&str]) -> Vec<Vec<f32>> {
    let mut out = Vec::with_capacity(texts.len());
    for t in texts {
        out.push(embed(t).await);
    }
    out
}

async fn protos() -> &'static Protos {
    PROTOS
        .get_or_init(|| async {
            Protos {
                seo: embed_all(SEO_PROTOS).await,
                proposta: embed_all(PROPOSTA_PROTOS).await,
                offerta: embed_all(OFFERTA_PROTOS).await,
                neutro: embed_all(NEUTRO_PROTOS).await,
            }
        })
        .await
}

/// Máxima similitud coseno del mensaje contra un conjunto de prototipos (ignora los vacíos /
/// de dimensión distinta: si Ollama falló al embeber un prototipo, no contamina).
fn max_sim(q: &[f32], protos: &[Vec<f32>]) -> f32 {
    protos
        .iter()
        .filter(|p| p.len() == q.len() && !p.is_empty())
        .map(|p| cosine(q, p))
        .fold(0.0_f32, f32::max)
}

/// Puntuaciones de similitud por skill + neutro. Públicas para el endpoint de diagnóstico
/// (`/api/intent/doc`), que permite calibrar los umbrales en vivo.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Scores {
    pub seo: f32,
    pub proposta: f32,
    pub offerta: f32,
    pub neutro: f32,
}

/// Embebe el mensaje y devuelve las similitudes contra cada skill + neutro (o `None` si no se
/// pudo embeber).
pub async fn scores(msg: &str) -> Option<Scores> {
    let q = embed(msg).await;
    if q.is_empty() {
        return None;
    }
    let p = protos().await;
    Some(Scores {
        seo: max_sim(&q, &p.seo),
        proposta: max_sim(&q, &p.proposta),
        offerta: max_sim(&q, &p.offerta),
        neutro: max_sim(&q, &p.neutro),
    })
}

/// Aplica la regla de decisión a unas puntuaciones (lógica pura, testeable sin embeddings).
pub fn decide(s: &Scores) -> Option<DocSkill> {
    let mut ranked = [
        (DocSkill::Seo, s.seo),
        (DocSkill::Proposta, s.proposta),
        (DocSkill::Offerta, s.offerta),
    ];
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let (best, best_s) = ranked[0];
    let second_s = ranked[1].1;
    if best_s < ABS_MIN {
        return None; // no se parece de verdad a ninguna skill
    }
    if best_s - s.neutro < NEUTRO_MARGIN {
        return None; // se parece tanto a charla/otra tarea → no forzar
    }
    if best_s - second_s < SKILL_MARGIN {
        return None; // dos skills empatadas → que decida la red léxica del handler
    }
    Some(best)
}

/// **Decide la skill por SIGNIFICADO.** Conservador: `Some` solo con confianza alta y margen
/// claro; en cualquier duda → `None` (la red de seguridad léxica del handler decide). Fail-soft:
/// si no hay embeddings, también `None`.
pub async fn classify(msg: &str) -> Option<DocSkill> {
    decide(&scores(msg).await?)
}

/// Pre-calienta los prototipos (al arrancar) para que el primer mensaje no pague embeberlos.
pub fn warm() {
    tokio::spawn(async {
        let _ = protos().await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // Umbrales en una banda sensata (aserción en tiempo de compilación, como en `intent`).
    const _: () = assert!(ABS_MIN > 0.3 && ABS_MIN < 0.8);
    const _: () = assert!(NEUTRO_MARGIN > 0.0 && NEUTRO_MARGIN < 0.2);
    const _: () = assert!(SKILL_MARGIN > 0.0 && SKILL_MARGIN < 0.3);

    fn sc(seo: f32, proposta: f32, offerta: f32, neutro: f32) -> Scores {
        Scores {
            seo,
            proposta,
            offerta,
            neutro,
        }
    }

    #[test]
    fn seo_claro_gana() {
        // «quiero un análisis SEO con la posición en Google»: SEO alto, claro sobre el resto.
        assert_eq!(decide(&sc(0.74, 0.55, 0.40, 0.45)), Some(DocSkill::Seo));
    }

    #[test]
    fn oferta_a_documento_gana() {
        assert_eq!(decide(&sc(0.42, 0.50, 0.71, 0.44)), Some(DocSkill::Offerta));
    }

    #[test]
    fn propuesta_explicita_gana() {
        assert_eq!(
            decide(&sc(0.48, 0.72, 0.50, 0.45)),
            Some(DocSkill::Proposta)
        );
    }

    #[test]
    fn seo_y_propuesta_empatadas_no_deciden() {
        // Margen entre las dos mejores < SKILL_MARGIN → None (lo resuelve el léxico).
        assert_eq!(decide(&sc(0.67, 0.65, 0.40, 0.45)), None);
    }

    #[test]
    fn debil_no_fuerza() {
        // Ninguna supera ABS_MIN → None (cae al flujo normal/léxico).
        assert_eq!(decide(&sc(0.40, 0.38, 0.30, 0.35)), None);
    }

    #[test]
    fn parecido_a_charla_no_fuerza() {
        // La mejor skill apenas supera lo neutro → None (probablemente es charla).
        assert_eq!(decide(&sc(0.55, 0.40, 0.40, 0.54)), None);
    }

    #[test]
    fn max_sim_ignora_vacios_y_dim_distinta() {
        let q = vec![1.0, 0.0];
        let protos = vec![vec![], vec![1.0], vec![1.0, 0.0]];
        assert!((max_sim(&q, &protos) - 1.0).abs() < 1e-6);
    }
}
