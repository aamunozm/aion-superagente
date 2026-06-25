//! **Skill de documento: «offerta» (oferta comercial rica).**
//!
//! Es el primer *documento enriquecido* de AION: en vez de markdown plano, recibe un
//! contenido TIPADO por bloques (hero, tarjetas, tabla de oferta, gráfico comparativo,
//! beneficios, condiciones, firma) y la plantilla `offerta.html` lo compone en un PDF
//! premium con la marca. La "skill" = este modelo + la plantilla + la librería de
//! componentes CSS; *enriquece* una petición fina en un documento de nivel agencia.
//!
//! Los campos de prosa aceptan **markdown inline** (negrita/cursiva): la plantilla los
//! pasa por el filtro `md` (ver [`crate::template`]).

use crate::BrandProfile;
use serde::{Deserialize, Serialize};

/// Tarjeta numerada de "qué incluimos" (los recuadros 1·2·3 del ejemplo).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Card {
    pub title: String,
    pub body: String,
}

/// Fila de la tabla de oferta: concepto + descripción + importe (+ nota tipo "/ mese").
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct OfferRow {
    pub title: String,
    #[serde(default)]
    pub desc: String,
    pub price: String,
    #[serde(default)]
    pub price_note: String,
}

/// Barra del gráfico comparativo "cuánto costaría en otro sitio". `pct` (0–100) es el ancho;
/// `tone` ∈ {"red","gold","green"} da el color (caro→barato).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CompareBar {
    pub label: String,
    pub pct: u8,
    pub value: String,
    #[serde(default)]
    pub tone: String,
}

/// Viñeta de beneficios con lead en negrita ("**Si ripaga da sola.** Basta…").
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Benefit {
    pub lead: String,
    pub body: String,
}

/// Condición esencial (etiqueta + texto): Pagamento, Privacy, Validità…
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Condition {
    pub label: String,
    pub body: String,
}

/// Contenido COMPLETO de una oferta comercial rica. Todos los campos son opcionales (los
/// bloques vacíos no se renderizan), así una oferta puede ser más corta o más larga.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OffertaContent {
    // Cabecera (la marca —company/tagline/contacto— viene del BrandProfile).
    #[serde(default)]
    pub doc_kicker: String, // "OFFERTA SERVIZI 2026"
    #[serde(default)]
    pub doc_number: String, // referencia secuencial "PREV-2026-001" (la pone el caller)
    #[serde(default)]
    pub doc_date: String, // fecha legible "24 giugno 2026" (la pone el caller)
    #[serde(default)]
    pub doc_subtitle: String, // "Crescita digitale per la tua azienda"
    #[serde(default)]
    pub attn_label: String, // "Alla cortese attenzione di:"
    #[serde(default)]
    pub attn_to: String, // "Spett.le ____" o el nombre del cliente

    // Hero (recuadro oscuro de portada).
    #[serde(default)]
    pub hero_kicker: String,
    #[serde(default)]
    pub hero_title: String, // admite saltos de línea con \n
    #[serde(default)]
    pub hero_body: String,

    #[serde(default)]
    pub intro: String,

    #[serde(default)]
    pub cards_title: String,
    #[serde(default)]
    pub cards: Vec<Card>,

    #[serde(default)]
    pub why_title: String,
    #[serde(default)]
    pub why_body: String,

    // Tabla de oferta.
    #[serde(default)]
    pub offer_title: String,
    #[serde(default)]
    pub offer_rows: Vec<OfferRow>,
    #[serde(default)]
    pub offer_total_label: String,
    #[serde(default)]
    pub offer_total_value: String,
    /// Anclaje de precio (price anchoring): valor "de listino"/más caro, tachado junto al total.
    #[serde(default)]
    pub offer_total_anchor: String,
    #[serde(default)]
    pub offer_note: String,
    #[serde(default)]
    pub banner: String, // franja "COSTO INTERAMENTE DEDUCIBILE…"

    // Comparativa visual.
    #[serde(default)]
    pub compare_title: String,
    #[serde(default)]
    pub compare_intro: String,
    #[serde(default)]
    pub compare_bars: Vec<CompareBar>,

    // Callout con pills.
    #[serde(default)]
    pub callout_pills: Vec<String>,
    #[serde(default)]
    pub callout_body: String,

    // Beneficios (caja oscura) + condiciones.
    #[serde(default)]
    pub benefits_title: String,
    #[serde(default)]
    pub benefits: Vec<Benefit>,
    #[serde(default)]
    pub conditions_title: String,
    #[serde(default)]
    pub conditions: Vec<Condition>,

    // Cierre + firma.
    #[serde(default)]
    pub acceptance: String,
    #[serde(default)]
    pub closing: String, // "Cordiali saluti,"
}

