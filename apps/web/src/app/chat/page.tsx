"use client";

import { useEffect, useRef, useState } from "react";
import AppShell from "@/components/AppShell";
import Icon from "@/components/Icon";
import Markdown from "@/components/Markdown";
import { useT } from "@/lib/i18n";
import {
  agentStream,
  crewStream,
  chatStream,
  chatReset,
  confirmDecision,
  answerQuestion,
  getGreeting,
  inboxList,
  inboxRead,
  libraryUpload,
  visionAsk,
  status,
  providerGet,
  providerToggle,
  type AgentEvent,
  type ChatEvent,
  type ProviderState,
} from "@/lib/api";

type Step = { kind: "thought" | "action" | "observation"; text: string; agent?: string };
type Mode = "chat" | "agent" | "crew";
type Turn = {
  prompt: string;
  mode: Mode;
  thinking: string;
  steps: Step[];
  answer: string;
  meta?: string;
  /** Mensaje que AION inició por su cuenta (saludo/aviso): se muestra sin burbuja de usuario. */
  reach?: { kind: string; at: string };
};
type ConvoMeta = { id: string; title: string; updatedAt: number };

// ── Persistencia de conversaciones (cliente) ──
const LS_LIST = "aion_convos";
const turnsKey = (id: string) => `aion_convo_${id}`;
function loadList(): ConvoMeta[] {
  try {
    return JSON.parse(localStorage.getItem(LS_LIST) ?? "[]");
  } catch {
    return [];
  }
}
function saveList(list: ConvoMeta[]) {
  localStorage.setItem(LS_LIST, JSON.stringify(list));
}
function loadTurns(id: string): Turn[] {
  try {
    return JSON.parse(localStorage.getItem(turnsKey(id)) ?? "[]");
  } catch {
    return [];
  }
}
function newId(): string {
  return `c_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 7)}`;
}

const STEP_STYLE: Record<Step["kind"], { icon: React.ComponentProps<typeof Icon>["name"]; color: string }> = {
  thought: { icon: "sparkle", color: "var(--on-lavender)" },
  action: { icon: "code", color: "var(--on-sky)" },
  observation: { icon: "eye", color: "var(--on-peach)" },
};

