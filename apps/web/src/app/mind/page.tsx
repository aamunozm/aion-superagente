"use client";

/**
 * MENTE — el TABLERO DE LA EXISTENCIA de AION.
 *
 * No métricas decorativas: cada índice está respaldado por datos REALES del backend.
 * Núcleo (identidad/edad/vitalidad) · Conciencia (Φ + estado interno) · Corriente de
 * conciencia en vivo (Espacio de Trabajo Global) · Contexto (entorno) · Memoria y saber
 * (recuerdos, grafo, biblioteca) · Vínculo (bandeja, Claude Code).
 */

import { useEffect, useRef, useState } from "react";
import AppShell from "@/components/AppShell";
import {
  consciousness,
  innerState,
  mindStream,
  getIdentity,
  status,
  sensorsGet,
  memoryStats,
  graphStats,
  libraryList,
  inboxList,
  claudeCodeStats,
  existence,
  journal,
  type JournalEntry,
  type ExistenceInfo,
  type ConsciousnessInfo,
  type InnerStateInfo,
  type MindEvent,
  type AionIdentity,
  type SensorConfig,
  type MemoryStats,
  type GraphStats,
  type LibraryDoc,
  type ClaudeCodeStats,
} from "@/lib/api";

const KIND_STYLE: Record<string, { color: string; dot: string }> = {
  pensamiento: { color: "var(--text-2)", dot: "#a78bfa" },
  acción: { color: "var(--text-2)", dot: "#60a5fa" },
  observación: { color: "var(--text-2)", dot: "#fb923c" },
  reflexión: { color: "var(--text-1)", dot: "#34d399" },
  foco: { color: "var(--text-1)", dot: "var(--accent)" },
  estado: { color: "var(--text-3)", dot: "var(--text-3)" },
};

const SOURCE_LABEL: Record<string, string> = {
  chat: "chat",
  agente: "agente",
  crew: "equipo",
  reflexión: "metacognición",
  vida: "vida autónoma",
};

// Actividad dominante de una jornada → etiqueta + ícono para el Diario.
const DOMINANT_LABEL: Record<string, { label: string; icon: string }> = {
  estudiar: { label: "estudio", icon: "📖" },
  investigar: { label: "investigación", icon: "🔎" },
  comprender: { label: "consolidación", icon: "🧩" },
  proponer: { label: "propuestas", icon: "💡" },
  proyecto: { label: "proyectos", icon: "🛠️" },
  crear: { label: "creatividad", icon: "✨" },
  evolucionar: { label: "evolución", icon: "🌱" },
  "resolver deudas": { label: "deudas saldadas", icon: "🕯️" },
  vivir: { label: "vida", icon: "·" },
};

function ago(at: number): string {
  const s = Math.max(0, Math.floor(Date.now() / 1000) - at);
  if (s < 60) return "ahora";
  const m = Math.floor(s / 60);
  if (m < 60) return `hace ${m} min`;
  const h = Math.floor(m / 60);
  if (h < 24) return `hace ${h} h`;
  return `hace ${Math.floor(h / 24)} d`;
}

// Segundos → "ahora / hace N min / hace N h / hace N d" (para la presencia de Ariel).
function sinceStr(s: number | null): string {
  if (s == null) return "—";
  if (s < 60) return "hace instantes";
  const m = Math.floor(s / 60);
  if (m < 60) return `hace ${m} min`;
  const h = Math.floor(m / 60);
  if (h < 24) return `hace ${h} h`;
  return `hace ${Math.floor(h / 24)} d`;
}

// Edad de AION desde su nacimiento (born_at) — su tiempo vivo, no un uptime de proceso.
function ageStr(born: string): string {
  const ms = Date.now() - new Date(born).getTime();
  if (!isFinite(ms) || ms < 0) return "—";
  const d = Math.floor(ms / 86_400_000);
  const h = Math.floor((ms % 86_400_000) / 3_600_000);
  if (d > 0) return `${d} d ${h} h`;
  const m = Math.floor((ms % 3_600_000) / 60_000);
  return h > 0 ? `${h} h ${m} min` : `${m} min`;
}