/// **Hechos COMPACTOS** de una oferta: solo lo variable (servicios, precios, propuestas de
/// valor, comparativa). El resto —títulos de sección, franja deducible, condiciones legales,
/// firma, cierre— lo rellena la skill con su scaffolding estándar. Es la mitad DETERMINISTA
/// del modo híbrido: tú das los hechos, [`build_offerta`] monta el documento; el LLM local
/// (en aion-core) solo pule la prosa después, si quieres.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct OffertaFacts {
    pub kicker: String,
    pub subtitle: String,
    pub client: String,
    pub hero_kicker: String,
    pub hero_title: String,
    pub hero_pitch: String,
    pub intro: String,
    pub highlights: Vec<Card>,
    pub why: String,
    pub services: Vec<OfferRow>,
    pub recurring_label: String,
    pub recurring_value: String,
    pub benefits: Vec<Benefit>,
    pub comparison: Vec<CompareBar>,
    pub deductible: bool,
    pub validity_days: u32,
}

fn or(a: &str, b: &str) -> String {
    if a.trim().is_empty() {
        b.to_string()
    } else {
        a.to_string()
    }
}

/// Monta una [`OffertaContent`] COMPLETA a partir de hechos compactos, rellenando el
/// scaffolding estándar de una oferta comercial italiana (títulos, franja deducible,
/// condiciones, firma…). Lo que aportes en los hechos manda; lo que dejes vacío lo pone la
/// skill. El idioma del scaffolding es italiano (el caso ProntoClick); cualquier campo se
/// puede sobrescribir después sobre la `OffertaContent` devuelta.
pub fn build_offerta(f: &OffertaFacts) -> OffertaContent {
    let days = if f.validity_days == 0 {
        30
    } else {
        f.validity_days
    };
    let mut conditions = vec![
        Condition {
            label: "Fatturazione".into(),
            body: "servizio interamente fatturato, fatturabile e deducibile per l'azienda.".into(),
        },
        Condition {
            label: "Proprietà".into(),
            body: "sito, contenuti, dominio e dati dei clienti restano di proprietà del Cliente."
                .into(),
        },
        Condition {
            label: "Flessibilità".into(),
            body: "**nessun contratto vincolante**, puoi interrompere quando vuoi senza penali; attività extra concordate sempre in anticipo.".into(),
        },
        Condition {
            label: "Privacy".into(),
            body: "dati trattati ai sensi del Reg. UE 2016/679 (GDPR). **Foro competente:** Milano.".into(),
        },
        Condition {
            label: "Validità dell'offerta".into(),
            body: format!("{days} giorni dalla data di presentazione."),
        },
    ];
    if let Some(first) = f.services.first() {
        conditions.insert(
            0,
            Condition {
                label: "Pagamento".into(),
                body: format!(
                    "{} ({}). Bonifico bancario o altra modalità concordata.",
                    first.price,
                    first.title.to_lowercase()
                ),
            },
        );
    }
    let (callout_pills, callout_body) = if f.deductible {
        (
            vec!["COSTO 100% DEDUCIBILE".into(), "IVA 100% DETRAIBILE".into()],
            "Per la tua azienda è un costo di gestione **interamente deducibile** e con **IVA interamente detraibile**: il costo reale netto è quindi **molto più basso** di quello che vedi. Un investimento che lavora, non una spesa che resta ferma.".into(),
        )
    } else {
        (vec![], String::new())
    };
    let has_cmp = !f.comparison.is_empty();
    // PRICE ANCHORING: si hay comparativa y un canone, el valor MÁS CARO (la 1ª barra, por
    // convención expensive→cheap) se muestra TACHADO junto a tu total → tu precio se percibe
    // como una ganga frente al ancla. Anclaje derivado de datos que ya diste (cero input extra).
    let offer_total_anchor = if has_cmp && !f.recurring_value.trim().is_empty() {
        f.comparison
            .first()
            .map(|b| b.value.clone())
            .unwrap_or_default()
    } else {
        String::new()
    };
    // SCAFFOLDING PERSUASIVO: si no diste intro/beneficios, la skill los rellena con copy de
    // venta probado (apertura + risk reversal + ROI + propiedad) — así hasta una oferta mínima
    // («cliente + 2 servizi») sale como un documento completo y convincente, no esquelético.
    let intro = or(
        &f.intro,
        "Di seguito la nostra proposta per far lavorare davvero la tua presenza online: \
         **più visibilità, più contatti e risultati misurabili**, gestiti da noi con un canone \
         chiaro e senza sorprese.",
    );
    let benefits = if f.benefits.is_empty() {
        vec![
            Benefit {
                lead: "Si ripaga da sola.".into(),
                body: "basta un cliente in più al mese perché l'investimento sia già rientrato."
                    .into(),
            },
            Benefit {
                lead: "Zero vincoli.".into(),
                body: "nessun contratto: continui solo se sei soddisfatto, interrompi quando vuoi senza penali.".into(),
            },
            Benefit {
                lead: "Tutto tuo, tutto chiaro.".into(),
                body: "sito, dominio e dati restano di tua proprietà; report trasparenti, nero su bianco.".into(),
            },
        ]
    } else {
        f.benefits.clone()
    };
    OffertaContent {
        doc_kicker: or(&f.kicker, "OFFERTA SERVIZI"),
        doc_number: String::new(),
        doc_date: String::new(),
        doc_subtitle: or(&f.subtitle, "Crescita digitale per la tua azienda"),
        attn_label: "Alla cortese attenzione di:".into(),
        attn_to: or(&f.client, "Spett.le ______________________"),
        hero_kicker: f.hero_kicker.clone(),
        hero_title: f.hero_title.clone(),
        hero_body: f.hero_pitch.clone(),
        intro,
        cards_title: if f.highlights.is_empty() {
            String::new()
        } else {
            "Cosa includiamo, ogni mese".into()
        },
        cards: f.highlights.clone(),
        why_title: if f.why.trim().is_empty() {
            String::new()
        } else {
            "Perché fa la differenza".into()
        },
        why_body: f.why.clone(),
        offer_title: "L'offerta".into(),
        offer_rows: f.services.clone(),
        offer_total_label: f.recurring_label.clone(),
        offer_total_value: f.recurring_value.clone(),
        offer_total_anchor,
        offer_note: "Importi IVA esclusa. **Nessun contratto vincolante: puoi interrompere quando vuoi, senza penali.**".into(),
        banner: if f.deductible {
            "COSTO INTERAMENTE DEDUCIBILE   •   IVA INTERAMENTE DETRAIBILE".into()
        } else {
            String::new()
        },
        compare_title: if has_cmp {
            "Quanto vale davvero (e quanto costerebbe altrove)".into()
        } else {
            String::new()
        },
        compare_intro: if has_cmp {
            "Lo stesso lavoro, fatto in altri modi, ha un costo molto più alto. Ecco il confronto:".into()
        } else {
            String::new()
        },
        compare_bars: f.comparison.clone(),
        callout_pills,
        callout_body,
        benefits_title: "Perché è un'offerta che conviene accettare".into(),
        benefits,
        conditions_title: "Condizioni essenziali".into(),
        conditions,
        acceptance: "Per accettazione dell'offerta, datare e firmare nello spazio sottostante:"
            .into(),
        closing: "Cordiali saluti,".into(),
    }
}

