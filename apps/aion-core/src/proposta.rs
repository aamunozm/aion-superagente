//! **Skill experta: «Proposta analitica» (nivel consultor, tipo PREV-2026-030).**
//!
//! No rellena una plantilla: ANALIZA el sitio del cliente (SEO real), RAZONA su situación y
//! REDACTA una propuesta a medida con la estructura de un consultor (fotografía de hoy → problemas
//! → propuesta → inversión → por qué conviene → condiciones). La MARCA del documento es DINÁMICA:
//! se extrae de las fuentes del proyecto (empresa que emite); si no hay, cae a AION con un pie que
//! aclara que lo generó un agente de IA. Pensada para correr en SEGUNDO PLANO (es un análisis
//! largo): el resultado se persiste y se avisa en la Bandeja aunque cierres la página.

use aion_kernel::traits::{GenerateRequest, LlmEngine};
use aion_kernel::types::Message;

/// ¿La petición pide una PROPUESTA/PREVENTIVO ANALÍTICO (consultor), no una oferta rápida?
pub fn is_proposta(task: &str) -> bool {
    let t = task.to_lowercase();
    let noun = t.contains("preventivo")
        || t.contains("proposta")
        || t.contains("propuesta")
        || t.contains("análisis")
        || t.contains("analisis")
        || t.contains("analitic")
        || t.contains("analítica")
        || t.contains("consultor");
    let make = t.contains("haz")
        || t.contains("hace")
        || t.contains("genera")
        || t.contains("crea")
        || t.contains("prepara")
        || t.contains("redacta")
        || t.contains("analiza")
        || t.contains("dame")
        || t.contains("quiero");
    noun && make
}

