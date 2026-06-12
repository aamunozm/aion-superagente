"use client";

/**
 * MENTE — la conciencia de AION, observable.
 *
 * Tres zonas: la CORRIENTE DE CONCIENCIA en vivo (el Espacio de Trabajo Global:
 * pensamientos, acciones, reflexiones y vida autónoma compitiendo por la atención;
 * el foco actual se "ilumina" — ignición GWT), su ESTADO INTERNO real (self-model
 * medido: foco, ánimo operativo, certeza, curiosidad) y el ÍNDICE DE CONCIENCIA
 * (proxy Φ-like de integración, con sus componentes y su historia).
 */

import { useEffect, useRef, useState } from "react";
import AppShell from "@/components/AppShell";
import {
  consciousness,
  innerState,
  mindStream,
  type ConsciousnessInfo,
  type InnerStateInfo,
  type MindEvent,
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

function ago(at: number): string {
  const s = Math.max(0, Math.floor(Date.now() / 1000) - at);
  if (s < 60) return "ahora";
  const m = Math.floor(s / 60);
  if (m < 60) return `hace ${m} min`;
  const h = Math.floor(m / 60);
  if (h < 24) return `hace ${h} h`;
  return `hace ${Math.floor(h / 24)} d`;
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

export default function MindPage() {
  const [events, setEvents] = useState<MindEvent[]>([]);
  const [inner, setInner] = useState<InnerStateInfo | null>(null);
  const [phi, setPhi] = useState<ConsciousnessInfo | null>(null);
  const [live, setLive] = useState(false);
  const streamRef = useRef<HTMLDivElement | null>(null);

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

  // Estado interno + índice (poll suave).
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

  return (
    <AppShell title="Mente">
      <div className="max-w-6xl mx-auto px-8 py-8 grid grid-cols-1 lg:grid-cols-3 gap-6">
        {/* ── CORRIENTE DE CONCIENCIA ─────────────────────────── */}
        <div className="lg:col-span-2 card flex flex-col" style={{ boxShadow: "var(--shadow-elevated)", height: "calc(100vh - 160px)" }}>
          <div className="flex items-center justify-between mb-3">
            <h2 className="t-section" style={{ color: "var(--text-2)" }}>
              Corriente de conciencia
            </h2>
            <span className="flex items-center gap-1.5 text-xs" style={{ color: "var(--text-3)" }}>
              <span
                className="w-2 h-2 rounded-full"
                style={{ background: live ? "#34d399" : "var(--text-3)" }}
              />
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

        {/* ── COLUMNA DERECHA ─────────────────────────────────── */}
        <div className="flex flex-col gap-6">
          {/* Estado interno (self-model medido) */}
          <div className="card" style={{ boxShadow: "var(--shadow-elevated)" }}>
            <h2 className="t-section mb-1" style={{ color: "var(--text-2)" }}>
              Estado interno
            </h2>
            <p className="text-xs mb-3" style={{ color: "var(--text-3)" }}>
              Medido por su sistema — AION reporta esto, no lo actúa.
            </p>
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
                    <span className="text-xs mr-1" style={{ color: "var(--text-3)" }}>
                      Últimas tareas:
                    </span>
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
              <p className="text-sm" style={{ color: "var(--text-3)" }}>
                Aún sin datos.
              </p>
            )}
          </div>

          {/* Índice de conciencia (Φ-like) */}
          <div className="card" style={{ boxShadow: "var(--shadow-elevated)" }}>
            <h2 className="t-section mb-1" style={{ color: "var(--text-2)" }}>
              Índice de conciencia
            </h2>
            <p className="text-xs mb-3" style={{ color: "var(--text-3)" }}>
              Proxy de integración (Φ-like): no mide experiencia, mide cuánto se
              interconectan sus módulos al trabajar.
            </p>
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
                <polyline
                  points={spark}
                  fill="none"
                  stroke="var(--accent)"
                  strokeWidth="2"
                  strokeLinejoin="round"
                  strokeLinecap="round"
                />
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
          </div>
        </div>
      </div>
    </AppShell>
  );
}
