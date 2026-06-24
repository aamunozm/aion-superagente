//! Extrae el ESTILO de un documento de referencia y lo aplica a una oferta.
//! Uso: `cargo run -p aion-docgen --example extract_demo -- <imagen.png> [referencia.pdf]`

use aion_docgen::{
    build_offerta, extract_style, render_offerta_pdf, Benefit, BrandProfile, Card, CompareBar,
    OfferRow, OffertaFacts, PdfOptions,
};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let png = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "/tmp/ref.png".into());
    let img = std::fs::read(&png).expect("no pude leer la imagen de referencia");
    let pdf = args.get(2).and_then(|p| std::fs::read(p).ok());

    let ex =
        extract_style(&img, pdf.as_deref(), "Estilo de la referencia").expect("extracción falló");
    println!("── ESTILO EXTRAÍDO ─────────────────────────────");
    println!("Paleta dominante : {}", ex.palette.join("  "));
    println!("Fuentes detectadas: {:?}", ex.fonts);
    println!(
        "DocStyle         : ink={} · accent={} · paper={}",
        ex.style.ink, ex.style.accent, ex.style.paper
    );
    println!("Display font     : {}", ex.style.font_display);

    // Aplica el estilo extraído a una oferta de ejemplo.
    let brand = BrandProfile {
        company: "PRONTO CLICK SRLS".into(),
        tagline: "Gestione Sito Web, SEO & Intelligenza Artificiale".into(),
        legal_footer: "PRONTO CLICK SRLS  |  Milano  |  info@prontoclick.it".into(),
        ..BrandProfile::default()
    };
    let facts = OffertaFacts {
        kicker: "OFFERTA SERVIZI 2026".into(),
        hero_kicker: "La tua presenza online che lavora per te".into(),
        hero_title: "Più clienti, in automatico.".into(),
        hero_pitch: "Sito **trovato su Google**, app che **risponde subito** e dashboard dei risultati. Tutto gestito, a canone fisso.".into(),
        highlights: vec![
            Card { title: "Sito & SEO".into(), body: "Gestione e posizionamento ogni mese.".into() },
            Card { title: "App con AI".into(), body: "Risponde ai contatti in modo **naturale**.".into() },
            Card { title: "Dashboard".into(), body: "Risultati trasparenti, nero su bianco.".into() },
        ],
        services: vec![
            OfferRow { title: "Primo mese — tutto incluso".into(), desc: "Avvio, sito, server e app AI.".into(), price: "€ 300,00".into(), ..Default::default() },
            OfferRow { title: "Dal secondo mese".into(), desc: "Gestione completa + assistenza.".into(), price: "€ 200,00".into(), price_note: "/ mese".into() },
        ],
        recurring_label: "Dal 2° mese (IVA esclusa)".into(),
        recurring_value: "€ 200,00 / mese".into(),
        benefits: vec![
            Benefit { lead: "Si ripaga da sola.".into(), body: "Basta un cliente in più al mese.".into() },
            Benefit { lead: "Zero vincoli.".into(), body: "Interrompi quando vuoi, senza penali.".into() },
        ],
        comparison: vec![
            CompareBar { label: "Persona dedicata".into(), pct: 95, value: "€ 1.800+/mese".into(), tone: "red".into() },
            CompareBar { label: "Agenzia tradizionale".into(), pct: 60, value: "€ 800–1.500/mese".into(), tone: "gold".into() },
            CompareBar { label: "La nostra offerta".into(), pct: 14, value: "€ 200/mese".into(), tone: "green".into() },
        ],
        deductible: true,
        ..Default::default()
    };
    let o = build_offerta(&facts);

    match render_offerta_pdf(&brand, &ex.style, &o, &PdfOptions::default()).await {
        Ok(b) => {
            let p = std::env::temp_dir().join("aion-offerta-EXTRACTED.pdf");
            std::fs::write(&p, &b).unwrap();
            println!(
                "\nPDF con el estilo EXTRAÍDO → {} ({} bytes)",
                p.display(),
                b.len()
            );
        }
        Err(e) => println!("ERROR render: {e}"),
    }
}
