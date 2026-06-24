//! Reproduce la «Offerta» de ProntoClick con la skill de documento `offerta`.
//! Ejecuta: `cargo run -p aion-docgen --example offerta_sample`

use aion_docgen::{
    render_offerta_pdf, style_presets, Benefit, BrandProfile, Card, CompareBar, Condition,
    OfferRow, OffertaContent, PdfOptions,
};

fn card(t: &str, b: &str) -> Card {
    Card {
        title: t.into(),
        body: b.into(),
    }
}
fn bar(label: &str, pct: u8, value: &str, tone: &str) -> CompareBar {
    CompareBar {
        label: label.into(),
        pct,
        value: value.into(),
        tone: tone.into(),
    }
}
fn ben(lead: &str, body: &str) -> Benefit {
    Benefit {
        lead: lead.into(),
        body: body.into(),
    }
}
fn cond(label: &str, body: &str) -> Condition {
    Condition {
        label: label.into(),
        body: body.into(),
    }
}

#[tokio::main]
async fn main() {
    let brand = BrandProfile {
        company: "PRONTO CLICK SRLS".into(),
        tagline: "Gestione Sito Web, SEO & Intelligenza Artificiale".into(),
        ink: "#2f4858".into(),
        accent: "#c69a24".into(),
        lang: "it".into(),
        legal_footer:
            "PRONTO CLICK SRLS  |  Via Casoretto, 9 - MILANO  |  P.IVA: 14436860960  |  Tel: +39 3339992219  |  info@prontoclick.it"
                .into(),
        ..BrandProfile::default()
    };

    let o = OffertaContent {
        doc_kicker: "OFFERTA SERVIZI 2026".into(),
        doc_subtitle: "Crescita digitale per la tua azienda".into(),
        attn_label: "Alla cortese attenzione di:".into(),
        attn_to: "Spett.le ______________________".into(),

        hero_kicker: "La tua presenza online che lavora per te".into(),
        hero_title: "Più clienti, in automatico.\nTu pensi al lavoro, al resto pensiamo noi.".into(),
        hero_body: "Oggi un sito web non basta: deve essere **trovato su Google**, deve **rispondere subito** a chi ti contatta e deve **trasformare le visite in clienti**. Noi gestiamo tutto questo per te, ogni mese, e ti mostriamo i risultati nero su bianco.".into(),

        intro: "La maggior parte delle aziende ha un sito che «c'è e basta»: non viene trovato, non risponde ai contatti in tempo, e nessuno sa se sta portando clienti o no. La nostra offerta nasce per cambiare esattamente questo, con un servizio completo e a **canone fisso**, senza sorprese.".into(),

        cards_title: "Cosa includiamo, ogni mese".into(),
        cards: vec![
            card("Sito & SEO gestiti", "Gestione completa del sito e posizionamento su Google nella tua zona e nel tuo settore. Lavoriamo perché ti trovi chi ti sta già cercando."),
            card("App clienti con AI", "Un'app che raccoglie e gestisce i tuoi contatti e risponde in modo **rapido, naturale e personale** grazie all'intelligenza artificiale, così nessun cliente resta senza risposta."),
            card("Dashboard risultati", "Ogni mese vedi **metriche e traguardi** in modo semplice: visite, contatti, posizione su Google e cosa abbiamo fatto. Tutto trasparente."),
        ],

        why_title: "Perché l'app con AI fa la differenza".into(),
        why_body: "La maggior parte dei clienti si perde nei primi minuti: chi non riceve una risposta veloce passa al concorrente. La nostra app con intelligenza artificiale gestisce i contatti in arrivo in modo **fluido e naturale**, raccoglie le richieste, organizza tutto in automatico e ti lascia solo il lavoro che conta davvero: parlare con chi è pronto a diventare cliente. Il risultato è semplice: **più contatti gestiti, più clienti, meno tempo perso.**".into(),

        offer_title: "L'offerta".into(),
        offer_rows: vec![
            OfferRow {
                title: "Primo mese — tutto incluso e attivato".into(),
                desc: "Avvio del progetto, configurazione del sito e del **server professionale con email dedicate e ampio spazio**, installazione dell'**app di gestione clienti con AI** e impostazione della dashboard. Tutto operativo da subito.".into(),
                price: "€ 300,00".into(),
                price_note: String::new(),
            },
            OfferRow {
                title: "Dal secondo mese — gestione completa".into(),
                desc: "Gestione sito + SEO, app clienti con AI sempre attiva, dashboard di metriche e risultati aggiornata **mese per mese**, assistenza dedicata.".into(),
                price: "€ 200,00".into(),
                price_note: "/ mese".into(),
            },
        ],
        offer_total_label: "Dal 2° mese (IVA esclusa)".into(),
        offer_total_value: "€ 200,00 / mese".into(),
        offer_note: "Importi IVA esclusa. **Nessun contratto vincolante: puoi interrompere quando vuoi, senza penali.**".into(),
        banner: "COSTO INTERAMENTE DEDUCIBILE   •   IVA INTERAMENTE DETRAIBILE".into(),

        compare_title: "Quanto vale davvero (e quanto costerebbe altrove)".into(),
        compare_intro: "Lo stesso lavoro, fatto in altri modi, ha un costo molto più alto. Ecco il confronto:".into(),
        compare_bars: vec![
            bar("Assumere una persona dedicata al marketing", 95, "da € 1.800+/mese", "red"),
            bar("Affidarsi a un'agenzia tradizionale", 62, "€ 800–1.500/mese", "gold"),
            bar("La nostra offerta completa", 14, "€ 200/mese", "green"),
        ],

        callout_pills: vec!["COSTO 100% DEDUCIBILE".into(), "IVA 100% DETRAIBILE".into()],
        callout_body: "Per la tua azienda è un costo di gestione **interamente deducibile** e con **IVA interamente detraibile**: il costo reale netto è quindi **molto più basso** di quello che vedi. Un investimento che lavora, non una spesa che resta ferma.".into(),

        benefits_title: "Perché è un'offerta che conviene accettare".into(),
        benefits: vec![
            ben("Si ripaga da sola.", "Basta un solo cliente in più al mese trovato grazie al servizio per coprire il canone. Tutto il resto è guadagno."),
            ben("Costo fisso, zero sorprese.", "Sai esattamente quanto spendi ogni mese e cosa ricevi in cambio, con la dashboard sempre sotto controllo."),
            ben("Nessuna perdita di contatti.", "L'AI risponde subito e in modo naturale: i clienti non scappano dalla concorrenza per una risposta arrivata tardi."),
            ben("Tu resti concentrato sul tuo lavoro.", "Sito, posizionamento, contatti e report: ce ne occupiamo noi. A te arrivano i risultati."),
            ben("Nessun vincolo, zero rischi.", "Nessun contratto da firmare per anni: puoi fermarti quando vuoi. Continui solo finché sei soddisfatto — è il nostro modo di dimostrarti che ci crediamo."),
        ],

        conditions_title: "Condizioni essenziali".into(),
        conditions: vec![
            cond("Pagamento", "primo mese € 300,00 (tutto incluso e attivato), poi € 200,00/mese a partire dal secondo mese. Bonifico bancario o altra modalità concordata."),
            cond("Fatturazione", "servizio interamente fatturato, fatturabile e deducibile per l'azienda."),
            cond("Proprietà", "sito, contenuti, dominio e dati dei clienti restano di proprietà del Cliente."),
            cond("Flessibilità", "**nessun contratto vincolante**, puoi interrompere quando vuoi senza penali; attività extra concordate sempre in anticipo."),
            cond("Privacy", "dati trattati ai sensi del Reg. UE 2016/679 (GDPR). **Foro competente:** Milano."),
            cond("Validità dell'offerta", "30 giorni dalla data di presentazione."),
        ],

        acceptance: "Per accettazione dell'offerta (primo mese **€ 300,00**, poi **€ 200,00/mese** dal secondo mese, IVA esclusa — senza vincolo di durata):".into(),
        closing: "Cordiali saluti,".into(),
    };

    // La MISMA oferta, renderizada en CADA estilo de la galería → muchos looks distintos.
    let dir = std::env::temp_dir();
    for st in style_presets() {
        let slug: String = st
            .name
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect();
        match render_offerta_pdf(&brand, &st, &o, &PdfOptions::default()).await {
            Ok(b) => {
                let p = dir.join(format!("aion-offerta-{slug}.pdf"));
                std::fs::write(&p, &b).unwrap();
                println!("[{}] → {} ({} bytes)", st.name, p.display(), b.len());
            }
            Err(e) => println!("[{}] ERROR: {e}", st.name),
        }
    }
}
