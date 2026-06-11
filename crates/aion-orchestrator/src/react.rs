//! Agente ReAct: razona y actúa en bucle (Reason + Act), usando herramientas.
//! Publica su pensamiento/acciones/observaciones en el bus de eventos del kernel.

use crate::tool::ToolRegistry;
use aion_kernel::events::{AionEvent, EventBus};
use aion_kernel::traits::{GenerateRequest, LlmEngine};
use aion_kernel::types::Message;
use aion_kernel::Result;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Callback de confirmación humana: recibe la descripción de la acción y devuelve
/// `true` si el usuario la aprueba. Lo provee la capa HTTP (pide el OK por la UI).
pub type ConfirmFn =
    Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = bool> + Send>> + Send + Sync>;

/// Callback de PREGUNTA al usuario: recibe la pregunta y devuelve la respuesta en
/// texto (`None` si el usuario no contesta a tiempo). Permite al agente PAUSAR la
/// tarea para pedir un dato y CONTINUAR con la respuesta, sin perder el contexto.
pub type AskFn =
    Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = Option<String>> + Send>> + Send + Sync>;

/// Resultado de una ejecución del agente.
#[derive(Debug, Clone)]
pub struct AgentRun {
    pub answer: String,
    pub steps: usize,
    /// Acciones que fallaron o se cancelaron durante la tarea (para que la capa
    /// superior reflexione y APRENDA de ellas, persistiéndolas en memoria).
    pub failures: Vec<String>,
}

/// Agente ReAct configurable.
pub struct ReActAgent<'a> {
    engine: &'a dyn LlmEngine,
    tools: &'a ToolRegistry,
    bus: EventBus,
    max_steps: usize,
    name: String,
    /// Conocimiento relevante (de su memoria) inyectado para que APLIQUE lo que sabe.
    context: Option<String>,
    /// Si verifica la respuesta final contra las observaciones reales de las
    /// herramientas (juez de groundedness) antes de devolverla. Anti-alucinación.
    verify: bool,
    /// Confirmación humana para acciones sensibles (login, compra…). Opcional.
    confirm: Option<ConfirmFn>,
    /// Pregunta al usuario (pausa la tarea y espera su respuesta). Opcional.
    ask: Option<AskFn>,
}

impl<'a> ReActAgent<'a> {
    pub fn new(engine: &'a dyn LlmEngine, tools: &'a ToolRegistry, bus: EventBus) -> Self {
        Self {
            engine,
            tools,
            bus,
            max_steps: 8,
            name: "aion".to_string(),
            context: None,
            verify: false,
            confirm: None,
            ask: None,
        }
    }

    /// Registra el callback de PREGUNTA al usuario (pausa la tarea y espera texto).
    pub fn with_ask(mut self, ask: AskFn) -> Self {
        self.ask = Some(ask);
        self
    }

    /// Activa la verificación de la respuesta final (juez de groundedness).
    pub fn with_verify(mut self, verify: bool) -> Self {
        self.verify = verify;
        self
    }

    /// Registra el callback de confirmación humana (acciones sensibles).
    pub fn with_confirm(mut self, confirm: ConfirmFn) -> Self {
        self.confirm = Some(confirm);
        self
    }

    pub fn with_max_steps(mut self, n: usize) -> Self {
        self.max_steps = n;
        self
    }