export default function ChatPage() {
  const { t } = useT();
  const [input, setInput] = useState("");
  const [mode, setMode] = useState<Mode>("agent");
  const [think, setThink] = useState(true);
  const [turns, setTurns] = useState<Turn[]>([]);
  const [busy, setBusy] = useState(false);
  const [modelReady, setModelReady] = useState(true);
  // Proveedor del motor (local Ollama / API externa) para el indicador + toggle del header.
  const [prov, setProv] = useState<ProviderState | null>(null);
  const [provBusy, setProvBusy] = useState(false);

  // Alterna local↔API en un clic. Solo se ofrece si ambos están configurados (can_toggle).
  async function toggleEngine() {
    if (provBusy) return;
    setProvBusy(true);
    try {
      await providerToggle();
      const p = await providerGet().catch(() => null);
      if (p) setProv(p);
    } finally {
      setProvBusy(false);
    }
  }

  // Añade un mensaje que AION inició (saludo/aviso) como un turno cronológico al final
  // (sin burbuja de usuario). Dedup por texto para no duplicar en recargas.
  function addReachTurn(text: string, kind: string, at: string) {
    setTurns((prev) => {
      if (prev.some((t) => t.reach && t.answer.trim() === text.trim())) return prev;
      return [...prev, { prompt: "", mode: "chat", thinking: "", steps: [], answer: text, reach: { kind, at } }];
    });
  }
  // Conversaciones persistentes: id actual + lista + dropdown de historial.
  const [convoId, setConvoId] = useState<string>("");
  const [convos, setConvos] = useState<ConvoMeta[]>([]);
  const [showHistory, setShowHistory] = useState(false);
  // Confirmación humana pendiente (login, compra…): se muestra una tarjeta con OK/No.
  const [pendingConfirm, setPendingConfirm] = useState<{ id: string; text: string } | null>(null);
  const [pendingAsk, setPendingAsk] = useState<{ id: string; text: string } | null>(null);
  const [askDraft, setAskDraft] = useState("");
  // Adjunto de imagen pendiente (se envía con el siguiente mensaje, vía visión).
  const [pendingImage, setPendingImage] = useState<{ name: string; b64: string } | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const endRef = useRef<HTMLDivElement>(null);
  // ¿Ariel ya habló en esta sesión? Si saludó él primero, el saludo automático
  // de apertura se descarta — saludar dos veces rompe la sensación de vida.
  const userSpokeRef = useRef(false);
  // Aborta el stream SSE en curso al desmontar o al cambiar de conversación. Sin esto
  // el fetch sigue vivo en background y sus callbacks escriben sobre los turnos de OTRA
  // conversación (estado corrupto) o sobre un componente ya desmontado (warning de React).
  const streamAbort = useRef<AbortController | null>(null);

  // Al montar: restaura la última conversación (o crea una). Arregla la pérdida del
  // chat al navegar entre menús.
  useEffect(() => {
    const list = loadList();
    setConvos(list);
    if (list.length > 0) {
      setConvoId(list[0].id);
      setTurns(loadTurns(list[0].id));
    } else {
      setConvoId(newId());
    }
  }, []);

  // Al desmontar: corta cualquier stream en vuelo (evita fetch zombie + setState tras
  // desmontar). El cambio de conversación lo aborta `newChat`/`openConvo`.
  useEffect(() => () => streamAbort.current?.abort(), []);

  // Persiste los turnos de la conversación actual + actualiza su título en la lista.
  useEffect(() => {
    if (!convoId) return;
    localStorage.setItem(turnsKey(convoId), JSON.stringify(turns));
    if (turns.length === 0) return;
    setConvos((prev) => {
      const title = turns[0].prompt.slice(0, 40) || "Nueva conversación";
      const others = prev.filter((c) => c.id !== convoId);
      const next = [{ id: convoId, title, updatedAt: Date.now() }, ...others];
      saveList(next);
      return next;
    });
  }, [turns, convoId]);

  function newChat() {
    streamAbort.current?.abort();
    const id = newId();
    setConvoId(id);
    setTurns([]);
    setShowHistory(false);
    chatReset(id);
  }

  function openConvo(id: string) {
    streamAbort.current?.abort();
    setConvoId(id);
    setTurns(loadTurns(id));
    setShowHistory(false);
  }

  // Estado del modelo: en el 1er arranque se descarga (~9 GB). Mostramos un aviso
  // claro en vez de un error 404. Sondea hasta que esté listo.
  useEffect(() => {
    let alive = true;
    async function check() {
      try {
        const s = await status();
        if (alive) setModelReady(s.model_ready);
      } catch {
        /* núcleo aún arrancando */
      }
      try {
        const p = await providerGet();
        if (alive) setProv(p);
      } catch {
        /* núcleo aún arrancando */
      }
    }
    check();
    const id = setInterval(check, 15000);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  // Bandeja: AION te habla primero. Carga al abrir y sondea cada 30s.
  useEffect(() => {
    let alive = true;
    async function poll() {
      try {
        const r = await inboxList();
        if (!alive) return;
        if (r.unread.length > 0) {
          // Cada mensaje de AION entra como un turno cronológico (al final).
          r.unread.forEach((m) => addReachTurn(m.text, m.kind, m.at));
          inboxRead().catch(() => {});
        }
      } catch {
        /* núcleo aún no disponible: reintenta en el siguiente tick */
      }
    }
    poll();
    const id = setInterval(poll, 30000);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  // Saludo proactivo: AION te habla PRIMERO al abrir (cálido, con continuidad).
  // Se muestra una sola vez por sesión, junto a los "reachouts".
  useEffect(() => {
    if (sessionStorage.getItem("aion_greeted")) return;
    let alive = true;
    getGreeting().then((text) => {
      // Si Ariel saludó primero mientras se generaba, AION ya respondió por el
      // chat: este saludo llegaría tarde y duplicado — se descarta en silencio.
      if (!alive || !text.trim() || userSpokeRef.current) return;
      sessionStorage.setItem("aion_greeted", "1");
      addReachTurn(text, "saludo", new Date().toISOString());
    });
    return () => {
      alive = false;
    };
  }, []);

  // El chat siempre empieza ABAJO (lo más reciente): al cargar la conversación,
  // al cambiar de chat y cuando AION te escribe, baja al final.
  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: "auto" });
  }, [convoId, turns.length]);

  // Lee un archivo como base64 (sin el prefijo data:).
  function readAsBase64(file: File): Promise<string> {
    return new Promise((resolve, reject) => {
      const r = new FileReader();
      r.onload = () => resolve(String(r.result).split(",")[1] ?? "");
      r.onerror = () => reject(new Error("no pude leer el archivo"));
      r.readAsDataURL(file);
    });
  }

  // Maneja un archivo elegido: imagen → visión; documento → biblioteca.
  async function onPickFile(file: File) {
    const b64 = await readAsBase64(file).catch(() => "");
    if (!b64) return;
    if (file.type.startsWith("image/")) {
      // Queda pendiente; se analiza al pulsar Enviar (con tu pregunta opcional).
      setPendingImage({ name: file.name, b64 });
      return;
    }
    // Documento → ingestar en la biblioteca (dominio elegido o "documentos").
    const domain = (window.prompt("¿En qué dominio guardo este documento?", "documentos") || "documentos").trim();
    const turnIdx = turns.length;
    setTurns((t) => [...t, { prompt: `📎 ${file.name}`, mode, thinking: "", steps: [], answer: "📚 Indexando en la biblioteca…" }]);
    try {
      const r = await libraryUpload(domain, file.name, b64);
      setTurns((prev) => prev.map((t, i) => (i === turnIdx
        ? { ...t, answer: `✅ «${r.source}» indexado en «${domain}»: ${r.passages} pasajes. Ya puedo responder sobre su contenido.` }
        : t)));
    } catch (err) {
      setTurns((prev) => prev.map((t, i) => (i === turnIdx
        ? { ...t, answer: `⚠️ ${err instanceof Error ? err.message : "no pude ingerir el documento"}` }
        : t)));
    }
  }

  async function send(e: React.FormEvent) {
    e.preventDefault();
    const prompt = input.trim();
    if (busy) return;
    userSpokeRef.current = true;

    // Si hay una imagen adjunta, se analiza con visión (la pregunta es opcional).
    if (pendingImage) {
      const img = pendingImage;
      setPendingImage(null);
      setInput("");
      setBusy(true);
      const idx = turns.length;
      setTurns((t) => [...t, { prompt: prompt || `🖼️ ${img.name}`, mode, thinking: "", steps: [], answer: "" }]);
      try {
        const answer = await visionAsk(prompt, img.b64);
        setTurns((prev) => prev.map((t, i) => (i === idx ? { ...t, answer } : t)));
      } catch (err) {
        setTurns((prev) => prev.map((t, i) => (i === idx ? { ...t, answer: `⚠️ ${err instanceof Error ? err.message : "error de visión"}` } : t)));
      } finally {
        setBusy(false);
        endRef.current?.scrollIntoView({ behavior: "smooth" });
      }
      return;
    }

    if (!prompt) return;
    if (!modelReady) {
      setTurns((t) => [
        ...t,
        {
          prompt,
          mode,
          thinking: "",
          steps: [],
          answer:
            "Todavía me estoy preparando: descargando el modelo (~9 GB). Espera a la notificación «¡Listo!» y vuelve a intentarlo.",
        },
      ]);
      setInput("");
      return;
    }
    setInput("");
    setBusy(true);
    // Un controller por envío: aborta el anterior si quedó colgando y permite cortar
    // este al desmontar/cambiar de conversación.
    streamAbort.current?.abort();
    const ctrl = new AbortController();
    streamAbort.current = ctrl;
    const idx = turns.length;
    setTurns((t) => [...t, { prompt, mode, thinking: "", steps: [], answer: "" }]);
    const update = (patch: (t: Turn) => Turn) =>
      setTurns((prev) => prev.map((t, i) => (i === idx ? patch(t) : t)));
    const scroll = () => endRef.current?.scrollIntoView({ behavior: "smooth" });

    try {
      if (mode === "chat") {
        await chatStream(prompt, think, (ev: ChatEvent) => {
          /* eventos abajo */
          if (ev.kind === "thinking") update((t) => ({ ...t, thinking: t.thinking + ev.text }));
          else if (ev.kind === "answer") update((t) => ({ ...t, answer: t.answer + ev.text }));
          else if (ev.kind === "done")
            update((t) => ({ ...t, meta: `${ev.tokens} tokens · ${ev.tps.toFixed(1)} tok/s` }));
          else if (ev.kind === "error") update((t) => ({ ...t, answer: `⚠️ ${ev.text}` }));
          scroll();
        }, convoId, undefined, ctrl.signal);
      } else {
        const stream = mode === "crew" ? crewStream : agentStream;
        // CONTEXTO RECIENTE para el agente: los últimos turnos viajan con la tarea.
        // Sin esto, «puedes buscarlo tú» llega huérfano al backend y el modelo
        // alucina el antecedente. Acotado (3 turnos, 280 chars por mensaje) para
        // no inflar el prompt del modelo local.
        const context = turns
          .slice(-3)
          .map((t) =>
            [
              t.prompt ? `Usuario: ${t.prompt.slice(0, 280)}` : "",
              t.answer ? `AION: ${t.answer.slice(0, 280)}` : "",
            ]
              .filter(Boolean)
              .join("\n"),
          )
          .filter(Boolean)
          .join("\n");
        await stream(prompt, (ev: AgentEvent) => {
          if (ev.kind === "thought" || ev.kind === "action" || ev.kind === "observation")
            update((t) => ({
              ...t,
              steps: [...t.steps, { kind: ev.kind, text: ev.text, agent: ev.agent }],
            }));
          else if (ev.kind === "answer")
            update((t) => ({ ...t, answer: ev.text, meta: ev.steps ? `${ev.steps} ${ev.steps === 1 ? "paso" : "pasos"}` : undefined }));
          else if (ev.kind === "confirm") setPendingConfirm({ id: ev.id, text: ev.text });
          else if (ev.kind === "ask") { setPendingAsk({ id: ev.id, text: ev.text }); setAskDraft(""); }
          else if (ev.kind === "error") update((t) => ({ ...t, answer: `⚠️ ${ev.text}` }));
          scroll();
        }, context || undefined, ctrl.signal);
      }
    } catch (err) {
      // Abort por navegación/cambio de conversación: no es un fallo, no lo pintes.
      if (err instanceof DOMException && err.name === "AbortError") return;
      update((t) => ({ ...t, answer: `⚠️ ${err instanceof Error ? err.message : "error"}` }));
    } finally {
      if (streamAbort.current === ctrl) streamAbort.current = null;
      setBusy(false);
    }
  }

  return (
    <AppShell title={t("nav.chat")}>
      <div className="flex flex-col h-full max-w-4xl mx-auto w-full px-6">
      <div className="flex items-center gap-2 py-3 shrink-0">
        {/* Nuevo chat */}
        <button
          onClick={newChat}
          className="icon-chip"
          style={{ background: "var(--surface-2)", color: "var(--text-2)" }}
          title={t("chat.newChat")}
          aria-label={t("chat.newChat")}
        >
          <Icon name="plus" size={16} />
        </button>
        {/* Historial de conversaciones */}
        <div className="relative">
          <button
            onClick={() => { setConvos(loadList()); setShowHistory((s) => !s); }}
            className="icon-chip"
            style={{ background: "var(--surface-2)", color: "var(--text-2)" }}
            title={t("chat.history")}
            aria-label={t("chat.history")}
          >
            <Icon name="clock" size={16} />
          </button>
          {showHistory && (
            <div
              className="absolute left-0 mt-2 z-20 rounded-xl overflow-hidden"
              style={{ width: 280, background: "var(--surface)", border: "1px solid var(--border)", boxShadow: "var(--shadow-elevated)" }}
            >
              <p className="px-3 py-2 text-[10px] font-semibold uppercase tracking-wide" style={{ color: "var(--text-3)" }}>
                {t("chat.history")}
              </p>
              <div className="max-h-72 overflow-y-auto">
                {convos.length === 0 && (
                  <p className="px-3 py-3 text-sm" style={{ color: "var(--text-3)" }}>{t("chat.noHistory")}</p>
                )}
                {convos.map((c) => (
                  <button
                    key={c.id}
                    onClick={() => openConvo(c.id)}
                    className="w-full text-left px-3 py-2 flex items-center gap-2 text-sm hover:opacity-80"
                    style={{ background: c.id === convoId ? "var(--accent-subtle)" : "transparent", color: "var(--text-2)" }}
                  >
                    <Icon name="clock" size={13} className="shrink-0" />
                    <span className="truncate flex-1">{c.title || t("chat.untitled")}</span>
                  </button>
                ))}
              </div>
            </div>
          )}
        </div>
        {busy ? (
          <span className="text-xs" style={{ color: "var(--text-3)" }}>AION trabajando…</span>
        ) : prov ? (
          <div className="flex items-center gap-1.5">
            {prov.can_toggle ? (
              <button
                onClick={toggleEngine}
                disabled={provBusy}
                className="flex items-center gap-1.5 text-xs px-2.5 py-1 rounded-full transition-all"
                style={{ background: "var(--surface-2)", color: "var(--text-2)", opacity: provBusy ? 0.5 : 1 }}
                title={
                  prov.kind === "external"
                    ? `Cambiar al modelo local (${prov.local_model}) · privacidad total, nada sale del Mac`
                    : `Cambiar a la API externa (${prov.ext_model})`
                }
              >
                <span
                  className="inline-block w-2 h-2 rounded-full"
                  style={{ background: prov.kind === "external" ? "#f59e0b" : "var(--accent)" }}
                />
                {provBusy ? "Cambiando…" : `${prov.model} · ${prov.kind === "external" ? "API" : "local"}`}
                <Icon name="refresh" size={12} />
              </button>
            ) : (
              <span className="flex items-center gap-1.5 text-xs" style={{ color: "var(--text-3)" }}>
                <span
                  className="inline-block w-2 h-2 rounded-full"
                  style={{ background: prov.kind === "external" ? "#f59e0b" : "var(--accent)" }}
                />
                {prov.model} · {prov.kind === "external" ? "API" : "local"}
              </span>
            )}
            {prov.kind === "external" && (
              <span
                className="flex items-center cursor-help"
                style={{ color: "#f59e0b" }}
                title="Riesgo de privacidad: con la API externa, lo que escribes a AION se envía a un servidor en la nube. Cambia a un modelo local para que nada salga de tu Mac."
                aria-label="Riesgo de privacidad: la API externa envía tus mensajes a la nube"
              >
                <Icon name="warn" size={14} />
              </span>
            )}
          </div>
        ) : (
          <span className="text-xs" style={{ color: "var(--text-3)" }}>…</span>
        )}
        <div className="ml-auto flex gap-1 p-1 rounded-full" style={{ background: "var(--surface-2)" }}>
          {(["agent", "crew", "chat"] as const).map((m) => (
            <button
              key={m}
              onClick={() => setMode(m)}
              className="text-xs px-3 py-1 rounded-full transition-all"
              style={{
                background: mode === m ? "var(--primary)" : "transparent",
                color: mode === m ? "var(--primary-contrast)" : "var(--text-2)",
              }}
            >
              {m === "chat" ? t("chat.modeChat") : m === "agent" ? t("chat.modeAgent") : t("chat.modeCrew")}
            </button>
          ))}
        </div>
      </div>

      <div className="flex-1 overflow-y-auto py-5 flex flex-col gap-5">
        {!modelReady && (
          <div
            className="card text-sm"
            style={{ borderColor: "var(--accent)", borderWidth: 1, color: "var(--text-2)" }}
          >
<span className="inline-flex items-center gap-1.5"><Icon name="refresh" size={15} /> <strong>Preparando la IA…</strong></span> Estoy descargando el modelo (~9 GB). La
            primera vez tarda unos minutos según tu conexión. En cuanto termine podrás
            chatear (te avisaré con una notificación). Puedes dejar esta ventana abierta.
          </div>
        )}
        {turns.length === 0 && (
          <p className="text-center text-sm mt-20" style={{ color: "var(--text-3)" }}>
            {mode === "chat"
              ? "Chat: AION razona localmente, sin enviar tus datos a nadie."
              : mode === "crew"
                ? "Equipo: un orquestador descompone la tarea y delega en especialistas (investigador, programador, analista, redactor) que colaboran."
                : "Agente: AION usa herramientas (p. ej. calculadora) para resolver tareas."}
          </p>
        )}
        {turns.map((t, i) => (
          <div key={i} className="flex flex-col gap-2">
            {/* Mensaje iniciado por AION: mismo agente, misma voz — se muestra como
                cualquier otra respuesta suya (solo que sin burbuja de usuario). */}
            {t.reach ? (
              <div className="msg max-w-[85%] self-start">
                <Markdown>{t.answer}</Markdown>
              </div>
            ) : (
            <>
            {t.prompt && <div className="self-end msg-user max-w-[75%]">{t.prompt}</div>}

            {t.mode === "chat" && t.thinking && (
              <details className="text-sm" style={{ color: "var(--text-3)" }}>
                <summary className="cursor-pointer select-none" style={{ color: "var(--accent)" }}>
<span className="inline-flex items-center gap-1"><Icon name="brain" size={13} /> razonamiento</span>
                </summary>
                <pre className="whitespace-pre-wrap font-mono text-xs mt-2">{t.thinking}</pre>
              </details>
            )}

            {(t.mode === "agent" || t.mode === "crew") &&
              t.steps.map((s, j) => (
                <div key={j} className="flex items-start gap-2 text-sm pl-1" style={{ color: "var(--text-2)" }}>
                  <span style={{ color: STEP_STYLE[s.kind].color }} className="mt-0.5 shrink-0">
                    <Icon name={STEP_STYLE[s.kind].icon} size={15} />
                  </span>
                  {s.agent && (
                    <span
                      className="text-[10px] px-1.5 py-0.5 rounded-full shrink-0 font-medium"
                      style={{ background: "var(--accent-subtle)", color: "var(--accent)" }}
                    >
                      {s.agent}
                    </span>
                  )}
                  <span className={s.kind === "action" ? "font-mono text-xs" : ""}>{s.text}</span>
                </div>
              ))}

            {t.answer && (
              <div className="msg max-w-[85%] self-start">
                <Markdown>{t.answer}</Markdown>
                {t.meta && (
                  <p className="text-[11px] mt-1.5" style={{ color: "var(--text-3)" }}>
                    {t.meta}
                  </p>
                )}
              </div>
            )}
            </>
            )}
          </div>
        ))}
        <div ref={endRef} />
      </div>

      {pendingConfirm && (
        <div
          className="rounded-xl p-3 mb-1 flex items-center gap-3"
          style={{ background: "var(--accent-subtle)", border: "1px solid var(--accent)" }}
        >
          <Icon name="shield" size={18} />
          <div className="flex-1 min-w-0">
            <p className="text-xs font-semibold" style={{ color: "var(--gold-deep)" }}>
              {t("chat.confirmTitle")}
            </p>
            <p className="text-sm truncate" style={{ color: "var(--text-1)" }}>{pendingConfirm.text}</p>
          </div>
          <button
            className="btn shrink-0"
            onClick={() => { confirmDecision(pendingConfirm.id, true); setPendingConfirm(null); }}
            style={{ background: "var(--accent)", color: "#04201f" }}
          >
            {t("chat.approve")}
          </button>
          <button
            className="btn shrink-0"
            onClick={() => { confirmDecision(pendingConfirm.id, false); setPendingConfirm(null); }}
            style={{ background: "var(--surface-2)", color: "var(--text-2)" }}
          >
            {t("chat.reject")}
          </button>
        </div>
      )}
      {pendingAsk && (
        <form
          className="rounded-xl p-3 mb-1"
          style={{ background: "var(--accent-subtle)", border: "1px solid var(--accent)" }}
          onSubmit={(e) => {
            e.preventDefault();
            if (!askDraft.trim()) return;
            answerQuestion(pendingAsk.id, askDraft.trim());
            setPendingAsk(null);
            setAskDraft("");
          }}
        >
          <div className="flex items-center gap-2 mb-2">
            <Icon name="brain" size={18} />
            <p className="text-sm" style={{ color: "var(--text-1)" }}>{pendingAsk.text}</p>
          </div>
          <div className="flex items-center gap-2">
            <input
              autoFocus
              value={askDraft}
              onChange={(e) => setAskDraft(e.target.value)}
              placeholder={t("chat.askPlaceholder")}
              className="flex-1 px-3 py-2 rounded-lg text-sm outline-none"
              style={{ background: "var(--surface-1)", border: "1px solid var(--border)", color: "var(--text-1)" }}
            />
            <button type="submit" className="btn shrink-0" style={{ background: "var(--accent)", color: "#04201f" }}>
              {t("chat.send")}
            </button>
          </div>
        </form>
      )}
      {pendingImage && (
        <div className="flex items-center gap-2 -mb-1">
          <span className="inline-flex items-center gap-1.5 text-xs px-2.5 py-1.5 rounded-full"
            style={{ background: "var(--accent-subtle)", color: "var(--gold-deep)" }}>
            <Icon name="image" size={14} /> {pendingImage.name}
            <button type="button" onClick={() => setPendingImage(null)} className="ml-1 opacity-70 hover:opacity-100">✕</button>
          </span>
          <span className="text-xs" style={{ color: "var(--text-3)" }}>se analizará al enviar (pregunta opcional)</span>
        </div>
      )}
      <form onSubmit={send} className="py-4 flex gap-2 items-center border-t shrink-0" style={{ borderColor: "var(--border)" }}>
        <input
          ref={fileRef}
          type="file"
          accept=".pdf,.txt,.md,.markdown,image/*"
          className="hidden"
          onChange={(e) => {
            const f = e.target.files?.[0];
            if (f) onPickFile(f);
            e.target.value = "";
          }}
        />
        <button
          type="button"
          onClick={() => fileRef.current?.click()}
          className="shrink-0 rounded-full p-2 transition-colors"
          style={{ color: "var(--text-3)", background: "var(--surface-2)" }}
          title="Adjuntar documento o foto"
          aria-label="Adjuntar documento o foto"
        >
          <Icon name="paperclip" size={18} />
        </button>
        {mode === "chat" && (
          <button
            type="button"
            onClick={() => setThink(!think)}
            className="text-xs px-3 py-2 rounded-full shrink-0"
            style={{
              background: think ? "var(--accent-subtle)" : "var(--surface-2)",
              color: think ? "var(--accent)" : "var(--text-3)",
            }}
            title="Modo razonamiento"
          >
            <span className="inline-flex items-center gap-1"><Icon name="brain" size={14} /> {think ? "on" : "off"}</span>
          </button>
        )}
        <input
          className="input"
          placeholder={mode === "chat" ? t("chat.placeholderChat") : mode === "crew" ? t("chat.placeholderCrew") : t("chat.placeholderAgent")}
          value={input}
          onChange={(e) => setInput(e.target.value)}
        />
        <button className="btn shrink-0" disabled={busy}>
          {busy ? "…" : t("chat.send")}
        </button>
      </form>
      </div>
    </AppShell>
  );
}
