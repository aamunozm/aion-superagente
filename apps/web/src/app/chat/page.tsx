"use client";

import { useEffect, useRef, useState } from "react";
import AppShell from "@/components/AppShell";
import Icon from "@/components/Icon";
import { useT } from "@/lib/i18n";
import {
  agentStream,
  crewStream,
  chatStream,
  chatReset,
  confirmDecision,
  inboxList,
  inboxRead,
  libraryUpload,
  visionAsk,
  status,
  type AgentEvent,
  type ChatEvent,
  type InboxMessage,
} from "@/lib/api";

const INBOX_ICON: Record<string, React.ComponentProps<typeof Icon>["name"]> = {
  insight: "bulb",
  idea: "sparkle",
  pregunta: "help",
  saludo: "wave",
  alerta: "warn",
};

type Step = { kind: "thought" | "action" | "observation"; text: string; agent?: string };
type Mode = "chat" | "agent" | "crew";
type Turn = {
  prompt: string;
  mode: Mode;
  thinking: string;
  steps: Step[];
  answer: string;
  meta?: string;
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
  const [reachouts, setReachouts] = useState<InboxMessage[]>([]);
  const [modelReady, setModelReady] = useState(true);
  // Conversaciones persistentes: id actual + lista + dropdown de historial.
  const [convoId, setConvoId] = useState<string>("");
  const [convos, setConvos] = useState<ConvoMeta[]>([]);
  const [showHistory, setShowHistory] = useState(false);
  // Confirmación humana pendiente (login, compra…): se muestra una tarjeta con OK/No.
  const [pendingConfirm, setPendingConfirm] = useState<{ id: string; text: string } | null>(null);
  // Adjunto de imagen pendiente (se envía con el siguiente mensaje, vía visión).
  const [pendingImage, setPendingImage] = useState<{ name: string; b64: string } | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const endRef = useRef<HTMLDivElement>(null);

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
    const id = newId();
    setConvoId(id);
    setTurns([]);
    setShowHistory(false);
    chatReset(id);
  }

  function openConvo(id: string) {
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
          setReachouts((prev) => {
            const seen = new Set(prev.map((m) => m.id));
            const fresh = r.unread.filter((m) => !seen.has(m.id));
            if (fresh.length) inboxRead().catch(() => {});
            return fresh.length ? [...prev, ...fresh] : prev;
          });
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
        }, convoId);
      } else {
        const stream = mode === "crew" ? crewStream : agentStream;
        await stream(prompt, (ev: AgentEvent) => {
          if (ev.kind === "thought" || ev.kind === "action" || ev.kind === "observation")
            update((t) => ({
              ...t,
              steps: [...t.steps, { kind: ev.kind, text: ev.text, agent: ev.agent }],
            }));
          else if (ev.kind === "answer")
            update((t) => ({ ...t, answer: ev.text, meta: ev.steps ? `${ev.steps} ${ev.steps === 1 ? "paso" : "pasos"}` : undefined }));
          else if (ev.kind === "confirm") setPendingConfirm({ id: ev.id, text: ev.text });
          else if (ev.kind === "error") update((t) => ({ ...t, answer: `⚠️ ${ev.text}` }));
          scroll();
        });
      }
    } catch (err) {
      update((t) => ({ ...t, answer: `⚠️ ${err instanceof Error ? err.message : "error"}` }));
    } finally {
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
        <span className="text-xs" style={{ color: "var(--text-3)" }}>
          {busy ? "AION trabajando…" : "gemma4-reason · local"}
        </span>
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
        {reachouts.length > 0 && (
          <div className="flex flex-col gap-2">
            <p className="text-xs font-medium" style={{ color: "var(--accent)" }}>
<span className="inline-flex items-center gap-1.5"><Icon name="sparkle" size={13} /> AION te escribió mientras no estabas</span>
            </p>
            {reachouts.map((m) => (
              <div
                key={m.id}
                className="msg max-w-[85%] self-start"
                style={{ borderColor: "var(--accent)" }}
              >
                <p className="text-xs mb-1" style={{ color: "var(--accent)" }}>
                  <span className="inline-flex items-center gap-1"><Icon name={INBOX_ICON[m.kind] ?? "sparkle"} size={12} /> {m.kind}</span> ·{" "}
                  {new Date(m.at).toLocaleString()}
                </p>
                <p className="whitespace-pre-wrap">{m.text}</p>
              </div>
            ))}
          </div>
        )}
        {turns.length === 0 && reachouts.length === 0 && (
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
            <div className="self-end msg-user max-w-[75%]">{t.prompt}</div>

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
                <p className="whitespace-pre-wrap">{t.answer}</p>
                {t.meta && (
                  <p className="text-[11px] mt-1.5" style={{ color: "var(--text-3)" }}>
                    {t.meta}
                  </p>
                )}
              </div>
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
      <form onSubmit={send} className="py-4 flex gap-2 items-center border-t" style={{ borderColor: "var(--border)" }}>
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