/// Contexto que ve la plantilla `offerta.html`: marca (identidad) + estilo (look) + contenido.
#[derive(Serialize)]
struct OffertaCtx<'a> {
    brand: &'a BrandProfile,
    style: &'a crate::DocStyle,
    lang: &'a str,
    o: &'a OffertaContent,
}

/// Renderiza la oferta a **HTML** con el ESTILO dado (sin lanzar el navegador). El mismo
/// `content` con estilos distintos da looks distintos (galería tipo Canva).
pub fn render_offerta_html(
    brand: &BrandProfile,
    style: &crate::DocStyle,
    content: &OffertaContent,
) -> Result<String, String> {
    let ctx = OffertaCtx {
        brand,
        style,
        lang: &brand.lang,
        o: content,
    };
    crate::template::render("offerta", &ctx)
}

/// Renderiza la oferta a **PDF** premium con el estilo dado, vía el Chromium headless local.
pub async fn render_offerta_pdf(
    brand: &BrandProfile,
    style: &crate::DocStyle,
    content: &OffertaContent,
    opts: &crate::PdfOptions,
) -> Result<Vec<u8>, String> {
    let html = render_offerta_html(brand, style, content)?;
    aion_browser::html_to_pdf(&html, opts)
        .await
        .map_err(|e| e.to_string())
}