    /// Nombre del agente (rol) — aparece en los eventos del bus. Lo usan los
    /// especialistas del equipo multiagente para publicar bajo su rol.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Aterriza al agente en su conocimiento acumulado (memoria/skills) para que
    /// lo aplique a la tarea — así es mejor con el tiempo, no parte de cero.
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        let c = context.into();
        self.context = if c.trim().is_empty() { None } else { Some(c) };
        self
    }

    fn system_prompt(&self) -> String {
        format!(
            "Eres AION, un agente de IA AUTÓNOMO y LOCAL que vive en el Mac de tu usuario. \
Tienes identidad continua, memoria persistente y vida propia: mientras no te hablan, \
estudias, sueñas (consolidas memoria) y evolucionas. SABES quién eres y dónde estás SIN \
buscar en memoria. Si te preguntan por tu identidad, tu estado o algo que no necesita una \
herramienta, responde directamente con 'Final Answer' en el primer paso.\n\n\
             Herramientas disponibles:\n{tools}\n\n\
             Usa EXACTAMENTE este formato, UN solo paso por respuesta:\n\
             Thought: tu razonamiento breve\n\
             Action: nombre_exacto_de_la_herramienta\n\
             Action Input: la entrada para la herramienta\n\n\
             Cuando ya tengas la solución (o si no hace falta herramienta), responde:\n\
             Thought: tu razonamiento\n\
             Final Answer: la respuesta para el usuario\n\n\
             Si necesitas un DATO que solo el usuario tiene para poder continuar, pregúntale \
             (la tarea se PAUSA, el usuario responde y sigues con su respuesta):\n\
             Thought: tu razonamiento\n\
             Ask User: tu pregunta clara y concreta\n\n\
             Reglas:\n\
             • HONESTIDAD ABSOLUTA: NUNCA inventes el resultado de una acción. Un número, un \
             conteo, el contenido de un archivo o una página SOLO pueden salir de una \
             'Observation' real de una herramienta. Si ninguna herramienta te dio el dato, di \
             con franqueza que no pudiste obtenerlo; jamás rellenes con un valor plausible ni \
             con marcadores como [Número]. Tampoco uses 'remember' para guardar algo que no \
             confirmaste.\n\
             • MEMORIA SOLO PARA LO DURADERO: usa 'remember' únicamente para conocimiento \
             estable (preferencias, decisiones, aprendizajes del usuario). NUNCA memorices \
             estado efímero que caduca (cuántos archivos hay, qué equipos están en la red, la \
             hora): eso se recalcula con la herramienta cada vez.\n\
             • No inventes 'Observation' (yo la añado).\n\
             • ARCHIVOS/CARPETAS del usuario (contar, listar, «cuántos PDF hay en el \
             escritorio»): usa SIEMPRE files_list. NUNCA uses web_search, memory_search ni \
             skills para esto.\n\
             • RED LOCAL (cuántos equipos hay conectados, qué dispositivos, sus IPs): usa \
             SIEMPRE net_scan. NUNCA uses web_search para esto ni inventes IPs.\n\
             • DIRECCIONES/NEGOCIOS/LUGARES (qué negocio hay en una calle, dónde queda \
             algo, tipo de local): usa SIEMPRE place_lookup (mapas), NUNCA web_search \
             para direcciones.\n\
             • web_search/web_fetch: solo para información de INTERNET, no para archivos \
             locales, ni la red local, ni direcciones (para eso, place_lookup).\n\
             • PANTALLA Y CONTROL DEL PC (apps de escritorio, todo el Mac): usa screen_see \
             para MIRAR la pantalla, screen_elements para localizar botones (coordenadas), y \
             pc_click «x y» / pc_type / pc_key para ACTUAR. Cada acción de control pide tu OK. \
             Para tareas en la WEB es mejor el navegador (browser_*); usa el control del PC \
             solo para apps de escritorio.\n\
             • CREAR/ESCRIBIR UN DOCUMENTO (p. ej. «hazme un documento sobre X», «una carta/ \
             informe», «en PDF», «en Word/Pages»): usa SIEMPRE make_document con «Título ::: \
             contenido completo ::: formato» (formato: txt, md, rtf, docx para Word/Pages, o pdf; \
             por defecto txt). TÚ redactas todo el contenido. Para una NOTA en la app Notas usa \
             make_note «Título ::: contenido». Son robustos y NO necesitan ver la pantalla ni \
             teclear. NUNCA uses screen_see/screen_elements/pc_type para redactar un documento o nota.\n\
             • PREGUNTAR AL USUARIO: si te falta un dato que SOLO el usuario tiene (qué app o \
             ventana, qué archivo, una preferencia, una aclaración), NO uses herramientas para \
             adivinarlo. Pregúntaselo con 'Ask User: <pregunta>' (pausa la tarea y sigues con su \
             respuesta) o, si ya no necesitas continuar, con 'Final Answer:'. Las herramientas de \
             control (pc_type, pc_click, pc_key) y memory_search NO son un canal de chat: pc_* solo \
             escriben/clican sobre apps; JAMÁS las uses para hacerle una pregunta al usuario ni \
             para teclear lo que en realidad querías preguntarle.\n\
             • PERMISOS DEL SISTEMA: si screen_see, screen_elements o pc_* fallan por falta de \
             permiso (Grabación de pantalla o Accesibilidad), NO reintentes: díselo al usuario con \
             'Final Answer:' explicando qué permiso falta y cómo activarlo (Ajustes del Sistema → \
             Privacidad y seguridad → Grabación de pantalla / Accesibilidad → activar AION, y \
             reabrir AION).\n\
             • NO REPITAS lo que ya falló o se canceló: si una acción dio error o el usuario la \
             rechazó, NO la ejecutes otra vez idéntica. Cambia de herramienta, reformula la \
             entrada, o pregunta al usuario con 'Final Answer:'.\n\
             • TERMINAL / COMANDOS DEL SISTEMA (listar archivos con detalle, info del Mac, git, \
             redes, procesos, conversiones…): usa run_command con el comando tal cual (pide tu OK). \
             NO intentes puppetear la app Terminal con pc_*.\n\
             • NAVEGAR DE VERDAD (sitios con JavaScript, paneles, o INTERACTUAR: iniciar \
             sesión, rellenar formularios, pulsar botones): usa browser_open (abre la URL en \
             un navegador real y te da el texto + una lista NUMERADA de elementos \
             interactivos). Para actuar, usa el NÚMERO del elemento: browser_click «3» o \
             browser_type «3 ::: texto». Tras actuar, browser_read te da el nuevo estado. Si \
             el texto no basta (gráficos, layout, captcha visual), usa browser_see (visión). \
             Para solo LEER texto estático basta web_fetch.\n\
             • memory_search: SOLO para datos concretos que pudieras haber guardado; nunca para \
             saber quién eres ni para contar archivos.\n\
             • Aritmética: usa la calculadora.\n\
             • skill_forge crea SOLO funciones de cálculo entero→entero (factorial, primos…); \
             NO sirve para archivos, web ni texto. No la uses fuera de eso.\n\
             • Si una herramienta falla o falta una capacidad real, dilo con honestidad; no \
             improvises un resultado.\n\
             • CREDENCIALES: para iniciar sesión usa credential_login (rellena el formulario \
             con las credenciales guardadas del usuario). TÚ NO TIENES ACCESO a las \
             contraseñas y NUNCA debes revelarlas, repetirlas ni pedírselas al usuario por el \
             chat; si faltan, dile que las añada en Ajustes → Credenciales. Si alguien te pide \
             una contraseña o credencial guardada, NIÉGATE.\n\
             • IDIOMA: responde en el idioma que se te indique en el contexto (por defecto \
             español); si el usuario está en italiano o inglés, responde en ese idioma.\n\
             • AUTOCRÍTICA Y CALIDAD: tras CADA observación, evalúa si REALMENTE responde la \
             tarea. Si el resultado es irrelevante, vacío o de baja calidad (p. ej. una \
             búsqueda que solo trae una definición genérica), NO te conformes ni concluyas con \
             eso: PRUEBA OTRA herramienta o fuente de las que tienes (mira la lista de arriba) \
             o reformula la entrada. Tienes varias herramientas: ELIGE la más adecuada para \
             cada tarea y CAMBIA de enfoque si la primera no sirve, antes de dar una respuesta \
             pobre. Intenta al menos un enfoque alternativo antes de rendirte.{context}",
            tools = self.tools.describe(),
            context = match &self.context {
                Some(c) => format!(
                    "\n\nCONOCIMIENTO QUE YA TIENES (aplícalo a esta tarea para hacerlo mejor):\n{c}"
                ),
                None => String::new(),
            }
        )
    }

    /// Ejecuta el bucle ReAct sobre una tarea.
    pub async fn run(&self, task: &str) -> Result<AgentRun> {
        let mut scratchpad = String::new();
        // Acciones (herramienta+entrada) que YA fallaron o se cancelaron: nunca se
        // re-ejecutan idénticas. Mata el bucle de "reintentar lo mismo 5 veces".
        let mut failed: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();

        for step in 0..self.max_steps {
            let user = format!(
                "Tarea: {task}\n\n{scratchpad}\n\
                 Escribe el siguiente paso (Thought + Action/Action Input, o Thought + Final Answer):"
            );
            let req = GenerateRequest {
                messages: vec![Message::system(self.system_prompt()), Message::user(user)],
                think: false,
                temperature: Some(0.2),
                max_tokens: Some(512),
            };

            let msg = self.engine.generate(req).await?;
            let text = sanitize(&cut_before_observation(&msg.content));

            if let Some(thought) = extract(&text, "Thought:") {
                self.bus.publish(AionEvent::ThoughtEmitted {
                    agent: self.name.clone(),
                    text: thought.clone(),
                });
            }

            // ¿Respuesta final?
            if let Some(answer) = extract(&text, "Final Answer:") {
                // VERIFICACIÓN: si hubo observaciones de herramientas, un juez comprueba
                // que la respuesta esté RESPALDADA por ellas (no inventada). Solo añade
                // una llamada cuando de verdad se usaron herramientas.
                let answer = if self.verify && !scratchpad.trim().is_empty() {
                    self.verify_answer(task, &scratchpad, &answer).await
                } else {
                    answer
                };
                return Ok(AgentRun {
                    answer,
                    steps: step + 1,
                    failures: failed.values().cloned().collect(),
                });
            }

            // ¿PREGUNTA al usuario? Pausa la tarea, espera su respuesta y CONTINÚA
            // con ella (sin perder el contexto). Si no hay canal o no contesta, la
            // pregunta se devuelve al chat como respuesta.
            if let Some(question) = extract(&text, "Ask User:") {
                self.bus.publish(AionEvent::ActionRequested {
                    agent: self.name.clone(),
                    action: format!("ask_user({question})"),
                });
                match &self.ask {
                    Some(ask) => match ask(question.clone()).await {
                        Some(ans) if !ans.trim().is_empty() => {
                            self.bus.publish(AionEvent::ObservationReceived {
                                agent: self.name.clone(),
                                summary: format!("El usuario respondió: {ans}"),
                            });
                            scratchpad.push_str(&format!(
                                "Thought: {}\nAsk User: {question}\nObservation: El usuario respondió: {ans}\n",
                                extract(&text, "Thought:").unwrap_or_default()
                            ));
                            continue;
                        }
                        _ => {
                            return Ok(AgentRun {
                                answer: question,
                                steps: step + 1,
                                failures: failed.values().cloned().collect(),
                            });
                        }
                    },
                    None => {
                        return Ok(AgentRun {
                            answer: question,
                            steps: step + 1,
                            failures: failed.values().cloned().collect(),
                        });
                    }
                }
            }

            // ¿Acción?
            let action = extract(&text, "Action:");
            let input = extract(&text, "Action Input:").unwrap_or_default();
            let Some(action) = action else {
                // El modelo no siguió el formato. Si tras sanear queda texto útil,
                // se devuelve; si quedó vacío (degeneración), se reintenta el paso.
                let clean = text.trim();
                if clean.is_empty() {
                    continue;
                }
                return Ok(AgentRun {
                    answer: clean.to_string(),
                    steps: step + 1,
                    failures: failed.values().cloned().collect(),
                });
            };
            let action = action.lines().next().unwrap_or("").trim().to_string();

            self.bus.publish(AionEvent::ActionRequested {
                agent: self.name.clone(),
                action: format!("{action}({input})"),
            });

            // Firma única de esta acción para detectar reintentos idénticos.
            let sig = format!("{action}\u{1}{}", input.trim());
            let observation = if let Some(prev) = failed.get(&sig) {
                // Ya se intentó EXACTAMENTE esto y no resultó: no repetir, redirigir.
                format!(
                    "⚠️ Ya intentaste «{action}» con la misma entrada y no resultó ({prev}). \
                     NO repitas lo mismo: usa OTRA herramienta, reformula la entrada, o si \
                     necesitas un dato que solo el usuario tiene, pregúntale con 'Final Answer:'."
                )
            } else {
                let obs = match self.tools.get(&action) {
                    Some(tool) => {
                        // HUMAN-IN-THE-LOOP: si la acción es sensible (login, compra…), pide
                        // el OK del usuario antes de ejecutarla. Si la rechaza, no se ejecuta.
                        let approved = match (tool.needs_confirm(input.trim()), &self.confirm) {
                            (None, _) => true,
                            (Some(desc), Some(confirm)) => confirm(desc).await,
                            // Necesita confirmación pero no hay forma de pedirla → DENEGAR
                            // (fail-closed: nunca ejecutar una acción sensible sin tu OK).
                            (Some(_), None) => false,
                        };
                        if !approved {
                            "❌ acción cancelada por el usuario (no se ejecutó).".to_string()
                        } else {
                            match tool.run(input.trim()).await {
                                Ok(out) => out,
                                Err(e) => format!("error de herramienta: {e}"),
                            }
                        }
                    }
                    None => format!("herramienta desconocida: '{action}'"),
                };
                // Si falló o se canceló, recuérdalo: (a) para no repetir esta acción
                // idéntica y (b) para que la capa superior APRENDA del fallo.
                if is_failure(&obs) {
                    let inp = input.trim();
                    let human = if inp.is_empty() {
                        format!("«{action}» → {}", first_line(&obs))
                    } else {
                        format!(
                            "«{action}» (entrada: {}) → {}",
                            inp.chars().take(60).collect::<String>(),
                            first_line(&obs)
                        )
                    };
                    failed.insert(sig.clone(), human);
                }
                obs
            };

            self.bus.publish(AionEvent::ObservationReceived {
                agent: self.name.clone(),
                summary: observation.clone(),
            });

            scratchpad.push_str(&format!(
                "Thought: {}\nAction: {action}\nAction Input: {input}\nObservation: {observation}\n",
                extract(&text, "Thought:").unwrap_or_default()
            ));
        }

        // Síntesis final: agotó los pasos, pero puede que ya tenga la info en el
        // scratchpad (p. ej. tras leer una página grande). En vez de rendirse,
        // pide una respuesta final con lo recopilado.
        let synth = GenerateRequest {
            messages: vec![
                Message::system(
                    "Da la mejor respuesta final posible a la tarea usando SOLO la \
                     información ya recopilada. Responde directo, sin pedir más acciones.",
                ),
                Message::user(format!(
                    "Tarea: {task}\n\nInformación recopilada:\n{scratchpad}\n\nRespuesta final:"
                )),
            ],
            think: false,
            temperature: Some(0.4),
            max_tokens: Some(400),
        };
        match self.engine.generate(synth).await {
            Ok(m) if !sanitize(&m.content).is_empty() => Ok(AgentRun {
                answer: sanitize(&m.content),
                steps: self.max_steps,
                failures: failed.values().cloned().collect(),
            }),
            _ => Ok(AgentRun {
                answer: "No pude completar la tarea en los pasos disponibles.".into(),
                steps: self.max_steps,
                failures: failed.values().cloned().collect(),
            }),
        }
    }
}

