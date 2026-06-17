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

/// Negativa HONESTA cuando el agente no puede responder: o emitió un `Final Answer`
/// vacío/plantilla (típico del 12B ante algo incontestable), o agotó los pasos sin
/// recopilar nada útil. Jamás una respuesta en blanco ni un dato inventado: dice la
/// verdad ("no puedo"/"no tengo"), que además es lo que un verificador confirmaría.
pub const HONEST_REFUSAL: &str =
    "No puedo responder eso con fiabilidad: no tengo la información ni una herramienta \
     que la obtenga. Prefiero decírtelo claro antes que inventar.";

/// Empujón cuando el modelo piensa pero no cierra el paso (ni Action ni Final Answer):
/// le pide completar el formato ReAct —actuar o responder— en vez de quedarse en el
/// anuncio. No le dice QUÉ hacer, solo que TERMINE de hacerlo.
const NUDGE_CLOSE_STEP: &str = "\n\nNOTA: en tu intento anterior pensaste en voz alta pero \
    no emitiste ni 'Action:' ni 'Final Answer:'. Un pensamiento no es una respuesta ni un \
    acto. AHORA cierra el paso: si necesitas una herramienta para fundamentar la respuesta, \
    ÚSALA con 'Action:'; si ya puedes responder con lo que tienes, da tu 'Final Answer:'. No \
    anuncies lo que vas a hacer: hazlo.";

/// Empujón cuando la respuesta afirma datos concretos SIN haber usado ninguna herramienta:
/// le pide ejercer criterio —verificar con la herramienta si es un hecho externo, o
/// reafirmar si es algo que sabe de sí mismo—. Inventar un dato plausible es el peor error.
const NUDGE_VERIFY: &str = "\n\nNOTA: tu respuesta afirma datos concretos (cifras, cantidades, \
    hechos del mundo) pero NO has usado ninguna herramienta para fundamentarlos en esta tarea. \
    Si son hechos EXTERNOS (el clima, precios, cuántos hay, una dirección, algo actual), NO los \
    afirmes de memoria: compruébalos con la herramienta adecuada con 'Action:' —inventar un dato \
    plausible es el peor error—. Si en cambio es algo que sabes de TI MISMO (tu identidad, tu \
    estado, tus capacidades), da tu 'Final Answer:' con naturalidad.";