/// Aplana una oferta a Markdown (versión EDITABLE; pierde el layout visual rico pero conserva
/// todo el contenido). Base para el DOCX.
fn offerta_to_markdown(o: &OffertaContent) -> String {
    let mut m = String::new();
    let title = if o.hero_title.trim().is_empty() {
        o.doc_kicker.as_str()
    } else {
        o.hero_title.as_str()
    };
    m.push_str(&format!("# {}\n\n", title.replace('\n', " ")));
    if !o.doc_subtitle.is_empty() {
        m.push_str(&format!("*{}*\n\n", o.doc_subtitle));
    }
    if !o.doc_number.is_empty() || !o.doc_date.is_empty() {
        let sep = if !o.doc_number.is_empty() && !o.doc_date.is_empty() {
            " · "
        } else {
            ""
        };
        let num = if o.doc_number.is_empty() {
            String::new()
        } else {
            format!("Rif. {}", o.doc_number)
        };
        m.push_str(&format!("{num}{sep}{}\n\n", o.doc_date));
    }
    if !o.attn_to.is_empty() {
        m.push_str(&format!("{} {}\n\n", o.attn_label, o.attn_to));
    }
    if !o.hero_body.is_empty() {
        m.push_str(&format!("{}\n\n", o.hero_body));
    }
    if !o.intro.is_empty() {
        m.push_str(&format!("{}\n\n", o.intro));
    }
    if !o.cards.is_empty() {
        if !o.cards_title.is_empty() {
            m.push_str(&format!("## {}\n\n", o.cards_title));
        }
        for c in &o.cards {
            m.push_str(&format!("- **{}**: {}\n", c.title, c.body));
        }
        m.push('\n');
    }
    if !o.offer_rows.is_empty() {
        if !o.offer_title.is_empty() {
            m.push_str(&format!("## {}\n\n", o.offer_title));
        }
        m.push_str("| Servizio | Importo |\n|---|---|\n");
        for r in &o.offer_rows {
            m.push_str(&format!("| {} | {} {} |\n", r.title, r.price, r.price_note));
        }
        m.push('\n');
        if !o.offer_total_value.is_empty() {
            m.push_str(&format!(
                "**{}: {}**\n\n",
                o.offer_total_label, o.offer_total_value
            ));
        }
    }
    if !o.benefits.is_empty() {
        if !o.benefits_title.is_empty() {
            m.push_str(&format!("## {}\n\n", o.benefits_title));
        }
        for b in &o.benefits {
            m.push_str(&format!("- **{}** {}\n", b.lead, b.body));
        }
        m.push('\n');
    }
    if !o.conditions.is_empty() {
        if !o.conditions_title.is_empty() {
            m.push_str(&format!("## {}\n\n", o.conditions_title));
        }
        for c in &o.conditions {
            m.push_str(&format!("- **{}:** {}\n", c.label, c.body));
        }
        m.push('\n');
    }
    m
}