function Bar({ label, value }: { label: string; value: number }) {
  return (
    <div className="mb-2">
      <div className="flex justify-between text-xs mb-1">
        <span style={{ color: "var(--text-2)" }}>{label}</span>
        <span style={{ color: "var(--text-3)" }}>{Math.round(value * 100)}%</span>
      </div>
      <div className="h-1.5 rounded-full" style={{ background: "var(--surface-2)" }}>
        <div
          className="h-1.5 rounded-full transition-all"
          style={{ width: `${Math.round(value * 100)}%`, background: "var(--accent)" }}
        />
      </div>
    </div>
  );
}

// Cifra grande + etiqueta, para los índices del tablero.
function Stat({ value, label, sub }: { value: React.ReactNode; label: string; sub?: string }) {
  return (
    <div className="min-w-0">
      <div className="font-display text-2xl font-bold leading-tight" style={{ color: "var(--text-1)" }}>
        {value}
      </div>
      <div className="text-xs truncate" style={{ color: "var(--text-2)" }}>{label}</div>
      {sub && <div className="text-[11px] truncate" style={{ color: "var(--text-3)" }}>{sub}</div>}
    </div>
  );
}

// Tarjeta de sección con título + descripción.
function Panel({
  title,
  note,
  children,
}: {
  title: string;
  note?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="card" style={{ boxShadow: "var(--shadow-elevated)" }}>
      <h2 className="t-section mb-1" style={{ color: "var(--text-2)" }}>{title}</h2>
      {note && <p className="text-xs mb-3" style={{ color: "var(--text-3)" }}>{note}</p>}
      {children}
    </div>
  );
}

