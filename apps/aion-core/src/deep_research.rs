//! **Investigación profunda multi-agente** — el salto de "una búsqueda" a un INFORME PROFESIONAL
//! cruzado. Cuando Ariel pide investigar a fondo, AION no se queda en un par de enlaces: despliega
//! un pipeline de varios agentes que (1) DESCOMPONE el tema en ángulos, (2) BUSCA en MUCHAS fuentes
//! diversas y creíbles (web, académico, foros, código, vídeo — sin depender de Wikipedia),
//! (3) LEE y destila cada fuente en paralelo (varios agentes lectores), y (4) CRUZA las fuentes y
//! redacta un informe profesional: qué está corroborado por varias fuentes, qué es de fuente única
//! y qué se contradice, con conclusiones y bibliografía.
//!
//! Es lento a propósito (lee decenas de páginas + síntesis): calidad de investigación real. Emite
//! progreso por el canal del agente para que Ariel vea el trabajo en vivo. Fail-soft: una fuente o
//! un lector que falle no tumba la investigación. Usa el motor activo (local o API) para los agentes.

use aion_browser::{SearchResult, WebClient};
use aion_kernel::traits::{GenerateRequest, LlmEngine};
use aion_kernel::types::Message;

/// Una nota destilada de una fuente leída (afirmaciones clave + de dónde salen).
struct Note {
    idx: usize,
    title: String,
    url: String,
    source: String,
    claims: String,
}

/// Cuántas fuentes LEER a fondo (extraer afirmaciones). Leer decenas de páginas con el LLM es el
/// cuello de botella; este tope acota el tiempo. El resto de lo reunido se cita igualmente.
const READ_CAP: usize = 24;
/// Lectores concurrentes (acota carga al modelo/red; el resto espera por lotes).
const READ_CONCURRENCY: usize = 5;

