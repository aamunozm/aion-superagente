//! Capa de SKILLS (playbooks) estilo Claude Code / Anthropic Skills: procedimientos NOMBRADOS
//! con descripción + instrucciones que el agente DESCUBRE y SIGUE, componiendo sus tools. Cada
//! skill es un `SKILL.md` (frontmatter + cuerpo) en el directorio de datos; los defaults vienen
//! embebidos en el binario y se siembran en el primer arranque (sin pisar lo que edite Ariel).
//!
//! Progressive disclosure: en el prompt del agente solo va el catálogo (nombre + descripción,
//! barato); cuando una tarea encaja con una skill, el agente carga su cuerpo completo con la tool
//! `skill_load`. NO confundir con las skills WASM numéricas de `skill_store.rs` (otra cosa).

use std::path::PathBuf;

/// Skills semilla embebidas en el binario (se escriben a disco la 1ª vez). (archivo, contenido).
const SEEDS: &[(&str, &str)] = &[
    (
        "system-health-scan.md",
        include_str!("skills_seed/system-health-scan.md"),
    ),
    (
        "find-large-files.md",
        include_str!("skills_seed/find-large-files.md"),
    ),
    (
        "process-manager.md",
        include_str!("skills_seed/process-manager.md"),
    ),
    (
        "mac-security-audit.md",
        include_str!("skills_seed/mac-security-audit.md"),
    ),
    (
        "disk-cleanup.md",
        include_str!("skills_seed/disk-cleanup.md"),
    ),
    (
        "deep-research.md",
        include_str!("skills_seed/deep-research.md"),
    ),
    ("fact-check.md", include_str!("skills_seed/fact-check.md")),
    (
        "summarize-document.md",
        include_str!("skills_seed/summarize-document.md"),
    ),
    ("code-review.md", include_str!("skills_seed/code-review.md")),
    (
        "scaffold-project.md",
        include_str!("skills_seed/scaffold-project.md"),
    ),
    ("write-tests.md", include_str!("skills_seed/write-tests.md")),
    (
        "explain-codebase.md",
        include_str!("skills_seed/explain-codebase.md"),
    ),
];

#[derive(Debug, Clone)]
pub struct PlaybookSkill {
    pub name: String,
    pub description: String,
    pub when_to_use: String,
    pub category: String,
    pub tools: Vec<String>,
    pub body: String,
}

fn dir() -> PathBuf {
    crate::app_data_dir().join("skills_lib")
}

/// Siembra los defaults embebidos en disco si faltan. Idempotente: solo escribe los que no
/// existan por nombre de archivo, así que NO pisa skills que Ariel haya editado o añadido.
fn ensure_seeded() {
    let d = dir();
    let _ = std::fs::create_dir_all(&d);
    for (fname, content) in SEEDS {
        let p = d.join(fname);
        if !p.exists() {
            let _ = std::fs::write(&p, content);
        }
    }
}

/// Parsea un SKILL.md: frontmatter (entre `---`) + cuerpo. Tolerante: campos opcionales.
fn parse(raw: &str) -> Option<PlaybookSkill> {
    let raw = raw.trim_start();
    let rest = raw.strip_prefix("---")?;
    let end = rest.find("\n---")?;
    let front = &rest[..end];
    let body = rest[end + 4..]
        .trim_start_matches(['\n', '\r'])
        .trim()
        .to_string();
    let mut name = String::new();
    let mut description = String::new();
    let mut when_to_use = String::new();
    let mut category = String::new();
    let mut tools = Vec::new();
    for line in front.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let v = v.trim().trim_matches('"').to_string();
        match k.trim() {
            "name" => name = v,
            "description" => description = v,
            "when_to_use" => when_to_use = v,
            "category" => category = v,
            "tools" => {
                tools = v
                    .trim_start_matches('[')
                    .trim_end_matches(']')
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            _ => {}
        }
    }
    if name.is_empty() || body.is_empty() {
        return None;
    }
    Some(PlaybookSkill {
        name,
        description,
        when_to_use,
        category,
        tools,
        body,
    })
}

/// Todas las skills disponibles (siembra defaults si hace falta, luego lee la carpeta de datos).
pub fn all() -> Vec<PlaybookSkill> {
    ensure_seeded();
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir()) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            if let Ok(txt) = std::fs::read_to_string(&p) {
                if let Some(s) = parse(&txt) {
                    out.push(s);
                }
            }
        }
    }
    out.sort_by(|a, b| a.category.cmp(&b.category).then(a.name.cmp(&b.name)));
    out
}

/// Catálogo COMPACTO para el prompt del agente (progressive disclosure: solo nombre + para qué).
/// El agente carga el cuerpo de la que encaje con la tool `skill_load`.
pub fn catalog_brief() -> String {
    let skills = all();
    if skills.is_empty() {
        return String::new();
    }
    let mut s = String::from(
        "\nSKILLS (playbooks que ya sabes ejecutar). Si la tarea encaja con una, cárgala con la \
         herramienta `skill_load <nombre>` y SIGUE sus pasos en vez de improvisar:\n",
    );
    for sk in &skills {
        s.push_str(&format!("- {} — {}", sk.name, sk.description));
        if !sk.when_to_use.is_empty() {
            // Pistas de disparo: ayudan al agente a decidir CUÁNDO cargar la skill.
            s.push_str(&format!(" (cuándo: {})", sk.when_to_use));
        }
        s.push('\n');
    }
    s
}

/// Devuelve una skill por nombre (para `skill_load`). Tolerante a mayúsculas.
pub fn get(name: &str) -> Option<PlaybookSkill> {
    let n = name.trim();
    all().into_iter().find(|s| s.name.eq_ignore_ascii_case(n))
}