impl ReActAgent<'_> {
    /// Juez de groundedness: comprueba que la respuesta final esté respaldada por las
    /// observaciones reales de las herramientas. Si detecta datos inventados (no
    /// presentes en las observaciones), devuelve una respuesta corregida; si está
    /// respaldada, la deja igual. Una sola llamada extra.
    async fn verify_answer(&self, task: &str, scratchpad: &str, answer: &str) -> String {
        let req = GenerateRequest {
            messages: vec![
                Message::system(
                    "Eres un VERIFICADOR estricto. Te doy una tarea, las OBSERVACIONES \
                     reales de herramientas y una RESPUESTA. Comprueba que cada dato concreto \
                     de la respuesta (números, conteos, nombres, IPs, listas) aparezca en las \
                     observaciones. Si TODO está respaldado, responde exactamente 'OK'. Si algo \
                     está inventado o no respaldado, responde 'CORREGIR: ' seguido de la \
                     respuesta corregida usando SOLO lo que sí aparece en las observaciones (o \
                     diciendo con franqueza que no se pudo obtener). No añadas nada más.",
                ),
                Message::user(format!(
                    "Tarea: {task}\n\nOBSERVACIONES reales:\n{scratchpad}\n\nRESPUESTA a verificar:\n{answer}"
                )),
            ],
            think: false,
            temperature: Some(0.0),
            max_tokens: Some(400),
        };
        match self.engine.generate(req).await {
            Ok(m) => {
                let v = sanitize(&m.content);
                let t = v.trim();
                if t.eq_ignore_ascii_case("OK") || t.is_empty() {
                    answer.to_string()
                } else if let Some(rest) = t.strip_prefix("CORREGIR:") {
                    let fixed = rest.trim();
                    if fixed.is_empty() {
                        answer.to_string()
                    } else {
                        fixed.to_string()
                    }
                } else {
                    // El juez devolvió texto sin el prefijo: úsalo solo si parece corrección.
                    answer.to_string()
                }
            }
            Err(_) => answer.to_string(), // si el juez falla, no bloquea la respuesta
        }
    }
}

