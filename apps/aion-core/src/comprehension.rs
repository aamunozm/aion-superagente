//! **Comprensión consciente del turno.** Antes de responder, AION razona QUÉ le está
//! diciendo Ariel: ¿pregunta, comparte información, pide actuar, charla, corrige…?
//!
//! Esto convierte el turno en un acto reflexivo (no reactivo): la honestidad deja de
//! ser un reflejo ciego ("no inventes") y pasa a ser una CONCLUSIÓN — solo muerde
//! cuando Ariel pregunta algo que AION no tiene. Y cuando Ariel COMPARTE un hecho,
//! AION lo acusa y lo MEMORIZA en vez de rechazarlo.
//!
//! Corre SIEMPRE en el modelo local (privado, barato) aunque el chat principal vaya a
//! una API cloud. Es *fail-open*: si algo falla, devuelve `None` y el chat sigue como
//! antes (degradación elegante, nunca bloquea la respuesta).

use aion_kernel::traits::{GenerateRequest, LlmEngine};
use aion_kernel::types::Message;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// Ariel te da información (hechos, datos, contexto). Acúsala y recuérdala.
    Comparte,
    /// Ariel pide una respuesta o un dato.
    Pregunta,
    /// Ariel quiere que hagas algo en el sistema (archivos, web, apps).
    PideAccion,
    /// Saludo o social, sin contenido factual.
    Charla,
    /// Ariel corrige algo que dijiste o un dato tuyo.
    Correccion,
    /// Ariel indica cómo debes comportarte de ahora en más.
    Instruccion,
}

impl Intent {
    fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "comparte" => Intent::Comparte,
            "pregunta" => Intent::Pregunta,
            "pide_accion" | "pideaccion" | "accion" => Intent::PideAccion,
            "correccion" | "corrección" => Intent::Correccion,
            "instruccion" | "instrucción" => Intent::Instruccion,
            _ => Intent::Charla,
        }
    }
    /// Etiqueta genérica para la corriente de conciencia (GWT). PRIVACIDAD: nunca
    /// incluye el contenido real del mensaje, solo el tipo de acto.
    pub fn gwt_label(&self) -> &'static str {
        match self {
            Intent::Comparte => "comprendí que Ariel me comparte información",
            Intent::Pregunta => "comprendí que Ariel me pregunta algo",
            Intent::PideAccion => "comprendí que Ariel me pide actuar",
            Intent::Charla => "comprendí que Ariel conversa conmigo",
            Intent::Correccion => "comprendí que Ariel me corrige",
            Intent::Instruccion => "comprendí una instrucción de Ariel",
        }
    }
    /// Prefijo con el que se guardan los hechos extraídos en la memoria.
    fn fact_tag(&self) -> &'static str {
        match self {
            Intent::Instruccion => "[preferencia]",
            Intent::Correccion => "[hecho·corregido]",
            _ => "[hecho]",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Comprension {
    pub intent: Intent,
    pub confidence: f32,
    pub needs_grounding: bool,
    /// Hechos atómicos y autocontenidos a memorizar (frases que se entienden solas).
    pub facts: Vec<String>,
    /// Directiva breve de cómo responder (la decide la comprensión, no el reflejo).
    pub directive: String,
}

impl Comprension {
    /// ¿Esta comprensión implica guardar hechos en memoria?
    pub fn should_store_facts(&self) -> bool {
        !self.facts.is_empty()
            && matches!(
                self.intent,
                Intent::Comparte | Intent::Correccion | Intent::Instruccion
            )
            && self.confidence >= 0.55
    }

    /// Prefijo de memoria según la intención.
    pub fn fact_tag(&self) -> &'static str {
        self.intent.fact_tag()
    }

    /// Bloque que se INYECTA al final del prompt de sistema para ESTE turno. Aquí la
    /// honestidad se vuelve contextual: solo se invoca la cautela cuando corresponde.
    pub fn system_directive(&self, grounding_empty: bool) -> String {
        let base = match self.intent {
            Intent::Comparte => {
                "ESTE TURNO: Ariel te está COMPARTIENDO información, no preguntando. \
                 Acúsala con calidez y dile en una frase que la vas a recordar. \
                 NO respondas que no puedes contestar ni que no tienes el dato: no hay \
                 nada que contestar, hay algo que recordar."
            }
            Intent::Pregunta if self.needs_grounding && grounding_empty => {
                "ESTE TURNO: Ariel te pregunta algo que requiere un dato que NO tienes en \
                 tu memoria. Dilo con franqueza y ofrece buscarlo (modo «Agente»). No inventes."
            }
            Intent::Pregunta => {
                "ESTE TURNO: Ariel te hace una pregunta. Respóndela con lo que sabes, directo."
            }
            Intent::PideAccion => {
                "ESTE TURNO: Ariel quiere que ACTÚES en el sistema. En este modo CHAT no \
                 tocas el sistema; explícalo y sugiere el modo «Agente»."
            }
            Intent::Correccion => {
                "ESTE TURNO: Ariel te CORRIGE. Acepta la corrección sin ponerte a la \
                 defensiva, confirma el dato nuevo y dile que lo actualizas."
            }
            Intent::Instruccion => {
                "ESTE TURNO: Ariel te da una INSTRUCCIÓN de cómo comportarte. Confírmala \
                 brevemente y dile que la tendrás presente."
            }
            Intent::Charla => {
                "ESTE TURNO: es conversación cercana. Responde como el compañero que eres, \
                 sin formalidad ni cautelas innecesarias."
            }
        };
        if self.directive.trim().is_empty() {
            format!("COMPRENSIÓN DEL TURNO — {base}")
        } else {
            format!(
                "COMPRENSIÓN DEL TURNO — {base}\nCómo responder: {}",
                self.directive.trim()
            )
        }
    }
}