/// Renderiza la oferta a **DOCX** editable (Word/Pages). Versión estructurada y simplificada
/// del contenido (no el layout visual rico del PDF, pero editable).
pub fn render_offerta_docx(
    brand: &BrandProfile,
    content: &OffertaContent,
) -> Result<Vec<u8>, String> {
    let md = offerta_to_markdown(content);
    let title = if content.hero_title.trim().is_empty() {
        content.doc_kicker.clone()
    } else {
        content.hero_title.replace('\n', " ")
    };
    crate::docx::render(&title, &brand.company, &brand.accent, &md)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offerta_renders_rich_blocks() {
        let brand = BrandProfile {
            company: "ProntoClick".into(),
            ..BrandProfile::default()
        };
        let content = OffertaContent {
            doc_kicker: "OFFERTA SERVIZI 2026".into(),
            hero_title: "Più clienti, in automatico.".into(),
            hero_body: "Lo gestiamo **tutto** per te.".into(),
            cards_title: "Cosa includiamo".into(),
            cards: vec![Card {
                title: "Sito & SEO".into(),
                body: "Gestione completa.".into(),
            }],
            offer_rows: vec![OfferRow {
                title: "Primo mese".into(),
                desc: "Tutto incluso".into(),
                price: "€ 300,00".into(),
                ..Default::default()
            }],
            compare_bars: vec![CompareBar {
                label: "La nostra offerta".into(),
                pct: 15,
                value: "€ 200/mese".into(),
                tone: "green".into(),
            }],
            benefits: vec![Benefit {
                lead: "Si ripaga da sola.".into(),
                body: "Basta un cliente.".into(),
            }],
            ..Default::default()
        };
        let html = render_offerta_html(&brand, &crate::DocStyle::default(), &content)
            .expect("render offerta");
        assert!(html.contains("ProntoClick"), "marca");
        assert!(html.contains("OFFERTA SERVIZI 2026"), "kicker");
        assert!(html.contains("Più clienti"), "hero");
        assert!(
            html.contains("<strong>tutto</strong>"),
            "markdown inline en hero"
        );
        assert!(
            html.contains("Sito &amp; SEO") || html.contains("Sito & SEO"),
            "tarjeta"
        );
        assert!(html.contains("€ 300,00"), "fila de oferta");
        assert!(html.contains("Si ripaga"), "beneficio");
        assert!(
            html.to_lowercase().contains("<!doctype html>"),
            "doc completo"
        );
    }

    #[test]
    fn build_offerta_fills_scaffolding() {
        // Hechos MÍNIMOS → la skill monta el documento estándar completo.
        let f = OffertaFacts {
            client: "Spett.le Acme S.r.l.".into(),
            hero_title: "Più clienti.".into(),
            services: vec![OfferRow {
                title: "Primo mese".into(),
                price: "€ 300,00".into(),
                ..Default::default()
            }],
            deductible: true,
            ..Default::default()
        };
        let c = build_offerta(&f);
        assert_eq!(c.doc_kicker, "OFFERTA SERVIZI", "kicker por defecto");
        assert_eq!(c.offer_title, "L'offerta");
        assert!(c.banner.contains("DEDUCIBILE"), "franja deducible");
        assert_eq!(c.callout_pills.len(), 2, "pills del callout");
        assert!(
            c.conditions.iter().any(|x| x.label == "Pagamento"),
            "condición Pagamento desde el 1er servicio"
        );
        assert!(c.conditions.len() >= 6, "condiciones estándar completas");
        // Y renderiza sin romper.
        let html = render_offerta_html(&BrandProfile::default(), &crate::DocStyle::default(), &c)
            .expect("render");
        assert!(
            html.contains("Validità"),
            "condición de validez renderizada"
        );
        assert!(html.contains("Spett.le Acme"), "cliente renderizado");
    }
}