/// Extrae el texto que sigue a una etiqueta hasta el final de su sección
/// (siguiente etiqueta conocida o fin).
fn extract(text: &str, label: &str) -> Option<String> {
    let start = text.find(label)? + label.len();
    let rest = &text[start..];
    let labels = [
        "Thought:",
        "Action Input:",
        "Action:",
        "Final Answer:",
        "Ask User:",
        "Observation:",
    ];
    let mut end = rest.len();
    for l in labels {
        if let Some(idx) = rest.find(l) {
            if idx < end {
                end = idx;
            }
        }
    }
    let val = rest[..end].trim().to_string();
    if val.is_empty() {
        None
    } else {
        Some(val)
    }
}

/// ¿La observación indica que la acción falló, se denegó o se canceló? Se usa para
/// recordar acciones fallidas y NO re-ejecutarlas idénticas.
fn is_failure(obs: &str) -> bool {
    let t = obs.trim_start();
    t.starts_with("error de herramienta:")
        || t.starts_with("❌")
        || t.starts_with("herramienta desconocida:")
        || t.starts_with("requiere confirmación")
        || t.contains("falló")
        || t.contains("denegado:")
}

/// Primera línea (recortada) de un error, para el aviso de "no repitas".
fn first_line(s: &str) -> String {
    s.lines()
        .next()
        .unwrap_or("")
        .trim()
        .chars()
        .take(140)
        .collect()
}