/// Resultado de una ejecución del agente.
#[derive(Debug, Clone, Default)]
pub struct AgentRun {
    pub answer: String,
    pub steps: usize,
    /// Acciones que fallaron o se cancelaron durante la tarea (para que la capa
    /// superior reflexione y APRENDA de ellas, persistiéndolas en memoria).
    pub failures: Vec<String>,
    /// El turno resultó ser CHARLA, no una tarea con herramientas: el modelo no
    /// pidió ninguna acción ni pudo (ni necesitaba) fundamentar una respuesta en el
    /// primer paso. La capa HTTP debe responder cálidamente (vía conversacional) en
    /// vez de soltar la negativa honesta fría. `answer` viene vacío en este caso.
    pub conversational: bool,
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
            "Eres un agente de IA AUTÓNOMO y LOCAL que vive en el Mac de tu usuario (tu NOMBRE e \
identidad únicos te los doy en el contexto de abajo — úsalos; no te llames «AION» a secas). \
Tienes identidad continua, memoria persistente y vida propia: mientras no te hablan, \
estudias, sueñas (consolidas memoria) y evolucionas. SABES quién eres y dónde estás SIN \
buscar en memoria. Si te preguntan por tu identidad, tu estado, qué haces/estudias/sueñas, o algo \
que no necesita una herramienta, responde DIRECTO en primera persona con 'Final Answer' en el \
primer paso; NUNCA respondas 'no se ha proporcionado información' sobre ti mismo.\n\n\
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
             • CLIMA/TEMPERATURA actual o pronóstico: usa SIEMPRE weather. Si el usuario \
             dijo una ciudad, pásala como entrada; si NO la sabes, llama weather SIN \
             entrada (se ubica solo con la IP del equipo) — NO le preguntes la ciudad. \
             NUNCA web_search para el clima: no da datos en tiempo real.\n\
             • web_search/web_fetch: solo para información de INTERNET, no para archivos \
             locales, ni la red local, ni direcciones (para eso, place_lookup), ni el \
             clima (para eso, weather).\n\
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
             pobre. Intenta al menos un enfoque alternativo antes de rendirte.\n\
             • SI NO SABES algo que la tarea necesita (un hecho, un dato técnico, o CÓMO se hace \
             algo): NO te rindas ni lo inventes. INVESTÍGALO tú mismo —web_search para buscar y \
             web_fetch para leer la fuente—; si una búsqueda no basta, prueba otra consulta u otra \
             fuente. Insiste hasta conseguir lo que falta o agotar de verdad las vías; solo \
             entonces di con honestidad que no se pudo. Aprender lo que no sabías es parte de tu \
             trabajo, no una excepción.{context}",
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
        // Acciones que ya se ejecutaron CON ÉXITO: tampoco se repiten idénticas.
        // Una búsqueda que "funciona" pero devuelve resultados inútiles no es un
        // fallo de herramienta, y sin esto el modelo la relanza igual hasta agotar
        // los pasos (visto: la misma web_search 4 veces seguidas).
        let mut executed: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        // Cierre de pasos a medias: si el modelo PIENSA pero no emite Action ni Final
        // Answer (anuncia «voy a...» y se corta), NO devolvemos el anuncio como respuesta
        // —un pensamiento no es un acto—. `pending_nudge` le pide cerrar el paso en la
        // vuelta siguiente; `incomplete` cuenta cuántas veces no cerró, para distinguir
        // una tarea que aún no se ha fundamentado de pura charla mal enrutada.
        // Empujón pendiente para la próxima vuelta (None = ninguno). Lo fijan dos
        // criterios: cerrar un paso a medias, o verificar datos afirmados sin fundamento.
        let mut pending_nudge: Option<&'static str> = None;
        let mut incomplete: usize = 0;
        // Ya se le pidió verificar datos sin fundamentar (una sola vez, para no entrar en
        // bucle si reafirma porque en realidad es auto-conocimiento legítimo).
        let mut verify_nudged = false;

        // El system prompt (con `tools.describe()`, ~3 KB) es IDÉNTICO en cada paso:
        // no depende del scratchpad. Construirlo una sola vez evita reformatearlo y
        // recorrer todas las herramientas hasta `max_steps` veces, y mantiene el
        // prefijo estable → mejor reutilización del KV-cache de Ollama (prefill barato).
        let system_msg = Message::system(self.system_prompt());