/// **Ejecuta la investigación profunda.** `max_sources` = cuántas URLs diversas reunir;
/// `emit(kind, text)` publica progreso ("thought"/"action"/"observation"). Devuelve el informe
/// en markdown. Pensada para correr dentro del modo Agente y transmitir sus fases en vivo.
pub async fn run<F>(
    engine: &dyn LlmEngine,
    web: &WebClient,
    topic: &str,
    max_sources: usize,
    emit: F,
) -> String
where
    F: Fn(&str, &str),
{
    // 1) DESCOMPONER el tema en ángulos complementarios.
    emit(
        "thought",
        "Descomponiendo el tema en ángulos de investigación…",
    );
    let angles = decompose(engine, topic).await;
    emit(
        "observation",
        &format!(
            "{} ángulos: {}",
            angles.len(),
            angles
                .iter()
                .map(|a| a.as_str())
                .collect::<Vec<_>>()
                .join(" · ")
        ),
    );

    // 2) BUSCAR en muchas fuentes diversas, por cada ángulo, en paralelo. Fusionar + dedup.
    emit(
        "action",
        "Buscando en múltiples fuentes (web, académico, foros, código, vídeo)…",
    );
    // ESCALONADO (no ráfaga): lanzar las 6 búsquedas a la vez gatillaba el rate-limit de DDG
    // (sobre todo en varias investigaciones seguidas). Secuencial con una pausa breve es gentil
    // con los buscadores y apenas cuesta tiempo (el cuello de botella es la LECTURA, no la búsqueda).
    let mut per_angle: Vec<Vec<SearchResult>> = Vec::new();
    for (i, a) in angles.iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        }
        per_angle.push(web.search_deep(a, 10).await);
    }
    let mut sources: Vec<SearchResult> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut host_count: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for rs in per_angle {
        for r in rs {
            if !seen.insert(r.url.clone()) {
                continue;
            }
            let host = r.url.split('/').nth(2).unwrap_or("").to_string();
            let c = host_count.entry(host).or_insert(0);
            if *c >= 3 {
                continue; // diversidad: máx 3 por dominio
            }
            *c += 1;
            sources.push(r);
            if sources.len() >= max_sources {
                break;
            }
        }
        if sources.len() >= max_sources {
            break;
        }
    }
    if sources.is_empty() {
        return "No pude reunir fuentes para investigar el tema (las búsquedas no devolvieron \
                resultados). Reformula el tema o inténtalo de nuevo en un momento."
            .into();
    }
    // Resumen por familia de fuente, para que Ariel vea la diversidad.
    let by_fam = family_breakdown(&sources);
    emit(
        "observation",
        &format!("Reuní {} fuentes diversas ({by_fam}).", sources.len()),
    );

    // REBALANCEO POR FAMILIA antes de cortar a READ_CAP: las fuentes se reunieron ángulo a ángulo,
    // así que los primeros READ_CAP venían sesgados a los primeros ángulos y a la familia más
    // prolífica (web), dejando sin LEER (y por tanto sin CITAR) académico/código/vídeo de ángulos
    // tardíos. Round-robin estable por familia → el corte a 18 queda diverso de verdad.
    let sources = interleave_by_family(sources);

    // 3) LEER y destilar cada fuente en paralelo (agentes lectores), por lotes acotados.
    let to_read = sources.len().min(READ_CAP);
    emit(
        "action",
        &format!("Leyendo {to_read} fuentes y extrayendo sus afirmaciones clave…"),
    );
    // CONCURRENCIA FLUIDA con SEMÁFORO en vez de lotes con barrera: se lanzan todas las lecturas,
    // pero cada una espera un permiso (máx READ_CONCURRENCY en vuelo). En cuanto una termina, libera
    // el permiso y entra la siguiente — sin la barrera de `chunks` + join_all, donde un PDF lento
    // (timeout 20s) bloqueaba a los otros del lote. idx provisional 0: se renumera estable después.
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(READ_CONCURRENCY));
    let futs = sources[..to_read].iter().map(|sr| {
        let sem = sem.clone();
        async move {
            let _permit = sem.acquire().await.ok()?;
            read_source(engine, web, sr, topic, 0).await
        }
    });
    let mut notes: Vec<Note> = futures_util::future::join_all(futs)
        .await
        .into_iter()
        .flatten()
        .collect();
    // RENUMERA [1..N] estable y único: la asignación por lote (`notes.len() + j + 1`) solo
    // contaba los ÉXITOS, así que dos lotes podían repetir el mismo índice → citas [n]
    // duplicadas/incoherentes en el informe (claim citando una fuente y la bibliografía otra).
    for (i, n) in notes.iter_mut().enumerate() {
        n.idx = i + 1;
    }
    if notes.is_empty() {
        return format!(
            "Reuní {} fuentes pero no pude leer su contenido (páginas no accesibles o sin texto \
             útil). Las fuentes eran:\n{}",
            sources.len(),
            sources
                .iter()
                .take(10)
                .map(|s| format!("- {} — {}", s.title, s.url))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
    emit(
        "thought",
        &format!(
            "Leí {} fuentes con sustancia. Cruzando: qué se corrobora, qué se contradice…",
            notes.len()
        ),
    );

    // 4) CRUZAR + redactar el informe profesional (síntesis con verificación cruzada).
    emit("action", "Redactando el informe profesional cruzado…");
    synthesize(engine, topic, &notes).await
}

/// Descompone el tema en 4-6 ángulos complementarios. Fallback: el propio tema.
async fn decompose(engine: &dyn LlmEngine, topic: &str) -> Vec<String> {
    let req = GenerateRequest {
        messages: vec![
            Message::system(
                "Eres un investigador experto preparando CONSULTAS DE BÚSQUEDA. Descompón el TEMA \
                 en 5-7 ángulos COMPLEMENTARIOS (estado del arte, evidencia académica, práctica \
                 real, herramientas, debate). Para CADA ángulo, da una CONSULTA DE BÚSCADOR CORTA \
                 (3-7 palabras clave, NO una pregunta larga). Como muchas fuentes (papers, GitHub, \
                 foros tech) son anglófonas, escribe las consultas en INGLÉS con los términos \
                 técnicos exactos (añade el año si aplica). Responde SOLO la lista, una consulta \
                 por línea, sin numerar ni explicaciones.",
            ),
            Message::user(format!(
                "Tema: {topic}\n\nConsultas de búsqueda (cortas, en inglés, una por línea):"
            )),
        ],
        think: false,
        temperature: Some(0.4),
        max_tokens: Some(320),
    };
    let raw = match engine.generate(req).await {
        Ok(m) => m.content,
        Err(_) => return vec![topic.to_string()],
    };
    let mut angles: Vec<String> = raw
        .lines()
        .map(|l| {
            l.trim()
                .trim_start_matches(|c: char| {
                    c == '-'
                        || c == '*'
                        || c == '•'
                        || c == '.'
                        || c == ')'
                        || c == ' '
                        || c.is_ascii_digit()
                })
                // El LLM casi siempre DEVUELVE cada consulta ENTRECOMILLADA ("..." o «...»). Esas
                // comillas se mandan al buscador como FRASE EXACTA (%22…%22) y vacían TODAS las
                // fuentes → 0 resultados (era la causa raíz de que la investigación saliera vacía).
                // Las quitamos en ambos extremos; sin esto, la búsqueda profunda no encuentra nada.
                .trim_matches(|c: char| {
                    matches!(c, '"' | '\'' | '«' | '»' | '`' | '\u{201c}' | '\u{201d}')
                        || c.is_whitespace()
                })
                .trim()
                .to_string()
        })
        .filter(|l| l.chars().count() >= 8)
        .take(8)
        .collect();
    if angles.is_empty() {
        angles.push(topic.to_string());
    }
    angles
}

/// Lee UNA fuente (fetch) y destila sus afirmaciones clave con el LLM. `None` si no aporta.
async fn read_source(
    engine: &dyn LlmEngine,
    web: &WebClient,
    sr: &SearchResult,
    topic: &str,
    idx: usize,
) -> Option<Note> {
    // Lectura PROFUNDA: presupuesto amplio (12k) + soporte PDF (fuentes académicas). Antes
    // fetch_text recortaba a 4k y no leía PDFs → muchas fuentes rendían "NADA".
    let fetched = web
        .fetch_readable(&sr.url, 12_000)
        .await
        .unwrap_or_default();
    let text = if fetched.chars().count() >= 80 {
        fetched
    } else if sr.snippet.chars().count() >= 120 {
        // No se pudo leer la página (PDF de pago, muro, JS): usa el abstract/snippet como
        // RESPALDO para que la fuente (sobre todo papers académicos) contribuya igualmente.
        sr.snippet.clone()
    } else {
        return None; // ni página ni abstract útiles
    };
    let req = GenerateRequest {
        messages: vec![
            Message::system(
                "Extrae del TEXTO las afirmaciones CLAVE, concretas y verificables que sean \
                 relevantes al tema, en viñetas concisas (máx. 4). Datos, cifras, conclusiones o \
                 recomendaciones — nada de relleno. Si el texto no aporta nada relevante, responde \
                 EXACTAMENTE «NADA». No inventes ni añadas lo que no esté en el texto.",
            ),
            Message::user(format!(
                "Tema: {topic}\nFuente [{idx}] ({}): {}\n\nTEXTO:\n{}",
                sr.source,
                sr.title,
                text.chars().take(7000).collect::<String>()
            )),
        ],
        think: false,
        temperature: Some(0.2),
        max_tokens: Some(340),
    };
    let claims = engine.generate(req).await.ok()?.content.trim().to_string();
    let up = claims.to_uppercase();
    if claims.chars().count() < 20 || (up.contains("NADA") && claims.chars().count() < 30) {
        return None;
    }
    Some(Note {
        idx,
        title: sr.title.chars().take(120).collect(),
        url: sr.url.clone(),
        source: sr.source.clone(),
        claims,
    })
}

/// Cruza las notas y redacta el informe profesional (verificación cruzada en la síntesis).
async fn synthesize(engine: &dyn LlmEngine, topic: &str, notes: &[Note]) -> String {
    let corpus = notes
        .iter()
        .map(|n| {
            format!(
                "[{}] {} ({}) — {}\n{}",
                n.idx, n.title, n.source, n.url, n.claims
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let req = GenerateRequest {
        messages: vec![
            Message::system(
                "Eres un analista de investigación senior. Con las NOTAS de múltiples fuentes \
                 (cada una con su [n], tipo y URL), redacta un INFORME PROFESIONAL en español, en \
                 markdown, con estas secciones EXACTAS:\n\
                 ## Resumen ejecutivo\n\
                 ## Hallazgos clave — cada hallazgo indicando entre corchetes las fuentes que lo \
                 respaldan; marca CORROBORADO si lo sostienen ≥2 fuentes [n, m], o «fuente única \
                 [n]» si solo una.\n\
                 ## Discrepancias y debate — dónde las fuentes se contradicen o hay incertidumbre.\n\
                 ## Conclusiones — qué se puede afirmar con confianza y qué queda abierto.\n\
                 ## Fuentes — lista «[n] título — URL».\n\
                 CRUZA las fuentes: prioriza lo corroborado, señala lo dudoso y lo de fuente única. \
                 NO inventes nada que no esté en las notas. Riguroso, claro y útil.",
            ),
            Message::user(format!("Tema: {topic}\n\nNOTAS:\n{corpus}\n\nInforme:")),
        ],
        think: false,
        temperature: Some(0.5),
        max_tokens: Some(3200),
    };
    match engine.generate(req).await {
        Ok(m) => m.content.trim().to_string(),
        Err(e) => format!("Reuní y leí las fuentes, pero no pude redactar el informe final: {e}"),
    }
}

/// Resumen «N web · N académico · N foro …» para mostrar la diversidad reunida.
fn family_breakdown(sources: &[SearchResult]) -> String {
    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for s in sources {
        *counts.entry(s.source.clone()).or_insert(0) += 1;
    }
    counts
        .iter()
        .map(|(k, v)| format!("{v} {k}"))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Reordena las fuentes en ROUND-ROBIN por familia ("web", "académico", "foro", "código",
/// "vídeo"…), estable dentro de cada familia. Sirve para que un corte posterior (READ_CAP) tome
/// fuentes DIVERSAS en vez de agotar la familia más prolífica primero. No pierde ni duplica fuentes.
fn interleave_by_family(sources: Vec<SearchResult>) -> Vec<SearchResult> {
    use std::collections::{BTreeMap, VecDeque};
    let mut by_family: BTreeMap<String, VecDeque<SearchResult>> = BTreeMap::new();
    for s in sources {
        by_family.entry(s.source.clone()).or_default().push_back(s);
    }
    let mut out = Vec::new();
    while by_family.values().any(|q| !q.is_empty()) {
        for q in by_family.values_mut() {
            if let Some(s) = q.pop_front() {
                out.push(s);
            }
        }
    }
    out
}

/// **¿Es una petición de INVESTIGACIÓN PROFUNDA?** (no una búsqueda rápida). Conservador: solo
/// dispara el pipeline pesado ante señales claras de "a fondo / investigación / informe / foros",
/// para no convertir un simple «busca X» en 10 minutos de trabajo.
pub fn is_deep_research(task: &str) -> bool {
    let t = task.to_lowercase();
    const STRONG: &[&str] = &[
        "investigación profunda",
        "investigacion profunda",
        "investiga a fondo",
        "investigación exhaustiva",
        "investigacion exhaustiva",
        "investiga en profundidad",
        "a fondo en internet",
        "deep research",
        "deep search",
        "investigación completa",
        "investigacion completa",
        "haz un informe",
        "elabora un informe",
        "informe profesional",
        "investiga a profundidad",
        "ricerca approfondita",
    ];
    if STRONG.iter().any(|k| t.contains(k)) {
        return true;
    }
    // REGLA COMBINATORIA (fix): una señal de PROFUNDIDAD + una de BÚSQUEDA/INVESTIGACIÓN dispara el
    // pipeline pesado. Antes solo se reconocían frases fijas con "investiga…", así que la forma más
    // natural —«haz una BÚSQUEDA profunda de X», «busca a fondo Y», «búsqueda exhaustiva de Z»—
    // caía al camino rápido (una sola web_search, sin lectura cruzada) y rendía un informe pobre.
    let profundidad = t.contains("profund") || t.contains("exhaustiv") || t.contains("a fondo");
    let buscar = t.contains("investiga")
        || t.contains("busca")
        || t.contains("buscar")
        || t.contains("búsqueda")
        || t.contains("busqueda")
        || t.contains("research")
        || t.contains("informe")
        || t.contains("reporte");
    if profundidad && buscar {
        return true;
    }
    // "investiga(r)/investigación" + señal de amplitud (foros, varias fuentes, internet+foros).
    let investiga =
        t.contains("investiga") || t.contains("investigación") || t.contains("investigacion");
    let amplitud = t.contains("foros")
        || t.contains("varias fuentes")
        || t.contains("múltiples fuentes")
        || t.contains("multiples fuentes")
        || (t.contains("internet") && t.contains("foro"));
    investiga && amplitud
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_deep_research_requests() {
        assert!(is_deep_research(
            "quiero que hagas una investigación profunda en internet y foros sobre este tema"
        ));
        assert!(is_deep_research("investiga a fondo las técnicas de RAG"));
        assert!(is_deep_research(
            "haz un informe sobre el mercado de agentes"
        ));
        assert!(is_deep_research("investiga en foros qué opinan de esto"));
        // Forma natural «BÚSQUEDA profunda» (la que fallaba): debe disparar el pipeline.
        assert!(is_deep_research(
            "hace una busqueda profunda de filosofia para agentes ai 2026"
        ));
        assert!(is_deep_research(
            "haz una búsqueda profunda de filosofía para agentes de IA"
        ));
        assert!(is_deep_research(
            "busca a fondo sobre arquitecturas de transformers"
        ));
        assert!(is_deep_research("búsqueda exhaustiva de papers sobre RAG"));
    }

    #[test]
    fn interleaves_sources_by_family_for_balanced_reading() {
        let mk = |fam: &str, n: usize| SearchResult {
            title: format!("{fam}{n}"),
            url: format!("https://{fam}{n}.example"),
            snippet: String::new(),
            source: fam.into(),
        };
        // Reunidas sesgadas: 5 "web" seguidas y solo 1 de cada familia minoritaria al final
        // (lo que pasa al concatenar ángulos). Con corte directo a 3 se leerían 3 "web".
        let sources = vec![
            mk("web", 1),
            mk("web", 2),
            mk("web", 3),
            mk("web", 4),
            mk("web", 5),
            mk("académico", 1),
            mk("código", 1),
        ];
        let out = interleave_by_family(sources);
        // Las 3 primeras (lo que un READ_CAP pequeño leería) cubren las 3 familias, no 3 "web".
        let top3: Vec<&str> = out.iter().take(3).map(|s| s.source.as_str()).collect();
        assert!(top3.contains(&"web"));
        assert!(top3.contains(&"académico"));
        assert!(top3.contains(&"código"));
        // Ni se pierden ni se duplican fuentes.
        assert_eq!(out.len(), 7);
    }

    #[test]
    fn ignores_simple_searches() {
        // Una búsqueda rápida NO debe disparar el pipeline pesado (sería lentísimo).
        assert!(!is_deep_research("busca en internet el precio del bitcoin"));
        assert!(!is_deep_research("¿qué temperatura hace en Milán?"));
        assert!(!is_deep_research("cuántos habitantes tiene Tokio"));
        // "búsqueda" sin señal de profundidad = búsqueda rápida, NO el pipeline pesado.
        assert!(!is_deep_research(
            "haz una búsqueda rápida de noticias de hoy"
        ));
        assert!(!is_deep_research("busca el horario del tren a Roma"));
    }

    /// E2E REAL (ignorado por defecto: necesita Ollama + red). Corre el pipeline COMPLETO con la
    /// consulta exacta que antes caía al camino rápido y comprueba que produce un informe amplio
    /// y multi-fuente. Ejecutar: `cargo test -p aion-core e2e_deep_research_real_report -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "e2e: requiere Ollama corriendo y acceso a internet"]
    async fn e2e_deep_research_real_report() {
        let cfg = crate::provider::load();
        // Mismo motor que usa la app (build_engine): externo si está configurado, si no Ollama.
        let engine: Box<dyn aion_kernel::traits::LlmEngine> =
            if cfg.kind == "external" && !cfg.api_key.is_empty() && !cfg.base_url.is_empty() {
                Box::new(aion_llm::OpenAiEngine::new(
                    &cfg.base_url,
                    &cfg.api_key,
                    &cfg.model,
                ))
            } else {
                Box::new(aion_llm::OllamaEngine::new(
                    aion_llm::OllamaEngine::base_url_from_env(),
                    &cfg.model,
                ))
            };
        let web = aion_browser::WebClient::default();
        let q = "hace una busqueda profunda de filosofia para agentes ai 2026";
        assert!(
            is_deep_research(q),
            "el detector debe enrutar esto al pipeline profundo"
        );
        let report = run(&*engine, &web, q, 36, |k, t| println!("[{k}] {t}")).await;
        println!(
            "\n===== INFORME ({} chars) =====\n{}",
            report.chars().count(),
            report
        );
        assert!(
            report.chars().count() > 1500,
            "informe demasiado corto para ser 'profundo'"
        );
        assert!(
            report.contains("##"),
            "el informe debe traer secciones markdown"
        );
    }
}