fn first_json_object(s: &str) -> Option<String> {
    let start = s.find('{')?;
    let mut depth = 0i32;
    for (i, c) in s[start..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(s[start..start + i + c.len_utf8()].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

fn jstr(v: &serde_json::Value, k: &str) -> String {
    v.get(k)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Construye el pie legal «EMPRESA | dirección | P.IVA | Tel | email» con los campos presentes.
fn legal_footer(az: &serde_json::Value) -> String {
    let mut parts = Vec::new();
    let name = jstr(az, "nome");
    if !name.is_empty() {
        parts.push(name);
    }
    let dir = jstr(az, "indirizzo");
    if !dir.is_empty() {
        parts.push(dir);
    }
    let piva = jstr(az, "piva");
    if !piva.is_empty() {
        parts.push(format!("P.IVA: {piva}"));
    }
    let tel = jstr(az, "tel");
    if !tel.is_empty() {
        parts.push(format!("Tel: {tel}"));
    }
    let email = jstr(az, "email");
    if !email.is_empty() {
        parts.push(email);
    }
    parts.join("  |  ")
}

fn esc_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// **Cierre profesional DETERMINISTA** de un preventivo/propuesta — lo que SIEMPRE debe llevar un
/// documento serio y que no podemos dejar al azar del LLM: próximos pasos destacados, validez,
/// **firma de AMBAS partes con fecha** e **informativa de privacidad (GDPR)**. Se inyecta como HTML
/// (pulldown-cmark lo deja pasar) al final del cuerpo. La marca/empresa es la que emite.
fn professional_closing(azienda_nome: &str, cliente_nome: &str) -> String {
    let az = if azienda_nome.trim().is_empty() {
        "la nostra azienda".to_string()
    } else {
        esc_html(azienda_nome.trim())
    };
    let cl_name = if cliente_nome.trim().is_empty() {
        "&nbsp;".to_string()
    } else {
        esc_html(cliente_nome.trim())
    };
    format!(
        r#"

<div class="doc-closing">
  <div class="callout-next">
    <span class="callout-title">Prossimi passi</span>
    <ol>
      <li>Conferma di questa proposta firmandola in calce.</li>
      <li>Avvio dei lavori entro 5 giorni lavorativi dalla firma.</li>
      <li>Aggiornamenti regolari fino alla consegna concordata.</li>
    </ol>
  </div>

  <p class="validity"><strong>Validità.</strong> La presente proposta è valida 30 giorni dalla data di emissione indicata in alto. I prezzi si intendono IVA esclusa salvo diversa indicazione.</p>

  <div class="sign-grid">
    <div class="sign-box">
      <span class="sign-role">Il Cliente — per accettazione</span>
      <span class="sign-name">{cl_name}</span>
      <span class="sign-line">Firma</span>
      <span class="sign-line">Data</span>
    </div>
    <div class="sign-box">
      <span class="sign-role">Per {az}</span>
      <span class="sign-name">&nbsp;</span>
      <span class="sign-line">Firma</span>
      <span class="sign-line">Data</span>
    </div>
  </div>

  <p class="privacy-note"><strong>Informativa sul trattamento dei dati personali (Reg. UE 2016/679 — GDPR).</strong> I dati personali forniti saranno trattati da {az}, in qualità di Titolare del trattamento, esclusivamente per la gestione del presente rapporto e per gli adempimenti di legge. I dati non saranno diffusi a terzi non autorizzati e potranno essere consultati, rettificati o cancellati in qualsiasi momento su richiesta dell'interessato (artt. 15-22 GDPR), scrivendo ai recapiti riportati in calce.</p>
</div>
"#
    )
}

/// Detecta los elementos que FALTAN para un preventivo de primera (intuición de consultor): si algo
/// crítico no está, lo devolvemos como avisos para PEDÍRSELO al usuario (no inventamos). Vacío = OK.
fn missing_required(azienda_present: bool, cliente_nome: &str, n_servizi: usize) -> Vec<String> {
    let mut m = Vec::new();
    if cliente_nome.trim().is_empty() {
        m.push("el **nombre del cliente** (a quién va dirigida la oferta)".to_string());
    }
    if !azienda_present {
        m.push(
            "los **datos de tu empresa** (razón social, P.IVA, dirección): usé la marca **AION** con pie de «agente de IA» por defecto".to_string(),
        );
    }
    if n_servizi == 0 {
        m.push(
            "los **servicios y precios** a incluir (no los invento; márcalos en una fuente o dímelos)".to_string(),
        );
    }
    m
}

/// Orquesta la propuesta: SEO real + extracción de empresa/cliente + redacción + marca + PDF.
/// Devuelve `(ruta, nombre_cliente, faltantes)` o un error legible: `faltantes` son los elementos
/// que convendría aportar para un preventivo de primera (el agente los PIDE, no los inventa).
pub async fn compose(
    engine: &dyn LlmEngine,
    project_id: Option<&str>,
    task: &str,
    context: &str,
) -> Result<(String, String, Vec<String>), String> {
    // ── 1. Material del proyecto: instrucciones (notas) + contenido de las fuentes ──
    let mut material = String::new();
    let mut web_url: Option<String> = None;
    if let Some(pid) = project_id {
        material.push_str(&crate::projects::source_notes_block(pid));
        for s in crate::projects::sources(pid)
            .into_iter()
            .filter(|s| s.active)
        {
            if s.kind == "web" && web_url.is_none() {
                let cand = if s.content.trim().starts_with("http") {
                    s.content.trim().to_string()
                } else {
                    s.title.trim().to_string()
                };
                web_url = Some(if cand.starts_with("http") {
                    cand
                } else {
                    format!("https://{cand}")
                });
            }
            if !s.content.trim().is_empty() {
                let body: String = s.content.chars().take(3500).collect();
                material.push_str(&format!(
                    "\n[FUENTE «{}» ({})]:\n{}\n",
                    s.title, s.kind, body
                ));
            }
        }
    }
    if let Some(u) = crate::serve::extract_url(task) {
        web_url = Some(u);
    }
    let material: String = material.chars().take(11000).collect();

    // ── 2. Análisis SEO REAL del sitio del cliente (si hay web) ──
    let mut seo_findings = String::new();
    let mut seo_score: Option<u32> = None;
    if let Some(url) = &web_url {
        if let Ok(rep) = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            crate::seo_audit::audit(url),
        )
        .await
        .unwrap_or(Err("timeout".into()))
        {
            seo_score = Some(rep.score);
            seo_findings = format!(
                "ANÁLISIS SEO REAL del sito del cliente ({}, punteggio {}/100):\n{}",
                rep.url, rep.score, rep.markdown
            );
        }
    }

    // ── 3. Extraer EMPRESA (marca), CLIENTE y SERVICIOS de las fuentes (JSON) ──
    let ext_prompt = format!(
        "Del MATERIAL extrae estos datos y devuélvelos como JSON VÁLIDO y nada más (sin ```):\n\
         {{\"azienda\":{{\"nome\":\"\",\"sottotitolo\":\"\",\"indirizzo\":\"\",\"piva\":\"\",\"tel\":\"\",\"email\":\"\"}},\
\"cliente\":{{\"nome\":\"\",\"settore\":\"\",\"citta\":\"\"}},\
\"servizi\":[{{\"titolo\":\"\",\"descrizione\":\"\",\"prezzo\":\"\",\"nota\":\"\"}}]}}\n\
         REGLAS: «azienda» = NUESTRA empresa que EMITE el documento (la marcada en las notas como \
         «nuestra empresa»/datos de empresa). «cliente» = a quién va dirigida (la marcada «nuestro \
         cliente»). Usa SOLO datos presentes; deja vacío lo que no esté. No inventes.\n\
         \nMATERIAL:\n«««\n{material}\n»»»"
    );
    let ext = tokio::time::timeout(
        std::time::Duration::from_secs(50),
        engine.generate(GenerateRequest {
            messages: vec![Message::user(ext_prompt)],
            think: false,
            temperature: Some(0.0),
            max_tokens: Some(700),
        }),
    )
    .await
    .map_err(|_| "la extracción tardó demasiado".to_string())?
    .map_err(|e| format!("fallo del modelo en la extracción: {e}"))?;
    let meta: serde_json::Value = first_json_object(&ext.content)
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or(serde_json::json!({}));
    let azienda = meta
        .get("azienda")
        .cloned()
        .unwrap_or(serde_json::json!({}));
    let cliente = meta
        .get("cliente")
        .cloned()
        .unwrap_or(serde_json::json!({}));
    let cliente_nome = jstr(&cliente, "nome");

    // ── 4. REDACTAR la propuesta a medida (markdown, estructura de consultor) ──
    let comp_prompt = format!(
        "Eres un consultor senior de marketing digital y SEO. Redacta una PROPUESTA COMMERCIALE \
         PROFESSIONALE in ITALIANO, a medida para el cliente, en MARKDOWN (usa ## para secciones, \
         **negrita**, listas con - y tablas markdown). NO uses un bloque de código. Estructura \
         OBLIGATORIA (como un preventivo de consultoría):\n\
         ## La situazione di oggi\n(analiza la presencia online REAL del cliente usando el análisis \
         SEO de abajo: qué falla y por qué pierde clientes; concreto, no genérico)\n\
         ## La nostra proposta\n(la solución a medida, a partir de NUESTROS servicios)\n\
         ## Investimento\n(una TABLA markdown con i servizi e i prezzi reali)\n\
         ## Perché conviene partire adesso\n(3-4 motivos persuasivos)\n\
         ## Condizioni essenziali\n(pagamento, cosa è incluso, proprietà al cliente, validità 30 \
         giorni, foro Milano)\n\n\
         REGLAS: usa SOLO datos reales del material y del análisis SEO; NO inventes precios ni \
         servicios; trata al cliente de «Lei»; tono profesional y cercano. Empieza directamente con \
         un breve párrafo de saludo al cliente y luego las secciones.\n\n\
         === NUESTRA EMPRESA ===\n{}\n\n=== CLIENTE ===\n{}\n\n=== {} ===\n{}\n\n=== MATERIAL (servizi/prezzi/note) ===\n«««\n{}\n»»»",
        serde_json::to_string(&azienda).unwrap_or_default(),
        serde_json::to_string(&cliente).unwrap_or_default(),
        if seo_findings.is_empty() { "SIN ANÁLISIS SEO" } else { "ANÁLISIS SEO REAL" },
        if seo_findings.is_empty() { "(no se pudo analizar el sitio)" } else { &seo_findings },
        material,
    );
    let comp = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        engine.generate(GenerateRequest {
            messages: vec![Message::user(comp_prompt)],
            think: false,
            temperature: Some(0.4),
            max_tokens: Some(2600),
        }),
    )
    .await
    .map_err(|_| "la redacción tardó demasiado".to_string())?
    .map_err(|e| format!("fallo del modelo al redactar: {e}"))?;
    let body_md = comp.content.trim().to_string();
    if body_md.chars().count() < 200 {
        return Err("la redacción salió demasiado corta; reinténtalo".into());
    }

    // ── 5. MARCA DINÁMICA: la empresa extraída; si no, AION con pie de «agente IA» ──
    let mut brand = aion_docgen::BrandProfile::load(crate::agent_tools::brand_profile_path());
    let az_nome = jstr(&azienda, "nome");
    if !az_nome.is_empty() {
        brand.company = az_nome;
        let sub = jstr(&azienda, "sottotitolo");
        if !sub.is_empty() {
            brand.tagline = sub;
        }
        let footer = legal_footer(&azienda);
        if !footer.is_empty() {
            brand.legal_footer = footer;
        }
        let email = jstr(&azienda, "email");
        if !email.is_empty() {
            brand.email = email;
        }
    } else if brand.company.trim().is_empty() || brand.company == "AION" {
        // Sin datos de empresa → AION, aclarando que lo generó un agente de IA.
        brand.company = "AION".into();
        if brand.tagline.trim().is_empty() {
            brand.tagline = "Inteligencia local con mente observable".into();
        }
        brand.legal_footer =
            "Documento generato da AION · agente di intelligenza artificiale".into();
    }
    brand.lang = "it".into();
    let st = crate::serve::resolve_default_style();

    // ── 6. GRÁFICO on-brand: medidor SVG del score SEO (vector, nítido) al inicio del cuerpo ──
    let mut body_full = String::new();
    if let Some(score) = seo_score {
        let gauge = aion_docgen::charts::score_gauge(score, "SEO oggi", &st);
        let verdict = if score >= 75 {
            "una buona base"
        } else if score >= 50 {
            "diverse criticità"
        } else {
            "gravi carenze"
        };
        body_full.push_str(&format!(
            "<div class=\"kpi-hero\">{gauge}<div class=\"kpi-note\"><strong>Punteggio SEO del sito attuale: {score}/100.</strong> L'analisi rivela {verdict} che frenano la visibilità su Google. Di seguito, cosa significa e come lo risolviamo.</div></div>\n\n"
        ));
    }
    body_full.push_str(&body_md);

    // ── 6b. CIERRE PROFESIONAL DETERMINISTA: próximos pasos + validez + FIRMAS de ambas partes
    //        con fecha + INFORMATIVA GDPR. No lo dejamos al LLM: un preventivo serio SIEMPRE lo lleva.
    body_full.push_str(&professional_closing(&brand.company, &cliente_nome));

    brand.ink = st.ink.clone();
    brand.accent = st.accent.clone();

    // ── 7. Render PDF con la marca ──
    let title = if cliente_nome.is_empty() {
        "Proposta".to_string()
    } else {
        format!(
            "Proposta — {}",
            cliente_nome.chars().take(50).collect::<String>()
        )
    };
    let mut req = aion_docgen::DocRequest::new("base", &title, &body_full);
    req.meta.date = crate::agent_tools::human_date("it");
    req.meta.number = Some(crate::agent_tools::next_preventivo_number());
    req.brand = brand;
    let bytes = aion_docgen::render_pdf(&req, &aion_docgen::PdfOptions::default())
        .await
        .map_err(|e| format!("no pude renderizar el PDF: {e}"))?;

    // ── 8. Guardar + abrir ──
    let home = std::env::var("HOME").map_err(|_| "no encuentro tu carpeta".to_string())?;
    let safe: String = cliente_nome
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let safe = safe.trim().trim_matches('_').trim();
    let fname = if safe.is_empty() {
        "Proposta.pdf".to_string()
    } else {
        format!("Proposta {}.pdf", safe.chars().take(50).collect::<String>())
    };
    let path = std::path::Path::new(&home).join("Desktop").join(fname);
    std::fs::write(&path, &bytes).map_err(|e| format!("no pude escribir el PDF: {e}"))?;
    crate::agent_tools::open_file(&path, false);

    // Intuición de consultor: ¿qué FALTARÍA para que el preventivo sea redondo? (se pide, no se inventa)
    let n_servizi = meta
        .get("servizi")
        .and_then(|s| s.as_array())
        .map(|a| a.iter().filter(|x| !jstr(x, "titolo").is_empty()).count())
        .unwrap_or(0);
    let missing = missing_required(!jstr(&azienda, "nome").is_empty(), &cliente_nome, n_servizi);

    Ok((path.display().to_string(), cliente_nome, missing))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detecta_intencion_propuesta() {
        assert!(is_proposta("hazme un preventivo analítico para el cliente"));
        assert!(is_proposta("analiza el sitio y redacta una propuesta"));
        assert!(!is_proposta("qué tal estás"));
        assert!(!is_proposta("hazme la oferta en pdf")); // eso es la oferta rápida
    }

    #[test]
    fn footer_solo_campos_presentes() {
        let az = serde_json::json!({"nome":"X SRL","piva":"123","email":"a@b.it"});
        assert_eq!(legal_footer(&az), "X SRL  |  P.IVA: 123  |  a@b.it");
    }

    #[test]
    fn cierre_lleva_firmas_de_ambos_y_gdpr() {
        let c = professional_closing("PRONTO CLICK SRLS", "Avv. Lisa Armenio");
        // Firma de AMBAS partes.
        assert!(c.contains("Il Cliente"));
        assert!(c.contains("Avv. Lisa Armenio"));
        assert!(c.contains("Per PRONTO CLICK SRLS"));
        // Hueco de firma + fecha en cada parte (2 firmas, 2 fechas).
        assert_eq!(c.matches("sign-line\">Firma").count(), 2);
        assert_eq!(c.matches("sign-line\">Data").count(), 2);
        // Cláusula de privacidad (GDPR) + validez.
        assert!(c.contains("GDPR") && c.contains("2016/679"));
        assert!(c.to_lowercase().contains("valida 30 giorni"));
    }

    #[test]
    fn intuye_lo_que_falta() {
        // Falta cliente + empresa + servicios → 3 avisos.
        assert_eq!(missing_required(false, "", 0).len(), 3);
        // Todo presente → sin avisos.
        assert!(missing_required(true, "Lisa", 2).is_empty());
        // Solo faltan precios/servicios.
        let m = missing_required(true, "Lisa", 0);
        assert_eq!(m.len(), 1);
        assert!(m[0].contains("servicios"));
    }
}