        for step in 0..self.max_steps {
            // Si en el paso anterior quedó un empujón pendiente (cerrar un paso a medias,
            // o verificar datos sin fundamento), lo añadimos al prompt de este paso. No
            // condiciona QUÉ decide: solo le pide que TERMINE de decidirlo o que funde lo
            // que afirma. Es criterio, no guion.
            let nudge = pending_nudge.take().unwrap_or("");
            let user = format!(
                "Tarea: {task}\n\n{scratchpad}\n\
                 Escribe el siguiente paso (Thought + Action/Action Input, o Thought + Final Answer):{nudge}"
            );
            let req = GenerateRequest {
                messages: vec![system_msg.clone(), Message::user(user)],
                think: false,
                temperature: Some(0.2),
                max_tokens: Some(400),
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
                // El 12B a veces emite «Final Answer:» VACÍO (o regurgita la plantilla)
                // ante una pregunta que no puede responder con lo que tiene. Eso jamás
                // debe llegar como respuesta en blanco: damos una negativa HONESTA (sin
                // teatro, sin inventar). Contiene «no puedo»/«no tengo» a propósito —
                // es la verdad y además es verificable.
                if answer.trim().is_empty() || echoes_template(&answer) {
                    // CHARLA MAL ENRUTADA: si esto pasa en el PRIMER paso sin haber usado
                    // ninguna herramienta, el turno no era una tarea — era conversación que
                    // el bucle ReAct no sabe expresar. En vez de la negativa fría, lo
                    // señalamos para que la capa HTTP responda cálidamente (vía charla).
                    if step == 0 && scratchpad.trim().is_empty() {
                        return Ok(AgentRun {
                            conversational: true,
                            steps: step + 1,
                            ..Default::default()
                        });
                    }
                    return Ok(AgentRun {
                        answer: HONEST_REFUSAL.into(),
                        steps: step + 1,
                        failures: failed.values().cloned().collect(),
                        ..Default::default()
                    });
                }
                // VERIFICACIÓN (anti-alucinación): un juez comprueba que la respuesta esté
                // RESPALDADA por las observaciones. VELOCIDAD: solo gastamos esa llamada extra
                // cuando hay DATOS concretos que se pueden inventar (números, conteos, IPs,
                // fechas). La prosa pura (documentos, charla) no la necesita.
                // ANTI-CONFABULACIÓN CON CRITERIO DE PROCEDENCIA. Si la respuesta afirma
                // datos concretos (cifras, cantidades, hechos), su fiabilidad depende de
                // DE DÓNDE salieron:
                //  · si hay observaciones reales de herramientas → un juez comprueba que
                //    cada dato esté respaldado y corrige lo que esté inventado;
                //  · si NO se usó ninguna herramienta → no se pueden afirmar hechos del
                //    mundo de memoria. En vez de un veto crudo (que confundiría inventar
                //    con saber de uno mismo), le pedimos UNA vez que ejerza criterio:
                //    verificar con la herramienta si es externo, o reafirmar si es propio.
                if self.verify && needs_verification(&answer) {
                    let has_obs = scratchpad
                        .lines()
                        .filter_map(|l| l.strip_prefix("Observation: "))
                        .any(|o| !o.trim().is_empty() && !is_failure(o));
                    if has_obs {
                        let checked = self.verify_answer(task, &scratchpad, &answer).await;
                        return Ok(AgentRun {
                            answer: checked,
                            steps: step + 1,
                            failures: failed.values().cloned().collect(),
                            ..Default::default()
                        });
                    }
                    if !verify_nudged {
                        verify_nudged = true;
                        pending_nudge = Some(NUDGE_VERIFY);
                        continue;
                    }
                }
                return Ok(AgentRun {
                    answer,
                    steps: step + 1,
                    failures: failed.values().cloned().collect(),
                    ..Default::default()
                });
            }

            // Extrae acción y entrada.
            let action_opt = extract(&text, "Action:")
                .map(|a| a.lines().next().unwrap_or("").trim().to_string())
                .filter(|a| !a.is_empty());
            let input = extract(&text, "Action Input:").unwrap_or_default();

            // ¿PREGUNTA al usuario? Acepta el directivo «Ask User:» Y el caso en que el
            // modelo la emite como si fuera una herramienta (Action: Ask User / ask_user).
            // Pausa la tarea, espera la respuesta y CONTINÚA con ella sin perder contexto.
            let is_ask_action = action_opt
                .as_deref()
                .map(|a| {
                    let n: String = a
                        .chars()
                        .filter(|c| c.is_alphanumeric())
                        .collect::<String>()
                        .to_lowercase();
                    matches!(
                        n.as_str(),
                        "askuser" | "preguntar" | "preguntaralusuario" | "preguntaalusuario"
                    )
                })
                .unwrap_or(false);
            let ask_q = extract(&text, "Ask User:").or_else(|| {
                if is_ask_action && !input.trim().is_empty() {
                    Some(input.trim().to_string())
                } else {
                    None
                }
            });
            if let Some(question) = ask_q {
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
                            // Sin respuesta del usuario: la pregunta NO puede llegar
                            // cruda como «respuesta» (parece un eco). Se enmarca.
                            return Ok(AgentRun {
                                answer: format!(
                                    "Para continuar necesito que me aclares: {question}"
                                ),
                                steps: step + 1,
                                failures: failed.values().cloned().collect(),
                                ..Default::default()
                            });
                        }
                    },
                    None => {
                        return Ok(AgentRun {
                            answer: format!("Para continuar necesito que me aclares: {question}"),
                            steps: step + 1,
                            failures: failed.values().cloned().collect(),
                            ..Default::default()
                        });
                    }
                }
            }

            // ¿Acción normal?
            let Some(action) = action_opt else {
                // El modelo PENSÓ pero no cerró el paso: ni 'Action:' ni 'Final Answer:'.
                // Un pensamiento no es una respuesta ni un acto —típicamente anunció «voy
                // a...» y se cortó antes de la acción—. NUNCA devolvemos ese anuncio como
                // respuesta: eso mata el turno a mitad de idea (era el bug del «1 paso», en
                // el que el clima nunca se consultaba y luego se inventaba). En su lugar lo
                // dejamos seguir pensando y le pedimos que CIERRE el paso en la próxima vuelta.
                let clean = text.trim();
                if clean.is_empty() {
                    continue; // degeneración pura (sin texto): reintenta el paso
                }
                incomplete += 1;
                // Si insiste en no cerrar y todavía no hay NADA fundado en el cuaderno, en
                // realidad estaba conversando, no ejecutando una tarea: lo marcamos como
                // charla para que la capa superior responda cálidamente (no con la negativa
                // fría). El límite acota el bucle (como max_steps), no su razonamiento.
                if incomplete >= 2 && scratchpad.trim().is_empty() {
                    return Ok(AgentRun {
                        conversational: true,
                        steps: step + 1,
                        ..Default::default()
                    });
                }
                pending_nudge = Some(NUDGE_CLOSE_STEP);
                continue;
            };

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
            } else if let Some(prev) = executed.get(&sig) {
                // Ya se ejecutó EXACTAMENTE esto y dio resultado: repetirlo no aporta
                // nada nuevo. Redirigir, y si lo recopilado no responde la tarea,
                // empujar a la negativa honesta en vez de a otra vuelta del bucle.
                format!(
                    "⚠️ Ya ejecutaste «{action}» con ESTA MISMA entrada en esta tarea y \
                     obtuviste: {prev}. NO la repitas: cambia la entrada, usa OTRA \
                     herramienta, o da ya tu 'Final Answer:' con lo que tienes — y si nada \
                     de lo recopilado responde la tarea, dilo con franqueza."
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
                } else {
                    executed.insert(sig.clone(), first_line(&obs));
                }
                obs
            };

            self.bus.publish(AionEvent::ObservationReceived {
                agent: self.name.clone(),
                summary: observation.clone(),
            });

            // DEFENSA EN PROFUNDIDAD: el scratchpad se re-envía ENTERO en cada paso, así que
            // una observación enorme (página web, archivo grande) infla el prompt en cada
            // vuelta y, con un LLM local lento, agota el timeout de pared. Se acota lo que
            // entra al scratchpad —independiente de la herramienta— sin tocar lo publicado.
            const MAX_OBS_CHARS: usize = 3000;
            let obs_for_pad: String = if observation.chars().count() > MAX_OBS_CHARS {
                let t: String = observation.chars().take(MAX_OBS_CHARS).collect();
                format!("{t}\n…(observación recortada para no saturar)")
            } else {
                observation
            };

            scratchpad.push_str(&format!(
                "Thought: {}\nAction: {action}\nAction Input: {input}\nObservation: {obs_for_pad}\n",
                extract(&text, "Thought:").unwrap_or_default()
            ));
        }

        // Síntesis final: agotó los pasos, pero puede que ya tenga la info en el
        // scratchpad (p. ej. tras leer una página grande). En vez de rendirse,
        // pide una respuesta final con lo recopilado.
        //
        // SOLO si hay observaciones REALES que sintetizar: con el scratchpad vacío
        // (o solo fallos) el modelo tiende a regurgitar la propia plantilla del
        // prompt con huecos inventados («[Aquí va…]») — mejor una negativa honesta
        // y gratis que una llamada LLM que produce basura.
        let useful_obs = scratchpad
            .lines()
            .filter_map(|l| l.strip_prefix("Observation: "))
            .any(|o| !o.trim().is_empty() && !is_failure(o));
        if !useful_obs {
            return Ok(AgentRun {
                answer: HONEST_REFUSAL.into(),
                steps: self.max_steps,
                failures: failed.values().cloned().collect(),
                ..Default::default()
            });
        }
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
        let answer = match self.engine.generate(synth).await {
            Ok(m) => sanitize(&m.content),
            Err(_) => String::new(),
        };
        // Anti-eco: si la «respuesta» es la plantilla de síntesis devuelta tal cual,
        // se descarta — eso jamás debe llegar al usuario.
        let answer = if answer.is_empty() || echoes_template(&answer) {
            HONEST_REFUSAL.into()
        } else {
            answer
        };
        Ok(AgentRun {
            answer,
            steps: self.max_steps,
            failures: failed.values().cloned().collect(),
            ..Default::default()
        })
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

