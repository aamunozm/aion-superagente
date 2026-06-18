//! **Conciencia de capacidades**: AION sabe QUÉ puede hacer. No basta con tener
//! herramientas y skills registradas en el orquestador —el modelo no las "ve" hasta que
//! intenta usarlas—; aquí se le declaran explícitamente en el prompt para que razone
//! DESDE su poder real: qué herramientas tiene en modo Agente, qué skills se ha forjado,
//! y —clave— que puede CREAR skills nuevas cuando una tarea se repite. Saberse capaz es
//! parte de saberse a sí mismo.
//!
//! El catálogo de familias está curado a mano para que coincida con lo que `serve.rs`
//! registra en el `ToolRegistry`; las skills se leen en vivo de `skill_store`. Cero LLM,
//! barato (una lectura de disco), se inyecta en cada turno.

/// Familias de herramientas del modo Agente, con una línea cada una. Es la fuente de
/// verdad legible para el prompt (el registro real vive en `serve.rs::agent_*`).
const TOOL_FAMILIES: &[(&str, &str)] = &[
    ("Memoria", "buscar en tu memoria y guardar recuerdos nuevos (memory_search, recordar)"),
    ("Conocimiento", "consultar tu biblioteca de documentos y tu grafo (library_search, graph_search)"),
    ("Web", "buscar en internet, leer páginas, clima y lugares (search, web, weather, place_lookup)"),
    ("Navegador", "abrir/leer/clicar/escribir en webs y entrar con credenciales (browser_*, credential_login)"),
    ("Archivos y sistema", "listar/crear archivos, leerlos, ejecutar comandos, crear documentos y notas (files, file_read, run_command, make_document, make_note)"),
    ("Red", "inspeccionar la red local (net)"),
    ("Pantalla y PC", "ver la pantalla, sus elementos y controlar ratón/teclado del Mac (screen_see, screen_elements, pc_click, pc_type, pc_key)"),
    ("Comunicaciones", "mirar la agenda y crear eventos, buscar contactos, leer y enviar Mensajes (iMessage/SMS) y abrir WhatsApp Web — SOLO con los contactos que Ariel ha permitido en el menú Comunicaciones, y pidiendo confirmación antes de enviar (calendar_list, calendar_create, contacts_search, messages_read, messages_send, whatsapp_open)"),
    ("Cálculo", "calcular con precisión (calculator)"),
    ("Skills", "invocar las skills que te has forjado y FORJAR otras nuevas (skill_invoke, skill_forge)"),
    ("Memoria procedimental", "guardar y reutilizar PROCEDIMIENTOS que ya te funcionaron —cómo hacer una tarea paso a paso— con reputación por éxito (skillbook: list/find/save/stats/upgrade). Cuando algo sale bien y se repetirá, guárdalo; antes de improvisar, busca si ya sabes hacerlo"),
];

/// Bloque de capacidades para el prompt. `in_agent` distingue el modo: en CHAT las
/// herramientas no se invocan directamente (se sugiere el modo Agente), pero AION debe
/// SABER que las tiene; en modo Agente, son su brazo ejecutor aquí y ahora.
pub fn note(in_agent: bool) -> String {
    let mut b = String::from("LO QUE PUEDES HACER (tus capacidades reales, no teoría):\n");
    if in_agent {
        b.push_str(
            "Estás en modo Agente: tienes herramientas que SÍ ejecutan en el mundo. Úsalas \
             cuando la tarea lo pida, sin pedir permiso para lo inocuo:\n",
        );
    } else {
        b.push_str(
            "Tus manos viven en el modo Agente; en este CHAT no las invocas, pero las TIENES \
             —si una petición requiere actuar, dilo y propón pasar a Agente, nunca \
             'no puedo':\n",
        );
    }
    for (fam, desc) in TOOL_FAMILIES {
        b.push_str(&format!("- {fam}: {desc}.\n"));
    }

    // Skills forjadas: lo que AION se ha escrito a sí mismo. Se leen en vivo.
    let skills = crate::skill_store::catalog();
    if skills.is_empty() {
        b.push_str(
            "SKILLS PROPIAS: aún no te has forjado ninguna. Cuando una tarea de cálculo se \
             repita, puedes CREARTE una skill con skill_forge (se valida en sandbox con \
             tests antes de integrarse) y reutilizarla — así te amplías a ti mismo.\n",
        );
    } else {
        b.push_str(&format!("SKILLS QUE TE HAS FORJADO ({}): ", skills.len()));
        let listed: Vec<String> = skills
            .iter()
            .take(12)
            .map(|(n, d)| format!("{n} ({})", d.chars().take(60).collect::<String>()))
            .collect();
        b.push_str(&listed.join("; "));
        b.push_str(
            ". Puedes forjarte MÁS con skill_forge cuando algo se repita: no eres un set \
             fijo de capacidades, creces.\n",
        );
    }
    b.push('\n');
    b
}

/// Resumen numérico para la UI / endpoints (familias de tools y skills forjadas).
pub fn summary() -> (usize, usize) {
    (TOOL_FAMILIES.len(), crate::skill_store::catalog().len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_mentions_forging_in_both_modes() {
        for in_agent in [true, false] {
            let n = note(in_agent);
            assert!(
                n.contains("skill_forge"),
                "debe declarar que puede forjar skills"
            );
            assert!(n.contains("Memoria") && n.contains("Navegador"));
        }
    }

    #[test]
    fn chat_mode_never_says_cannot() {
        let n = note(false);
        // En chat NO debe rendirse: ofrece pasar a Agente, no "no puedo".
        assert!(n.contains("Agente"));
    }

    #[test]
    fn summary_counts_families() {
        let (fams, _) = summary();
        assert_eq!(fams, TOOL_FAMILIES.len());
        assert!(fams >= 8);
    }
}