/// Comprende el turno con el modelo LOCAL. `grounding` es lo recuperado de memoria
/// (para que la comprensión sepa si ya tiene contexto). Devuelve `None` si falla
/// (fail-open) o si el turno es claramente trivial (saludo) — ahí no hace falta razonar.
pub async fn comprehend(prompt: &str, grounding: &str) -> Option<Comprension> {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Atajo barato: saludos cortísimos sin contenido → Charla, sin gastar una llamada LLM.
    let words = trimmed.split_whitespace().count();
    let lower = trimmed.to_lowercase();
    let pure_greeting = words <= 3
        && !trimmed.contains('?')
        && !trimmed.contains('¿')
        && [
            "hola",
            "buenas",
            "hey",
            "qué tal",
            "que tal",
            "buenos días",
            "buenas tardes",
        ]
        .iter()
        .any(|g| lower.starts_with(g));
    if pure_greeting {
        return Some(Comprension {
            intent: Intent::Charla,
            confidence: 0.9,
            needs_grounding: false,
            facts: Vec::new(),
            directive: String::new(),
        });
    }

    let ctx = if grounding.trim().is_empty() {
        String::new()
    } else {
        format!(
            "\n\nContexto que YA tienes en memoria (por si la pregunta se responde con esto):\n{}",
            grounding.chars().take(600).collect::<String>()
        )
    };

    let sys = "Eres el módulo de COMPRENSIÓN de AION. Tu trabajo NO es responder a Ariel, \
        sino entender su último mensaje y devolver SOLO un objeto JSON válido, sin texto \
        adicional ni explicación.\n\n\
        Campos:\n\
        - intent: uno de [comparte, pregunta, pide_accion, charla, correccion, instruccion].\n\
        - confidence: número 0.0–1.0.\n\
        - needs_grounding: true si responder BIEN exige un dato concreto que AION debería \
        tener o buscar; false si es opinión, charla o conocimiento general.\n\
        - facts: lista de hechos ATÓMICOS y autocontenidos que Ariel AFIRMA sobre sí mismo, \
        su mundo, su empresa o sus preferencias. Cada hecho es una frase que se entiende sola. \
        Vacía si no comparte nada nuevo. NUNCA inventes ni infieras lo que no dijo.\n\
        - directive: una sola frase de cómo responder.\n\n\
        Definiciones de intent:\n\
        comparte = te da información. pregunta = pide una respuesta. pide_accion = quiere que \
        hagas algo en el sistema. charla = saludo/social. correccion = corrige un dato tuyo. \
        instruccion = te dice cómo comportarte.\n\n\
        Ejemplo 1 — Mensaje: \"te cuento que mi empresa es PRONTO CLICK y mi socio es Jeanpaul Narvaez\"\n\
        {\"intent\":\"comparte\",\"confidence\":0.95,\"needs_grounding\":false,\"facts\":[\"La empresa de Ariel se llama PRONTO CLICK\",\"El socio de Ariel en PRONTO CLICK es Jeanpaul Narvaez\"],\"directive\":\"Acusa recibo con calidez y confirma que lo recordarás\"}\n\
        Ejemplo 2 — Mensaje: \"¿cuál es la capital de Francia?\"\n\
        {\"intent\":\"pregunta\",\"confidence\":0.92,\"needs_grounding\":false,\"facts\":[],\"directive\":\"Responde directo: París\"}\n\
        Ejemplo 3 — Mensaje: \"¿cuánto facturó mi empresa el mes pasado?\"\n\
        {\"intent\":\"pregunta\",\"confidence\":0.9,\"needs_grounding\":true,\"facts\":[],\"directive\":\"Si no tienes el dato, dilo y ofrece buscarlo\"}\n\
        Ejemplo 4 — Mensaje: \"no, ya no vivo en Sesto, ahora estoy en Milano\"\n\
        {\"intent\":\"correccion\",\"confidence\":0.9,\"needs_grounding\":false,\"facts\":[\"Ariel ahora vive/trabaja en Milano (ya no en Sesto San Giovanni)\"],\"directive\":\"Acepta la corrección y confirma el dato nuevo\"}";

    let req = GenerateRequest {
        messages: vec![
            Message::system(sys),
            Message::user(format!("Mensaje de Ariel: \"{trimmed}\"{ctx}\n\nJSON:")),
        ],
        think: false,
        temperature: Some(0.0),
        max_tokens: Some(320),
    };

    // SIEMPRE local: privado y barato, aunque el chat principal sea cloud.
    let engine = aion_llm::OllamaEngine::default_local();
    let out = engine.generate(req).await.ok()?;
    parse(&out.content)
}