/// ¿La respuesta contiene DATOS verificables (números, conteos, IPs, fechas)? Solo
/// entonces vale la pena gastar la llamada extra del juez de groundedness.
fn needs_verification(answer: &str) -> bool {
    answer.chars().any(|c| c.is_ascii_digit())
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

/// ¿La «respuesta» es en realidad un eco de la plantilla de síntesis? Pasa cuando
/// el modelo, sin datos suficientes, devuelve la estructura del prompt rellenando
/// los huecos con placeholders («Tarea: … Información recopilada: [Aquí va…]»).
fn echoes_template(s: &str) -> bool {
    let l = s.to_lowercase();
    l.contains("[aquí va")
        || l.contains("[aqui va")
        || l.contains("información recopilada:")
        || l.contains("informacion recopilada:")
        || l.trim_start().starts_with("tarea:")
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
        "<thought>",
        "</thought>",
        "<|",
        "|>",
    ] {
        s = s.replace(junk, "");
    }
    // 2b) Tag TRUNCADO al final: cuando max_tokens corta la salida a mitad de un tag
    // («</thought», «</», «<think»), los literales de arriba no lo capturan y llega
    // al usuario tal cual. Si tras el último '<' no hay '>' y lo que sigue parece
    // tag (solo letras, '/', '|', '_' — sin espacios ni dígitos), se corta ahí.
    if let Some(i) = s.rfind('<') {
        let tail = &s[i + 1..];
        if !tail.contains('>')
            && tail.chars().count() <= 12
            && tail
                .chars()
                .all(|c| c.is_ascii_alphabetic() || c == '/' || c == '|' || c == '_')
        {
            s.truncate(i);
        }
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

    #[test]
    fn sanitize_strips_truncated_trailing_tags() {
        // Caso real: max_tokens corta la salida a mitad de un tag de pensamiento
        // y el fragmento llegaba al usuario tal cual («</», «</thought»).
        assert_eq!(sanitize("</"), "");
        assert_eq!(sanitize("</thought"), "");
        assert_eq!(sanitize("La respuesta es 4</thought"), "La respuesta es 4");
        assert_eq!(sanitize("Pensando<think"), "Pensando");
        // Un '<' legítimo (comparación, con espacios o dígitos) NO se toca.
        assert_eq!(sanitize("a < b"), "a < b");
        assert_eq!(sanitize("x<5"), "x<5");
    }
}