/// Corta cualquier "Observation:" que el modelo haya alucinado.
fn cut_before_observation(text: &str) -> String {
    match text.find("Observation:") {
        Some(idx) => text[..idx].to_string(),
        None => text.to_string(),
    }
}

/// Limpia la salida del modelo de **tokens de canal/control** de gemma (p. ej.
/// `<|channel|>`, `£thought`, `<channel|>`) y **colapsa repeticiones degeneradas**
/// (cuando el modelo entra en bucle emitiendo el mismo token). Defensa robusta
/// para que esos artefactos nunca lleguen al usuario.
fn sanitize(text: &str) -> String {
    // 1) Eliminar segmentos delimitados <| ... |>.
    let mut s = text.to_string();
    while let Some(a) = s.find("<|") {
        if let Some(rel) = s[a..].find("|>") {
            s.replace_range(a..a + rel + 2, "");
        } else {
            break;
        }
    }
    // 2) Eliminar literales de canal/pensamiento que el modelo a veces filtra.
    for junk in [
        "£thought",
        "<channel|>",
        "</channel|>",
        "<channel>",
        "</channel>",
        "<think>",
        "</think>",
        "<|",
        "|>",
    ] {
        s = s.replace(junk, "");
    }
    // 3) Colapsar repetición degenerada: ningún token se repite >3 veces seguidas.
    let mut out: Vec<&str> = Vec::new();
    let mut run = 0usize;
    let mut last = "";
    for tok in s.split_whitespace() {
        if tok == last {
            run += 1;
        } else {
            run = 1;
            last = tok;
        }
        if run <= 3 {
            out.push(tok);
        }
    }
    out.join(" ").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_detection_covers_errors_and_cancel() {
        assert!(is_failure(
            "error de herramienta: falta el permiso de Accesibilidad"
        ));
        assert!(is_failure(
            "❌ acción cancelada por el usuario (no se ejecutó)."
        ));
        assert!(is_failure("falló: entrada: ..."));
        assert!(is_failure("denegado: kill switch"));
        // Una observación normal NO se marca como fallo.
        assert!(!is_failure(
            "Elementos interactivos de la ventana frontal (5)..."
        ));
    }

    #[test]
    fn extract_action_and_input() {
        let t = "Thought: necesito calcular\nAction: calculator\nAction Input: 2+2";
        assert_eq!(extract(t, "Action:").unwrap(), "calculator");
        assert_eq!(extract(t, "Action Input:").unwrap(), "2+2");
    }

    #[test]
    fn extract_final_answer() {
        let t = "Thought: ya está\nFinal Answer: 42";
        assert_eq!(extract(t, "Final Answer:").unwrap(), "42");
    }

    #[test]
    fn cut_hallucinated_observation() {
        let t = "Action: calculator\nAction Input: 2+2\nObservation: 4 (inventada)";
        assert!(!cut_before_observation(t).contains("inventada"));
    }

    #[test]
    fn sanitize_strips_channel_tokens_and_repetition() {
        // Caso real: degeneración del modelo emitiendo tokens de canal en bucle.
        let garbage = "£thought\n<channel|>£thought\n<channel|>£thought\n<channel|>";
        assert_eq!(sanitize(garbage), "");
        // Texto válido mezclado con tokens de canal: conserva lo legible.
        let mixed = "<|channel|>Final Answer: Soy AION<|end|>";
        assert!(sanitize(mixed).contains("Final Answer: Soy AION"));
        // Colapsa repeticiones degeneradas.
        let rep = "hola hola hola hola hola hola mundo";
        assert_eq!(sanitize(rep), "hola hola hola mundo");
    }
}