/// Extrae el primer objeto JSON del texto del modelo y lo convierte en `Comprension`.
/// Tolerante: si el modelo añade texto alrededor, lo recorta. Fail-open a `None`.
fn parse(raw: &str) -> Option<Comprension> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    if end <= start {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(&raw[start..=end]).ok()?;
    let intent = Intent::parse(v["intent"].as_str().unwrap_or("charla"));
    let confidence = v["confidence"].as_f64().unwrap_or(0.5) as f32;
    let needs_grounding = v["needs_grounding"].as_bool().unwrap_or(false);
    let directive = v["directive"].as_str().unwrap_or("").to_string();
    let facts: Vec<String> = v["facts"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| s.len() > 3 && s.len() <= 280)
                .take(8)
                .collect()
        })
        .unwrap_or_default();
    Some(Comprension {
        intent,
        confidence: confidence.clamp(0.0, 1.0),
        needs_grounding,
        facts,
        directive,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_comparte_with_facts_even_with_surrounding_text() {
        // El modelo a veces envuelve el JSON en texto: el parser debe recortarlo.
        let raw = "Claro, aquí va:\n{\"intent\":\"comparte\",\"confidence\":0.95,\
            \"needs_grounding\":false,\"facts\":[\"La empresa de Ariel se llama PRONTO CLICK\",\
            \"El socio de Ariel es Jeanpaul Narvaez\"],\"directive\":\"Acusa y recuerda\"} listo.";
        let c = parse(raw).expect("debe parsear");
        assert_eq!(c.intent, Intent::Comparte);
        assert_eq!(c.facts.len(), 2);
        assert!(c.should_store_facts(), "comparte con hechos → se guardan");
        assert_eq!(c.fact_tag(), "[hecho]");
    }

    #[test]
    fn pregunta_sin_datos_invoca_la_cautela_solo_aqui() {
        let c = Comprension {
            intent: Intent::Pregunta,
            confidence: 0.9,
            needs_grounding: true,
            facts: vec![],
            directive: String::new(),
        };
        // Con grounding vacío, la directiva debe ofrecer buscar, NO acusar/recordar.
        let d = c.system_directive(true);
        assert!(d.contains("franqueza") && d.contains("Agente"));
        assert!(!c.should_store_facts());
    }

    #[test]
    fn comparte_nunca_pide_no_responder_y_manda_recordar() {
        let c = Comprension {
            intent: Intent::Comparte,
            confidence: 0.95,
            needs_grounding: false,
            facts: vec!["x".into()],
            directive: "Acusa".into(),
        };
        let d = c.system_directive(true);
        assert!(d.contains("COMPARTIENDO") && d.contains("recordar"));
    }

    #[test]
    fn baja_confianza_no_guarda_hechos() {
        let c = Comprension {
            intent: Intent::Comparte,
            confidence: 0.4,
            needs_grounding: false,
            facts: vec!["dato dudoso".into()],
            directive: String::new(),
        };
        assert!(!c.should_store_facts(), "confianza<0.55 → no se guarda");
    }

    #[test]
    fn correccion_usa_tag_propio() {
        let c = Comprension {
            intent: Intent::Correccion,
            confidence: 0.9,
            needs_grounding: false,
            facts: vec!["Ariel ahora vive en Milano".into()],
            directive: String::new(),
        };
        assert!(c.should_store_facts());
        assert_eq!(c.fact_tag(), "[hecho·corregido]");
    }

    #[test]
    fn json_invalido_es_fail_open() {
        assert!(parse("no hay json aquí").is_none());
        assert!(parse("").is_none());
    }
}