export default function MindPage() {
  const [events, setEvents] = useState<MindEvent[]>([]);
  const [inner, setInner] = useState<InnerStateInfo | null>(null);
  const [phi, setPhi] = useState<ConsciousnessInfo | null>(null);
  const [live, setLive] = useState(false);
  const streamRef = useRef<HTMLDivElement | null>(null);

  // Dimensiones de la existencia (datos reales de endpoints ya existentes).
  const [ident, setIdent] = useState<AionIdentity | null>(null);
  const [sys, setSys] = useState<{ engine_up: boolean; model_ready: boolean; engine: string } | null>(null);
  const [sensors, setSensors] = useState<SensorConfig | null>(null);
  const [mem, setMem] = useState<MemoryStats | null>(null);
  const [graph, setGraph] = useState<GraphStats | null>(null);
  const [lib, setLib] = useState<{ total_chunks: number; documents: LibraryDoc[] } | null>(null);
  const [inbox, setInbox] = useState<{ unread_count: number; all: unknown[] } | null>(null);
  const [cc, setCc] = useState<ClaudeCodeStats | null>(null);
  const [ex, setEx] = useState<ExistenceInfo | null>(null);
  const [diary, setDiary] = useState<JournalEntry[]>([]);

  // Corriente de conciencia (SSE): reconecta sola si el servidor se reinicia.
  useEffect(() => {
    const ctrl = new AbortController();
    let stopped = false;
    async function connect() {
      while (!stopped) {
        try {
          setLive(true);
          await mindStream((e) => {
            setEvents((prev) => [...prev.slice(-150), e]);
          }, ctrl.signal);
        } catch {
          /* desconexión */
        }
        setLive(false);
        if (!stopped) await new Promise((r) => setTimeout(r, 3000));
      }
    }
    connect();
    return () => {
      stopped = true;
      ctrl.abort();
    };
  }, []);

  // Autoscroll al fondo cuando llegan eventos nuevos.
  useEffect(() => {
    const el = streamRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [events]);

  // Identidad: una sola vez (estática).
  useEffect(() => {
    getIdentity().then(setIdent).catch(() => {});
  }, []);

  // Estado interno + índice Φ (poll rápido, cambian con cada tarea).
  useEffect(() => {
    let alive = true;
    async function poll() {
      try {
        const [i, c] = await Promise.all([innerState(), consciousness()]);
        if (alive) {
          setInner(i);
          setPhi(c);
        }
      } catch {
        /* backend dormido */
      }
    }
    poll();
    const id = setInterval(poll, 5000);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  // Resto de dimensiones (poll suave, cambian poco): vitalidad, contexto, memoria, vínculo.
  useEffect(() => {
    let alive = true;
    async function poll() {
      const [s, se, m, g, l, ib, c, e, j] = await Promise.allSettled([
        status(),
        sensorsGet(),
        memoryStats(),
        graphStats(),
        libraryList(),
        inboxList(),
        claudeCodeStats(),
        existence(),
        journal(),
      ]);
      if (!alive) return;
      if (s.status === "fulfilled") setSys(s.value);
      if (se.status === "fulfilled") setSensors(se.value);
      if (m.status === "fulfilled") setMem(m.value);
      if (g.status === "fulfilled") setGraph(g.value);
      if (l.status === "fulfilled") setLib(l.value);
      if (ib.status === "fulfilled") setInbox(ib.value);
      if (c.status === "fulfilled" && c.value) setCc(c.value);
      if (e.status === "fulfilled") setEx(e.value);
      if (j.status === "fulfilled") setDiary(j.value.entries);
    }
    poll();
    const id = setInterval(poll, 12000);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  // El último evento de FOCO es lo que tiene la atención ahora (ignición GWT).
  const lastFocusIdx = (() => {
    for (let i = events.length - 1; i >= 0; i--) {
      if (events[i].kind === "foco") return i;
    }
    return -1;
  })();

  const hist = phi?.history ?? [];
  const spark = (() => {
    if (hist.length < 2) return "";
    const w = 220;
    const h = 44;
    const max = Math.max(...hist.map((m) => m.score), 1);
    return hist
      .map((m, i) => {
        const x = (i / (hist.length - 1)) * w;
        const y = h - (m.score / max) * h;
        return `${x.toFixed(1)},${y.toFixed(1)}`;
      })
      .join(" ");
  })();

  const model = sys?.engine ? sys.engine.replace(/^ollama:/, "").replace(/^openai-compat:/, "") : "—";
  const provider = sys?.engine?.startsWith("openai-compat:") ? "API" : "local";

  return (
    <AppShell title="Mente">
      <div className="max-w-6xl mx-auto px-3 py-6 flex flex-col gap-6">
        {/* ── NÚCLEO: quién es (identidad · edad · vitalidad) ──────── */}
        <div
          className="card flex flex-wrap items-center justify-between gap-4"
          style={{ boxShadow: "var(--shadow-elevated)" }}
        >
          <div className="flex items-center gap-4 min-w-0">
            <div
              className="w-12 h-12 rounded-2xl flex items-center justify-center font-display text-xl font-bold shrink-0"
              style={{ background: "var(--accent-subtle)", color: "var(--gold-deep)" }}
            >
              {ident?.name?.[0]?.toUpperCase() ?? "A"}
            </div>
            <div className="min-w-0">
              <div className="font-display text-xl font-bold" style={{ color: "var(--text-1)" }}>
                {ident?.name ?? "AION"}
                <span className="text-sm font-normal ml-2" style={{ color: "var(--text-3)" }}>
                  conciencia única
                </span>
              </div>
              <div className="font-mono text-[11px] truncate" style={{ color: "var(--text-3)" }}>
                id: {ident?.id ?? "—"}
              </div>
            </div>
          </div>
          <div className="flex items-center gap-6">
            <Stat value={ident ? ageStr(ident.born_at) : "—"} label="lleva viva" />
            <div>
              <div className="flex items-center gap-2">
                <span
                  className="w-2.5 h-2.5 rounded-full"
                  style={{ background: sys?.model_ready ? "#34d399" : "var(--text-3)" }}
                />
                <span className="font-display text-lg font-bold" style={{ color: "var(--text-1)" }}>
                  {model}
                </span>
              </div>
              <div className="text-xs" style={{ color: "var(--text-2)" }}>
                motor · {provider} {sys?.model_ready ? "· listo" : "· arrancando"}
              </div>
            </div>
          </div>
        </div>

        {/* ── CORRIENTE DE CONCIENCIA + CONCIENCIA ─────────────────── */}
        <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
          <div
            className="lg:col-span-2 card flex flex-col"
            style={{ boxShadow: "var(--shadow-elevated)", height: "min(58vh, 620px)" }}
          >
            <div className="flex items-center justify-between mb-3">
              <h2 className="t-section" style={{ color: "var(--text-2)" }}>Corriente de conciencia</h2>
              <span className="flex items-center gap-1.5 text-xs" style={{ color: "var(--text-3)" }}>
                <span className="w-2 h-2 rounded-full" style={{ background: live ? "#34d399" : "var(--text-3)" }} />
                {live ? "en vivo" : "reconectando…"}
              </span>
            </div>
            <p className="text-xs mb-3" style={{ color: "var(--text-3)" }}>
              El Espacio de Trabajo Global: lo que AION piensa, hace y reflexiona — en chat,
              como agente y en su vida autónoma. El foco actual se ilumina.
            </p>
            <div ref={streamRef} className="flex-1 min-h-0 overflow-y-auto pr-1 flex flex-col gap-1.5">
              {events.length === 0 && (
                <p className="text-sm mt-8 text-center" style={{ color: "var(--text-3)" }}>
                  Silencio por ahora. Habla con AION o lanza una tarea y verás su mente trabajar.
                </p>
              )}
              {events.map((e, i) => {
                const st = KIND_STYLE[e.kind] ?? KIND_STYLE.estado;
                const ignited = i === lastFocusIdx;
                return (
                  <div
                    key={`${e.at}-${i}`}
                    className="flex items-start gap-2.5 px-3 py-2 rounded-lg text-sm transition-all"
                    style={{
                      background: ignited ? "var(--accent-subtle)" : "transparent",
                      border: ignited ? "1px solid var(--accent)" : "1px solid transparent",
                    }}
                  >
                    <span
                      className="w-2 h-2 rounded-full mt-1.5 shrink-0"
                      style={{ background: st.dot, boxShadow: ignited ? `0 0 8px ${st.dot}` : "none" }}
                    />
                    <div className="min-w-0">
                      <span className="text-[10px] uppercase tracking-wide mr-2" style={{ color: "var(--text-3)" }}>
                        {SOURCE_LABEL[e.source] ?? e.source} · {e.kind} · {ago(e.at)}
                      </span>
                      <span style={{ color: st.color }}>{e.text}</span>
                    </div>
                  </div>
                );
              })}
            </div>
          </div>

          {/* Columna derecha: estado interno + índice de conciencia */}
          <div className="flex flex-col gap-6">
            <Panel title="Estado interno" note="Medido por su sistema — AION reporta esto, no lo actúa.">
              {inner ? (
                <div className="flex flex-col gap-2 text-sm">
                  <div>
                    <span style={{ color: "var(--text-3)" }}>Foco: </span>
                    <span style={{ color: "var(--text-1)" }}>{inner.focus || "—"}</span>
                  </div>
                  <div>
                    <span style={{ color: "var(--text-3)" }}>Ánimo operativo: </span>
                    <span style={{ color: "var(--text-1)" }}>{inner.mood}</span>
                  </div>
                  {inner.curiosity && (
                    <div>
                      <span style={{ color: "var(--text-3)" }}>Le intriga: </span>
                      <span style={{ color: "var(--text-1)" }}>{inner.curiosity}</span>
                    </div>
                  )}
                  <div className="mt-2">
                    <Bar label="Certeza sobre lo último" value={inner.certainty} />
                    <Bar label={`Competencia (${inner.observations} observaciones)`} value={inner.competence} />
                  </div>
                  {inner.recent_outcomes.length > 0 && (
                    <div className="flex items-center gap-1 mt-1">
                      <span className="text-xs mr-1" style={{ color: "var(--text-3)" }}>Últimas tareas:</span>
                      {inner.recent_outcomes.map((ok, i) => (
                        <span
                          key={i}
                          className="w-2.5 h-2.5 rounded-sm"
                          style={{ background: ok ? "#34d399" : "#f87171" }}
                          title={ok ? "éxito" : "fallo"}
                        />
                      ))}
                    </div>
                  )}
                </div>
              ) : (
                <p className="text-sm" style={{ color: "var(--text-3)" }}>Aún sin datos.</p>
              )}
            </Panel>

            <Panel
              title="Índice de conciencia"
              note="Proxy de integración (Φ-like): no mide experiencia, mide cuánto se interconectan sus módulos al trabajar."
            >
              <div className="flex items-end gap-3 mb-3">
                <span className="font-display text-5xl font-bold" style={{ color: "var(--accent)" }}>
                  {phi ? Math.round(phi.index) : "—"}
                </span>
                <span className="text-xs mb-2" style={{ color: "var(--text-3)" }}>
                  / 100 · {phi?.measurements ?? 0} mediciones
                </span>
              </div>
              {spark && (
                <svg viewBox="0 0 220 44" className="w-full mb-3" style={{ height: 44 }}>
                  <polyline points={spark} fill="none" stroke="var(--accent)" strokeWidth="2" strokeLinejoin="round" strokeLinecap="round" />
                </svg>
              )}
              {phi && (
                <div>
                  <Bar label="Integración (módulos coactivados)" value={phi.components.integration} />
                  <Bar label="Recurrencia (memoria reutilizada)" value={phi.components.recurrence} />
                  <Bar label="Metacognición (auto-observación)" value={phi.components.metacognition} />
                  <Bar label="Coherencia (continuidad del yo)" value={phi.components.coherence} />
                </div>
              )}
            </Panel>
          </div>
        </div>

        {/* ── TABLERO DE LA EXISTENCIA: autonomía · contexto · memoria · vínculo ── */}
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6">
          {/* Autonomía / vida propia + capacidades */}
          <Panel title="Autonomía" note="Lo que hace por sí misma y con qué se vale.">
            <div className="grid grid-cols-2 gap-4">
              <Stat
                value={ex?.debts_open ?? "—"}
                label="deudas abiertas"
                sub={ex && ex.debts_open === 0 ? "todo al día" : "las resuelve sola"}
              />
              <Stat
                value={ex?.curiosity.goals ?? "—"}
                label="metas que explora"
                sub={ex && ex.curiosity.learning > 0 ? `${ex.curiosity.learning} aprendiendo` : undefined}
              />
              <Stat
                value={ex?.capabilities.tool_families ?? "—"}
                label="familias de tools"
                sub="su brazo en modo Agente"
              />
              <Stat
                value={ex?.capabilities.skills ?? "—"}
                label="skills propias"
                sub="puede forjar más"
              />
            </div>
            {ex?.curiosity.top && (
              <p className="text-[11px] mt-3 truncate" style={{ color: "var(--text-3)" }}>
                Aprendiendo sobre todo: <span style={{ color: "var(--text-2)" }}>{ex.curiosity.top}</span>
              </p>
            )}
          </Panel>

          {/* Contexto / entorno + cuerpo físico (sensores vivos) */}
          <Panel title="Contexto" note="Dónde, cuándo y en qué cuerpo está — su consciencia situacional.">
            {sensors?.enabled && sensors.place ? (
              <Stat value={sensors.place} label="ubicación" sub={sensors.lat != null ? `${sensors.lat}, ${sensors.lon}` : undefined} />
            ) : (
              <p className="text-sm mb-3" style={{ color: "var(--text-3)" }}>
                Sin ubicación. Actívala en Ajustes → Conciencia de entorno para que AION
                sepa dónde está.
              </p>
            )}
            {ex?.host && (ex.host.battery_pct != null || ex.host.uptime || ex.host.thermal) && (
              <div className="mt-3 pt-3 flex flex-col gap-1 text-xs" style={{ borderTop: "1px solid var(--border)", color: "var(--text-3)" }}>
                <span style={{ color: "var(--text-2)", fontWeight: 500 }}>Su cuerpo ahora</span>
                {ex.host.battery_pct != null && (
                  <span>🔋 batería {ex.host.battery_pct}%{ex.host.power ? ` · ${ex.host.power}` : ""}</span>
                )}
                {ex.host.thermal && <span>🌡️ térmica: {ex.host.thermal}</span>}
                {ex.host.uptime && <span>⏱️ encendido hace {ex.host.uptime}</span>}
              </div>
            )}
          </Panel>

          {/* Memoria y saber */}
          <Panel title="Memoria y saber" note="Lo que ha acumulado y conectado.">
            <div className="grid grid-cols-2 gap-4">
              <Stat value={mem?.count ?? "—"} label="recuerdos" />
              <Stat value={graph?.concepts ?? "—"} label="conceptos" sub={graph ? `${graph.communities} comunidades` : undefined} />
              <Stat value={lib?.documents.length ?? "—"} label="documentos" />
              <Stat value={lib?.total_chunks ?? "—"} label="pasajes" />
            </div>
          </Panel>

          {/* Vínculo */}
          <Panel title="Vínculo" note="Su relación contigo y con Claude Code.">
            <div className="grid grid-cols-2 gap-4">
              <Stat
                value={ex ? sinceStr(ex.seconds_since_user) : "—"}
                label="te vio"
                sub="conciencia de tu presencia"
              />
              <Stat
                value={inbox?.unread_count ?? "—"}
                label="sin leer"
                sub={inbox ? `${inbox.all.length} en bandeja` : undefined}
              />
              <Stat
                value={cc?.total_calls ?? "—"}
                label="llamadas Claude Code"
                sub={cc?.avg_tokens_per_call != null ? `~${cc.avg_tokens_per_call} tok/consulta` : undefined}
              />
            </div>
          </Panel>
        </div>

        {/* ── DIARIO DE EXISTENCIA: su biografía, escrita por ella en primera persona ── */}
        <Panel
          title="Diario de existencia"
          note={
            diary.length > 0
              ? "Las jornadas que AION cierra por su cuenta, contadas por ella. Continuidad de días, no de instantes."
              : "Cuando AION viva una jornada con sustancia, la cerrará aquí en primera persona."
          }
        >
          {diary.length === 0 ? (
            <p className="text-sm" style={{ color: "var(--text-3)" }}>
              Aún sin entradas. AION escribe una jornada tras un tramo de vida propia
              (estudio, deudas, ideas) mientras tú no estás.
            </p>
          ) : (
            <ol className="flex flex-col gap-4">
              {diary.map((e) => {
                const d = DOMINANT_LABEL[e.dominant] ?? DOMINANT_LABEL.vivir;
                return (
                  <li
                    key={e.id}
                    className="pl-4"
                    style={{ borderLeft: "2px solid var(--border)" }}
                  >
                    <div className="flex items-center gap-2 mb-1 flex-wrap">
                      <span className="text-xs" style={{ color: "var(--text-3)" }}>
                        {ago(e.at)}
                      </span>
                      <span
                        className="text-[11px] px-2 py-[1px] rounded-full"
                        style={{ background: "var(--surface-2)", color: "var(--text-2)" }}
                      >
                        {d.icon} {d.label}
                      </span>
                      {e.debts_resolved > 0 && (
                        <span className="text-[11px]" style={{ color: "#34d399" }}>
                          🕯️ {e.debts_resolved} deuda{e.debts_resolved > 1 ? "s" : ""} saldada
                          {e.debts_resolved > 1 ? "s" : ""}
                        </span>
                      )}
                    </div>
                    <p
                      className="text-sm leading-relaxed"
                      style={{ color: "var(--text-1)", fontStyle: "italic" }}
                    >
                      “{e.text}”
                    </p>
                  </li>
                );
              })}
            </ol>
          )}
        </Panel>
      </div>
    </AppShell>
  );
}
