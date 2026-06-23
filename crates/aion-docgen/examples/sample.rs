//! Genera un preventivo de muestra en HTML, PDF y DOCX.
//! Ejecuta: `cargo run -p aion-docgen --example sample`

use aion_docgen::{
    render_docx, render_html, render_pdf, BrandProfile, ClientInfo, DocMeta, DocRequest, PdfOptions,
};

#[tokio::main]
async fn main() {
    let md = r#"## Oggetto della proposta

Realizzazione di un **sito web** professionale e attività di **ottimizzazione SEO**.

### Dettaglio servizi

| Servizio | Dettaglio | Prezzo |
|---|---|---:|
| Sito web | Design + sviluppo (5 pagine) | € 2.500 |
| SEO on-page | Audit + ottimizzazione | € 800 |
| Manutenzione | Canone annuale | € 480 |

> Obiettivo: comparire *tra i primi risultati* di Google per le query rilevanti.

### Tempistiche
- Consegna bozza: **10 giorni** lavorativi
- Go-live: **3 settimane** dall'approvazione
"#;

    let mut req = DocRequest::new("preventivo", "Sito web + SEO", md);
    req.brand = BrandProfile {
        company: "ProntoClick".into(),
        tagline: "Web · SEO · Automazione AI".into(),
        lang: "it".into(),
        website: "prontoclick.it".into(),
        email: "info@prontoclick.it".into(),
        legal_footer: "ProntoClick di Ariel Marquez · P.IVA — · Documento riservato".into(),
        ..BrandProfile::default()
    };
    req.meta = DocMeta {
        subtitle: Some("Proposta commerciale".into()),
        date: "23 giugno 2026".into(),
        number: Some("PREV-2026-031".into()),
        client: Some(ClientInfo {
            name: "Mario Rossi".into(),
            company: "Acme S.r.l.".into(),
            email: "mario.rossi@acme.it".into(),
            address: "Via Roma 1, 20100 Milano".into(),
        }),
    };

    let dir = std::env::temp_dir();

    let html = render_html(&req).expect("html");
    let p_html = dir.join("aion-docgen-sample.html");
    std::fs::write(&p_html, &html).unwrap();
    println!("HTML  → {} ({} bytes)", p_html.display(), html.len());

    match render_docx(&req) {
        Ok(b) => {
            let p = dir.join("aion-docgen-sample.docx");
            std::fs::write(&p, &b).unwrap();
            println!("DOCX  → {} ({} bytes)", p.display(), b.len());
        }
        Err(e) => println!("DOCX  → ERROR: {e}"),
    }

    match render_pdf(&req, &PdfOptions::default()).await {
        Ok(b) => {
            let p = dir.join("aion-docgen-sample.pdf");
            std::fs::write(&p, &b).unwrap();
            let ok = b.starts_with(b"%PDF");
            println!(
                "PDF   → {} ({} bytes){}",
                p.display(),
                b.len(),
                if ok {
                    " ✅ %PDF"
                } else {
                    " ⚠ cabecera inesperada"
                }
            );
        }
        Err(e) => println!("PDF   → ERROR: {e}"),
    }
}
