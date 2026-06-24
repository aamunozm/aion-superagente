"use client";

import { useCallback, useEffect, useState } from "react";
import AppShell from "@/components/AppShell";
import Icon from "@/components/Icon";
import { useT } from "@/lib/i18n";
import {
  backupMerge,
  claudeCodeAudit,
  claudeCodeConnect,
  claudeCodeDisconnect,
  claudeCodeGet,
  claudeCodeSet,
  claudeCodeStats,
  claudeCodeTest,
  downloadMemory,
  forgetProject,
  importMemory,
  claudeCodeCost,
  memoryNormalize,
  memoryProjects,
  vaultGet,
  vaultList,
  vaultRemove,
  vaultSet,
  type ClaudeCodeAuditEntry,
  type ClaudeCodeStats,
  type ClaudeCodeStatus,
  type CostData,
  type MemoryProjects,
  type ProjectMemory,
  type VaultSecret,
} from "@/lib/api";

const INSTALL_CMD = "npm install -g @anthropic-ai/claude-code";

const TOOLS: { name: string; key: string }[] = [
  { name: "aion_memory_search", key: "cc.tool.memory" },
  { name: "aion_library_search", key: "cc.tool.library" },
  { name: "aion_graph_query", key: "cc.tool.graph" },
  { name: "aion_project_context", key: "cc.tool.projects" },
  { name: "aion_remember", key: "cc.tool.remember" },
  { name: "aion_brief", key: "cc.tool.brief" },
];

function toolLabel(tool: string): string {
  return tool.replace(/^aion_/, "").replace(/_/g, " ");
}

function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
  return String(n);
}

/** Barra de progreso horizontal reutilizable */
function Bar({ value, max, color = "var(--accent)" }: { value: number; max: number; color?: string }) {
  const pct = max > 0 ? Math.max(4, (value / max) * 100) : 4;
  return (
    <div className="flex-1 h-1.5 rounded-full overflow-hidden" style={{ background: "var(--surface-2)" }}>
      <div className="h-full rounded-full transition-all" style={{ width: `${pct}%`, background: color }} />
    </div>
  );
}

function fmtBytes(n: number): string {
  if (n >= 1_048_576) return `${(n / 1_048_576).toFixed(1)} MB`;
  if (n >= 1024) return `${(n / 1024).toFixed(0)} KB`;
  return `${n} B`;
}

/** Abre el selector de archivos y devuelve el texto del archivo elegido (o null). */
function pickFileText(): Promise<string | null> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".jsonl,application/x-ndjson,text/plain";
    input.onchange = () => {
      const f = input.files?.[0];
      if (!f) return resolve(null);
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result ?? ""));
      reader.onerror = () => resolve(null);
      reader.readAsText(f);
    };
    input.click();
  });
}

function downloadText(text: string, filename: string) {
  const blob = new Blob([text], { type: "application/x-ndjson" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}

/** Panel de gestión de memoria POR PROYECTO: medidor de uso, ahorro acumulado, y backup /
 *  actualizar backup / descargar+liberar (con confirmación) / restaurar. Reúne todo lo que
 *  pidió Ariel: medir por proyecto, hacer backup para liberar espacio sin saturar AION, y
 *  restaurar cuando quiera reactivarlo — para cualquier proyecto, viejo o nuevo. */
function MemoryByProject() {
  const [data, setData] = useState<MemoryProjects | null>(null);
  const [busy, setBusy] = useState("");
  const [note, setNote] = useState("");
  const [confirmDel, setConfirmDel] = useState<{ project: string; count: number } | null>(null);

  const load = useCallback(() => {
    memoryProjects().then(setData).catch(() => {});
  }, []);
  useEffect(() => {
    load();
    const iv = setInterval(load, 20_000);
    return () => clearInterval(iv);
  }, [load]);

  const projects = data?.projects ?? [];
  const maxBytes = Math.max(1, ...projects.map((p) => p.bytes));

  async function download(p: ProjectMemory) {
    setBusy(p.project);
    const ok = await downloadMemory(p.project);
    setNote(ok ? `✓ Backup de «${p.project}» descargado (${p.count} recuerdos)` : "⚠️ No se pudo exportar");
    setBusy("");
  }

  async function updateBackup(p: ProjectMemory) {
    const existing = await pickFileText();
    if (existing == null) return;
    setBusy(p.project);
    const r = await backupMerge(p.project, existing).catch(() => null);
    if (r?.ok && r.jsonl != null) {
      downloadText(r.jsonl, `aion-memory-${p.project}.jsonl`);
      setNote(`✓ Backup actualizado: ${r.total} recuerdos (${r.from_current} actuales + ${r.from_backup} solo en el backup)`);
    } else {
      setNote("⚠️ No se pudo actualizar el backup");
    }
    setBusy("");
  }

  async function askFree(p: ProjectMemory) {
    setBusy(p.project);
    const r = await forgetProject(p.project, false).catch(() => null);
    setBusy("");
    if (r && typeof r.would_remove === "number") {
      setConfirmDel({ project: p.project, count: r.would_remove });
    } else {
      setNote("⚠️ No se pudo consultar");
    }
  }

  async function confirmFree() {
    if (!confirmDel) return;
    const { project } = confirmDel;
    setBusy(project);
    // Seguridad: SIEMPRE descarga el backup antes de borrar.
    const backed = await downloadMemory(project);
    if (!backed) {
      setNote("⚠️ Aborté: no pude descargar el backup antes de borrar");
      setBusy("");
      setConfirmDel(null);
      return;
    }
    const r = await forgetProject(project, true).catch(() => null);
    setNote(r?.ok ? `✓ Liberados ${r.removed} recuerdos de «${project}» (backup descargado primero)` : "⚠️ No se pudo borrar");
    setConfirmDel(null);
    setBusy("");
    load();
  }

  async function restore() {
    const jsonl = await pickFileText();
    if (jsonl == null) return;
    setBusy("__restore");
    const r = await importMemory(jsonl).catch(() => null);
    setNote(r?.ok ? `✓ Restaurados ${r.added} recuerdos (total: ${r.count})` : "⚠️ No se pudo restaurar");
    setBusy("");
    load();
  }

  async function normalize() {
    setBusy("__normalize");
    const r = await memoryNormalize().catch(() => null);
    setNote(r?.ok ? `✓ Etiquetas normalizadas: ${r.rewritten} reescritas de ${r.scanned}` : "⚠️ No se pudo normalizar");
    setBusy("");
    load();
  }

  return (
    <div className="card">
      <div className="flex items-center justify-between mb-1">
        <h2 className="t-section" style={{ color: "var(--text-2)" }}>Memoria por proyecto</h2>
        <div className="flex gap-2">
          <button className="btn btn-ghost text-xs" disabled={!!busy} onClick={restore} title="Restaurar/fusionar memoria desde un .jsonl">
            ⬆︎ Restaurar
          </button>
          <button className="btn btn-ghost text-xs" disabled={!!busy} onClick={normalize} title="Unifica etiquetas (AION/aion, Peace Harmony AFC/peace-harmony)">
            Normalizar
          </button>
        </div>
      </div>
      <p className="text-xs mb-4 leading-relaxed" style={{ color: "var(--text-3)" }}>
        Mide cuánta memoria ocupa cada proyecto y su ahorro de tokens. Haz backup para liberar
        espacio sin saturar AION; restaura cuando quieras reactivarlo. El backup incluye todas
        las ramas del proyecto.
      </p>

      {data && (
        <div className="flex flex-wrap gap-4 mb-4 text-xs" style={{ color: "var(--text-3)" }}>
          <span>Total: <b style={{ color: "var(--text-2)" }}>{data.total_count}</b> recuerdos · {fmtBytes(data.total_bytes)}</span>
          <span>Atribuible a proyectos: <b style={{ color: "var(--text-2)" }}>{fmtBytes(data.tagged_bytes)}</b></span>
          <span>Propia de AION (sin proyecto): {fmtBytes(data.untagged_bytes)}</span>
        </div>
      )}

      {note && (
        <p className="text-xs mb-3 px-3 py-2 rounded-lg" style={{ background: "var(--surface-1)", color: "var(--text-2)" }}>{note}</p>
      )}

      {projects.length === 0 ? (
        <p className="text-sm" style={{ color: "var(--text-3)" }}>
          Aún no hay recuerdos etiquetados por proyecto. Cuando Claude Code use{" "}
          <code className="font-mono text-xs">aion_remember</code> con un proyecto, aparecerán aquí.
        </p>
      ) : (
        <div className="flex flex-col gap-3">
          {projects.map((p) => (
            <div key={p.project} className="rounded-lg p-3" style={{ background: "var(--surface-1)", border: "1px solid var(--border)" }}>
              <div className="flex items-center justify-between gap-2 mb-2">
                <div className="min-w-0">
                  <span className="font-mono text-sm font-medium" style={{ color: "var(--text-1)" }}>{p.project}</span>
                  <span className="text-xs ml-2" style={{ color: "var(--text-3)" }}>
                    {p.count} recuerdos · {fmtBytes(p.bytes)} · {p.pct}% del total
                  </span>
                </div>
                {p.calls > 0 && (
                  <span className="text-xs whitespace-nowrap" style={{ color: "var(--text-3)" }} title="Consultas del puente MCP a este proyecto">
                    {p.calls} consultas
                  </span>
                )}
              </div>
              <div className="flex items-center gap-3 mb-2.5">
                <Bar value={p.bytes} max={maxBytes} />
                <span className="text-[10px] tabular-nums shrink-0" style={{ color: "var(--text-3)" }}>
                  {p.last_activity ? new Date(p.last_activity).toLocaleDateString() : "sin fecha"}
                </span>
              </div>
              {confirmDel?.project === p.project ? (
                <div className="flex flex-wrap items-center gap-2 text-xs">
                  <span style={{ color: "var(--danger)" }}>
                    ¿Borrar {confirmDel.count} recuerdos de «{p.project}»? Se descargará el backup antes.
                  </span>
                  <button className="btn btn-gold text-xs" disabled={busy === p.project} onClick={confirmFree}>
                    Descargar y borrar
                  </button>
                  <button className="btn btn-ghost text-xs" disabled={busy === p.project} onClick={() => setConfirmDel(null)}>
                    Cancelar
                  </button>
                </div>
              ) : (
                <div className="flex flex-wrap gap-2">
                  <button className="btn btn-ghost text-xs" disabled={!!busy} onClick={() => download(p)}>
                    ⬇︎ Descargar backup
                  </button>
                  <button className="btn btn-ghost text-xs" disabled={!!busy} onClick={() => updateBackup(p)} title="Fusiona la memoria actual del proyecto con un backup existente">
                    ⟳ Actualizar backup
                  </button>
                  <button className="btn btn-danger text-xs" disabled={!!busy} onClick={() => askFree(p)} title="Descarga el backup y borra de AION para liberar espacio">
                    🗑 Descargar y liberar
                  </button>
                </div>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

type ChartPoint = { label: string; value: number };

/** Gráfico de área estilo bursátil (SVG puro): gradiente, rejilla, ejes y crosshair al hover. */
function AreaChart({ data, color, fmtVal }: { data: ChartPoint[]; color: string; fmtVal: (v: number) => string }) {
  const [hover, setHover] = useState<number | null>(null);
  const W = 720, H = 200, pl = 12, pr = 12, pt = 16, pb = 24;
  const innerW = W - pl - pr, innerH = H - pt - pb;
  const n = data.length;
  if (n === 0) return <p className="text-sm py-6 text-center" style={{ color: "var(--text-3)" }}>Sin datos aún.</p>;
  const max = Math.max(1e-9, ...data.map((d) => d.value));
  const X = (i: number) => (n <= 1 ? pl + innerW / 2 : pl + (i * innerW) / (n - 1));
  const Y = (v: number) => pt + innerH - (v / max) * innerH;
  const line = data.map((d, i) => `${i ? "L" : "M"}${X(i).toFixed(1)},${Y(d.value).toFixed(1)}`).join(" ");
  const area = `${line} L${X(n - 1).toFixed(1)},${(pt + innerH).toFixed(1)} L${X(0).toFixed(1)},${(pt + innerH).toFixed(1)} Z`;
  const onMove = (e: React.MouseEvent<SVGSVGElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    const x = ((e.clientX - r.left) / r.width) * W;
    const i = Math.max(0, Math.min(n - 1, Math.round(((x - pl) / innerW) * (n - 1))));
    setHover(i);
  };
  const tipX = hover === null ? 0 : Math.min(Math.max(X(hover), 58), W - 58);
  return (
    <svg viewBox={`0 0 ${W} ${H}`} width="100%" style={{ display: "block" }} onMouseMove={onMove} onMouseLeave={() => setHover(null)}>
      <defs>
        <linearGradient id="areafill" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor={color} stopOpacity="0.30" />
          <stop offset="100%" stopColor={color} stopOpacity="0" />
        </linearGradient>
      </defs>
      {[0, 0.5, 1].map((f) => (
        <g key={f}>
          <line x1={pl} x2={W - pr} y1={pt + innerH * (1 - f)} y2={pt + innerH * (1 - f)} stroke="var(--border)" strokeWidth="1" strokeDasharray="3 4" />
          <text x={W - pr} y={pt + innerH * (1 - f) - 3} textAnchor="end" fontSize="9" fill="var(--text-3)">{fmtVal(max * f)}</text>
        </g>
      ))}
      <path d={area} fill="url(#areafill)" />
      <path d={line} fill="none" stroke={color} strokeWidth="2" strokeLinejoin="round" strokeLinecap="round" />
      <circle cx={X(n - 1)} cy={Y(data[n - 1].value)} r="3" fill={color} />
      {[...new Set([0, Math.floor((n - 1) / 2), n - 1])].map((i) => (
        <text key={i} x={X(i)} y={H - 6} textAnchor={i === 0 ? "start" : i === n - 1 ? "end" : "middle"} fontSize="9" fill="var(--text-3)">{data[i].label}</text>
      ))}
      {hover !== null && (
        <g>
          <line x1={X(hover)} x2={X(hover)} y1={pt} y2={pt + innerH} stroke={color} strokeWidth="1" strokeDasharray="2 3" opacity="0.6" />
          <circle cx={X(hover)} cy={Y(data[hover].value)} r="4" fill={color} stroke="#fff" strokeWidth="1.5" />
          <g transform={`translate(${tipX},${pt + 6})`}>
            <rect x="-54" y="-4" width="108" height="32" rx="6" fill="#11110f" opacity="0.94" />
            <text x="0" y="8" textAnchor="middle" fontSize="9" fill="#cbb78a">{data[hover].label}</text>
            <text x="0" y="21" textAnchor="middle" fontSize="11" fontWeight="bold" fill="#fff">{fmtVal(data[hover].value)}</text>
          </g>
        </g>
      )}
    </svg>
  );
}

/** Barras verticales con gradiente (mensual). */
function MoneyBars({ data, color, fmtVal }: { data: ChartPoint[]; color: string; fmtVal: (v: number) => string }) {
  if (data.length === 0) return <p className="text-sm py-4 text-center" style={{ color: "var(--text-3)" }}>Sin datos aún.</p>;
  const max = Math.max(1e-9, ...data.map((d) => d.value));
  return (
    <div className="flex items-end gap-3" style={{ height: 130 }}>
      {data.map((d, i) => (
        <div key={i} className="flex-1 flex flex-col items-center justify-end" style={{ height: "100%" }} title={`${d.label}: ${fmtVal(d.value)}`}>
          <span className="text-[10px] mb-1 tabular-nums" style={{ color: "var(--text-2)" }}>{fmtVal(d.value)}</span>
          <div className="w-full rounded-t transition-all" style={{ height: `${Math.max(4, (d.value / max) * 100)}%`, background: `linear-gradient(180deg, ${color}, ${color}44)` }} />
          <span className="text-[10px] mt-1.5" style={{ color: "var(--text-3)" }}>{d.label}</span>
        </div>
      ))}
    </div>
  );
}

/** Panel de TOKENS del puente AION↔Claude Code con gráficos en tiempo real. HONESTO: muestra los
 *  tokens de memoria que AION inyecta en tus sesiones (un COSTE, a cambio de continuidad) + un
 *  ahorro ESTIMADO —solo lecturas— vs. volcar todo el corpus. Sin dinero. */
function CostPanel({ stats }: { stats: ClaudeCodeStats | null }) {
  const [cost, setCost] = useState<CostData | null>(null);
  const load = useCallback(() => { claudeCodeCost().then((c) => { if (c) setCost(c); }).catch(() => {}); }, []);
  useEffect(() => { load(); const iv = setInterval(load, 15_000); return () => clearInterval(iv); }, [load]);
  if (!cost) return null;

  const monthKey = new Date().toISOString().slice(0, 7);
  const todayKey = new Date().toISOString().slice(0, 10);
  const monthTokens = cost.monthly.find((m) => m.month === monthKey)?.tokens ?? 0;
  const todayTokens = cost.daily.find((d) => d.day === todayKey)?.tokens ?? 0;
  const corpus = stats?.corpus_tokens ?? 0;
  const writeTokens = Math.max(0, cost.total_tokens - cost.read_tokens);
  // Ahorro estimado SOLO sobre LECTURAS (las escrituras no leen memoria → no ahorran): cada lectura
  // sirve ~avgRead tok relevantes en vez de los `corpus` de volcar toda la memoria.
  const avgRead = cost.read_calls > 0 ? cost.read_tokens / cost.read_calls : 0;
  const saved = Math.max(0, corpus - avgRead) * cost.read_calls;
  // Porcentaje de ahorro POR LECTURA vs volcar el corpus servible entero: cada lectura sirve
  // ~avgRead en vez de `corpus`. Es la respuesta honesta a "¿cuánto % ahorro?" (acotado [0,100)).
  const savedPct = corpus > 0 && avgRead > 0 ? Math.min(99.9, Math.max(0, (1 - avgRead / corpus) * 100)) : 0;
  const ft = (v: number) => fmtTokens(Math.round(v));

  const dailyT: ChartPoint[] = cost.daily.map((d) => ({ label: d.day.slice(5), value: d.tokens }));
  const monthlyT: ChartPoint[] = cost.monthly.map((m) => ({ label: m.month, value: m.tokens }));

  return (
    <div className="card" style={{ border: "1px solid var(--border)" }}>
      <div className="flex items-center gap-2 mb-1">
        <span style={{ fontSize: 16 }}>📊</span>
        <h2 className="t-section" style={{ color: "var(--text-2)" }}>Tokens del puente AION ↔ Claude Code</h2>
      </div>
      <p className="text-xs mb-4 leading-relaxed" style={{ color: "var(--text-3)" }}>
        Tokens de memoria que AION inyecta en tus sesiones de Claude Code. Son un <b>coste</b> a cambio de
        <b> continuidad</b> (Claude conoce tus proyectos sin re-explicar). El <b>ahorro</b> es una
        <b> estimación</b> y solo aplica a las <b>lecturas</b>: cuando Claude lee memoria recibe ~{ft(avgRead)} tok
        relevantes en vez de volcar el <b>corpus servible</b> (~{ft(corpus)} tok) —un <b>{savedPct.toFixed(1)}%</b> menos
        por lectura—. El corpus servible excluye recuerdos privados e introspección de AION, que nunca salen al puente.
        Las <b>escrituras</b> (remember) no ahorran.
      </p>

      {/* Héroes */}
      <div className="grid grid-cols-2 sm:grid-cols-4 gap-3 mb-5">
        {[
          { k: "Tokens servidos (total)", v: ft(cost.total_tokens), s: `${cost.read_calls} lecturas`, accent: "var(--text-1)" },
          { k: "Este mes", v: ft(monthTokens), s: "tokens", accent: "var(--text-1)" },
          { k: "Hoy", v: ft(todayTokens), s: "tokens", accent: "var(--text-1)" },
          { k: "Ahorro por lectura", v: `${savedPct.toFixed(1)}%`, s: `~${ft(saved)} tok ahorrados · vs volcar el corpus`, accent: "var(--accent)" },
        ].map((h) => (
          <div key={h.k} className="rounded-lg p-3" style={{ background: "var(--surface-1)" }}>
            <div className="text-xl font-bold tabular-nums" style={{ color: h.accent }}>{h.v}</div>
            <div className="text-[11px] mt-0.5" style={{ color: "var(--text-2)" }}>{h.k}</div>
            <div className="text-[10px]" style={{ color: "var(--text-3)" }}>{h.s}</div>
          </div>
        ))}
      </div>

      {/* Gráfico diario (área bursátil) */}
      <div className="mb-5">
        <div className="text-xs font-medium uppercase tracking-wide mb-1" style={{ color: "var(--text-3)" }}>Tokens servidos por día</div>
        <AreaChart data={dailyT} color="var(--accent)" fmtVal={ft} />
      </div>

      {/* Gráfico mensual (barras) */}
      <div className="mb-4">
        <div className="text-xs font-medium uppercase tracking-wide mb-2" style={{ color: "var(--text-3)" }}>Tokens por mes</div>
        <MoneyBars data={monthlyT} color="var(--accent)" fmtVal={ft} />
      </div>

      {/* Desglose lectura/escritura (transparencia) */}
      <div className="text-[11px]" style={{ color: "var(--text-3)" }}>
        <span style={{ color: "var(--accent)" }}>Lecturas: {ft(cost.read_tokens)} tok</span> ({cost.read_calls} consultas) ·{" "}
        Escrituras: {ft(writeTokens)} tok <span style={{ opacity: 0.8 }}>(guardan memoria, no ahorran)</span>
      </div>
    </div>
  );
}

/** Panel informativo de PRIVACIDAD: explica el blindaje de secretos/datos confidenciales,
 *  incluido el caso de usar un LLM externo de pago. Honesto sobre los límites. */
function PrivacyPanel() {
  return (
    <div className="card" style={{ border: "1px solid var(--border)" }}>
      <div className="flex items-center gap-2 mb-2">
        <span style={{ fontSize: 16 }}>🔒</span>
        <h2 className="t-section" style={{ color: "var(--text-2)" }}>Privacidad y datos confidenciales</h2>
      </div>
      <p className="text-sm leading-relaxed mb-3" style={{ color: "var(--text-2)" }}>
        AION es <b>local</b> (Gemma on-device, privado). Claude Code y cualquier LLM externo son
        <b> remotos</b>. Antes de que CUALQUIER texto salga del Mac hacia un modelo remoto, AION
        <b> redacta secretos automáticamente</b> — de forma determinista, local y sin latencia.
      </p>
      <div className="grid grid-cols-1 sm:grid-cols-2 gap-2.5 mb-3">
        {[
          { t: "Redacción de egreso", d: "IBAN, tarjetas (Luhn), claves de API (sk-/ghp_/AKIA/JWT…) y contraseñas → [IBAN] · [TARJETA] · [CLAVE]. Validado con checksums para no sobre-redactar." },
          { t: "Marca [confidencial]", d: "Cualquier recuerdo marcado [confidencial] NUNCA se sirve al puente ni a un LLM externo (deny duro), da igual su relevancia." },
          { t: "LLM externo de pago", d: "Si activas un proveedor externo (DeepSeek, etc.), la MISMA redacción corre antes de que el texto salga del Mac → privacidad máxima también ahí." },
          { t: "Solo en egresos remotos", d: "El Gemma local recibe la memoria íntegra (privado y gratis). La redacción solo actúa hacia modelos remotos." },
        ].map((b) => (
          <div key={b.t} className="rounded-lg p-3" style={{ background: "var(--surface-1)" }}>
            <div className="text-sm font-medium mb-0.5" style={{ color: "var(--accent)" }}>{b.t}</div>
            <div className="text-xs leading-relaxed" style={{ color: "var(--text-3)" }}>{b.d}</div>
          </div>
        ))}
      </div>
      <div className="rounded-lg p-3 text-xs leading-relaxed" style={{ background: "var(--surface-1)", color: "var(--text-3)", borderLeft: "3px solid #f59e0b" }}>
        <b style={{ color: "var(--text-2)" }}>Límites honestos.</b> La redacción protege el canal de la
        memoria de AION. NO cubre lo que escribes <b>directamente</b> en Claude Code ni los archivos
        que Claude lee del repo — eso sale igual. Para máxima privacidad real con datos bancarios:
        (1) márcalos <code className="font-mono">[confidencial]</code> o guárdalos fuera del chat,
        (2) usa cuenta <b>Commercial/API o Enterprise</b> de Anthropic (no entrena con tus datos +
        retención cero), evitando cuentas Consumer (Free/Pro/Max) que por defecto entrenan salvo
        opt-out. <i>Próximo paso: bóveda local cifrada para claves estructuradas.</i>
      </div>
    </div>
  );
}

/** Bóveda de secretos: claves/datos bancarios cifrados en el Llavero de macOS. El valor NUNCA
 *  llega a ningún LLM ni al puente — solo se revela aquí, bajo acción local explícita. */
function VaultPanel() {
  const [secrets, setSecrets] = useState<VaultSecret[]>([]);
  const [name, setName] = useState("");
  const [value, setValue] = useState("");
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState("");
  const [revealed, setRevealed] = useState<Record<string, string>>({});

  const load = useCallback(() => {
    vaultList().then(setSecrets).catch(() => {});
  }, []);
  useEffect(() => {
    load();
  }, [load]);

  async function add() {
    if (!name.trim() || !value.trim() || busy) return;
    setBusy(true);
    const r = await vaultSet(name.trim(), value, note.trim()).catch(() => ({ ok: false, error: "sin respuesta" }));
    if (r.ok) {
      setName(""); setValue(""); setNote("");
      setMsg(`✓ Guardado en el Llavero (cifrado, fuera de la memoria)`);
      load();
    } else {
      setMsg(`⚠️ ${r.error ?? "no se pudo guardar"}`);
    }
    setBusy(false);
  }
  async function reveal(n: string) {
    if (revealed[n] !== undefined) {
      setRevealed((m) => { const c = { ...m }; delete c[n]; return c; });
      return;
    }
    const r = await vaultGet(n).catch(() => ({ ok: false, value: undefined as string | undefined }));
    if (r.ok && r.value !== undefined) setRevealed((m) => ({ ...m, [n]: r.value as string }));
  }
  async function del(n: string) {
    if (!confirm(`¿Eliminar el secreto «${n}» del Llavero?`)) return;
    await vaultRemove(n).catch(() => {});
    setRevealed((m) => { const c = { ...m }; delete c[n]; return c; });
    load();
  }

  return (
    <div className="card" style={{ border: "1px solid var(--border)" }}>
      <div className="flex items-center gap-2 mb-1">
        <span style={{ fontSize: 16 }}>🗝️</span>
        <h2 className="t-section" style={{ color: "var(--text-2)" }}>Bóveda de secretos</h2>
      </div>
      <p className="text-xs mb-4 leading-relaxed" style={{ color: "var(--text-3)" }}>
        Claves, datos bancarios y tokens cifrados en el <b>Llavero de macOS</b>. El valor vive solo
        ahí — <b>nunca</b> entra en la memoria de AION, ni se embebe, ni se sirve a Claude Code o a un
        LLM externo. Solo se revela aquí, en tu Mac, bajo tu acción.
      </p>

      {msg && (
        <p className="text-xs mb-3 px-3 py-2 rounded-lg" style={{ background: "var(--surface-1)", color: "var(--text-2)" }}>{msg}</p>
      )}

      {/* Alta */}
      <div className="flex flex-wrap gap-2 mb-4">
        <input value={name} onChange={(e) => setName(e.target.value)} placeholder="nombre (ej. banco-santander)"
          className="text-sm px-3 py-2 rounded-lg" style={{ background: "var(--surface-1)", border: "1px solid var(--border)", color: "var(--text-1)", minWidth: 180, flex: 1 }} />
        <input value={value} onChange={(e) => setValue(e.target.value)} type="password" placeholder="valor (clave / nº de cuenta)"
          className="text-sm px-3 py-2 rounded-lg" style={{ background: "var(--surface-1)", border: "1px solid var(--border)", color: "var(--text-1)", minWidth: 180, flex: 1 }} />
        <input value={note} onChange={(e) => setNote(e.target.value)} placeholder="nota (opcional)"
          className="text-sm px-3 py-2 rounded-lg" style={{ background: "var(--surface-1)", border: "1px solid var(--border)", color: "var(--text-1)", minWidth: 120, flex: 1 }} />
        <button className="btn btn-gold text-sm" disabled={busy || !name.trim() || !value.trim()} onClick={add}>Guardar</button>
      </div>

      {/* Lista */}
      {secrets.length === 0 ? (
        <p className="text-sm" style={{ color: "var(--text-3)" }}>Aún no hay secretos guardados.</p>
      ) : (
        <div className="flex flex-col gap-2">
          {secrets.map((s) => (
            <div key={s.name} className="flex items-center gap-3 rounded-lg p-2.5" style={{ background: "var(--surface-1)" }}>
              <div className="min-w-0 flex-1">
                <span className="font-mono text-sm" style={{ color: "var(--text-1)" }}>{s.name}</span>
                {s.note && <span className="text-xs ml-2" style={{ color: "var(--text-3)" }}>{s.note}</span>}
                {revealed[s.name] !== undefined && (
                  <div className="font-mono text-xs mt-1 px-2 py-1 rounded break-all" style={{ background: "var(--surface-2)", color: "var(--accent)" }}>
                    {revealed[s.name]}
                  </div>
                )}
              </div>
              <button className="btn text-xs shrink-0" onClick={() => reveal(s.name)}>
                {revealed[s.name] !== undefined ? "Ocultar" : "Revelar"}
              </button>
              <button className="btn text-xs shrink-0" style={{ color: "var(--danger)" }} onClick={() => del(s.name)}>Eliminar</button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

export default function ClaudeCodePage() {
  const { t } = useT();
  const [cc, setCc] = useState<ClaudeCodeStatus | null>(null);
  const [stats, setStats] = useState<ClaudeCodeStats | null>(null);
  const [audit, setAudit] = useState<ClaudeCodeAuditEntry[]>([]);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState("");
  const [copied, setCopied] = useState(false);
  const [tab, setTab] = useState<"calls" | "tokens">("calls");

  const refresh = useCallback(async () => {
    claudeCodeGet().then(setCc).catch(() => {});
    claudeCodeStats().then(setStats).catch(() => {});
    claudeCodeAudit(200).then((e) => setAudit(e.reverse())).catch(() => {});
  }, []);

  useEffect(() => {
    refresh();
    const iv = setInterval(refresh, 15_000);
    return () => clearInterval(iv);
  }, [refresh]);

  const connected = !!cc && cc.enabled && cc.registered;
  const cliFound = cc?.cli_found ?? true;

  async function connect() {
    if (busy) return;
    setBusy(true);
    setMsg(t("cc.connecting"));
    const r = await claudeCodeConnect(cc?.auto_brief).catch(() => ({ ok: false, error: "sin respuesta" }));
    setMsg(r.ok ? `✓ ${t("cc.connected")}` : r.error === "cli_not_found" ? t("cc.installHint") : `⚠️ ${r.error ?? "error"}`);
    await refresh();
    setBusy(false);
  }
  async function disconnect() {
    if (busy) return;
    setBusy(true);
    await claudeCodeDisconnect().catch(() => {});
    setMsg(t("cc.notConnected"));
    await refresh();
    setBusy(false);
  }
  async function test() {
    setMsg("…");
    const r = await claudeCodeTest().catch(() => null);
    if (!r) setMsg("⚠️ AION no responde");
    else if (!r.cli_found) setMsg(t("cc.installHint"));
    else if (r.ok)
      setMsg(`✓ ${t("cc.connected")} · ${t("cc.lastSeen")}: ${r.last_seen_at ? new Date(r.last_seen_at).toLocaleString() : t("cc.never")}`);
    else setMsg(t("cc.notConnected"));
  }
  async function toggleBrief(next: boolean) {
    if (!cc) return;
    setCc({ ...cc, auto_brief: next });
    await claudeCodeSet({ auto_brief: next }).catch(() => {});
  }
  async function copyInstall() {
    try {
      await navigator.clipboard.writeText(INSTALL_CMD);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch { /* sin permiso */ }
  }

  // ── Cálculos derivados ───────────────────────────────────────────────────
  const errorsCount = stats?.errors ?? 0;
  const errorPct = stats && stats.total_calls > 0
    ? Math.round((errorsCount / stats.total_calls) * 100)
    : 0;
  const avgPerCall = stats?.avg_tokens_per_call
    ?? (stats && stats.total_calls > 0 ? Math.round(stats.tokens_served / stats.total_calls) : 0);
  // Corpus de memoria accesible bajo demanda (NO un "ahorro": es el contexto al que Claude
  // accede sin cargarlo entero — por consulta solo paga `avgPerCall`).
  const corpusTokens = stats?.corpus_tokens ?? 0;

  // Datos para el gráfico por herramienta
  const byToolCalls: [string, number][] = Object.entries(stats?.by_tool ?? {}).sort((a, b) => b[1] - a[1]);
  const byToolTokens: [string, number][] = Object.entries(stats?.by_tool_tokens ?? stats?.by_tool ?? {}).sort((a, b) => b[1] - a[1]);
  const toolRows = tab === "calls" ? byToolCalls : byToolTokens;
  const maxTool = toolRows.length ? toolRows[0][1] : 0;

  return (
    <AppShell title={t("cc.title")}>
      <div className="max-w-6xl mx-auto px-3 py-6 flex flex-col gap-4">

        {/* ── CABECERA: estado + control (patrón de Mente) ── */}
        <div className="card" style={{ boxShadow: "var(--shadow-elevated)" }}>
          <div className="flex flex-wrap items-center justify-between gap-3 mb-1">
            <div className="flex items-center gap-3 min-w-0">
              <span
                className="w-11 h-11 rounded-2xl flex items-center justify-center shrink-0"
                style={{ background: "var(--accent-subtle)", color: "var(--gold-deep)" }}
              >
                <Icon name="code" size={20} />
              </span>
              <div className="font-display text-lg font-bold" style={{ color: "var(--text-1)" }}>
                {t("cc.title")}
              </div>
            </div>
            <span className="flex items-center gap-1.5 text-sm" style={{ color: connected ? "var(--accent)" : "var(--text-3)" }}>
              <span className="inline-block w-2 h-2 rounded-full" style={{ background: connected ? "var(--accent)" : "var(--text-3)" }} />
              {connected ? t("cc.connected") : t("cc.notConnected")}
            </span>
          </div>
          <p className="text-xs mb-3" style={{ color: "var(--text-3)" }}>{t("cc.note")}</p>
          <div className="flex flex-wrap items-center gap-2">
            {connected ? (
              <button className="btn" disabled={busy} onClick={disconnect}>{t("cc.disconnect")}</button>
            ) : (
              <button className="btn" disabled={busy || !cliFound} onClick={connect}
                style={{ background: "var(--ink)", color: "#fff", opacity: busy || !cliFound ? 0.5 : 1 }}>
                {busy ? t("cc.connecting") : t("cc.connect")}
              </button>
            )}
            <button className="btn" disabled={busy} onClick={test}>{t("cc.test")}</button>
            <label className="flex items-center gap-2 text-sm ml-auto" style={{ color: "var(--text-2)" }}>
              <input type="checkbox" checked={cc?.auto_brief ?? false} onChange={(e) => toggleBrief(e.target.checked)} />
              {t("cc.autoBrief")}
            </label>
          </div>
          {connected && (
            <p className="text-xs mt-2" style={{ color: "var(--text-3)" }}>
              {t("cc.lastSeen")}: {cc?.last_seen_at ? new Date(cc.last_seen_at).toLocaleString() : t("cc.never")}
            </p>
          )}
          {msg && <p className="text-xs mt-2 whitespace-pre-wrap" style={{ color: "var(--text-2)" }}>{msg}</p>}
        </div>

        {/* ── Onboarding (no conectado) ── */}
        {!connected && (
          <div className="card">
            <h2 className="t-section mb-3" style={{ color: "var(--text-2)" }}>{t("cc.howTitle")}</h2>
            <div className="flex flex-col gap-4">
              {[
                { step: "1", titleKey: "cc.step1", noteKey: "cc.step1Note", done: cliFound },
                { step: "2", titleKey: "cc.step2", noteKey: "cc.step2Note", done: false },
                { step: "3", titleKey: "cc.step3", noteKey: "cc.step3Note", done: false },
              ].map(({ step, titleKey, noteKey, done }) => (
                <div key={step} className="flex gap-3">
                  <span className="shrink-0 w-6 h-6 rounded-full flex items-center justify-center text-xs font-semibold"
                    style={{ background: done ? "var(--accent)" : "var(--surface-2)", color: done ? "#04201f" : "var(--text-2)" }}>
                    {done ? "✓" : step}
                  </span>
                  <div className="min-w-0 flex-1">
                    <p className="text-sm font-medium" style={{ color: "var(--text-1)" }}>
                      {t(titleKey as Parameters<typeof t>[0])}
                      {done && <span className="text-[10px] ml-2 px-1.5 py-0.5 rounded-full" style={{ background: "var(--accent-subtle)", color: "var(--gold-deep)" }}>{t("cc.stepDone")}</span>}
                    </p>
                    <p className="text-xs mt-0.5" style={{ color: "var(--text-3)" }}>{t(noteKey as Parameters<typeof t>[0])}</p>
                    {step === "1" && !cliFound && (
                      <div className="flex items-center gap-2 mt-2 px-3 py-2 rounded-lg font-mono text-xs" style={{ background: "var(--surface-2)", color: "var(--text-2)" }}>
                        <span className="flex-1 truncate">{INSTALL_CMD}</span>
                        <button className="text-xs shrink-0" style={{ color: "var(--gold-deep)" }} onClick={copyInstall}>
                          {copied ? t("cc.copied") : t("cc.copy")}
                        </button>
                      </div>
                    )}
                  </div>
                </div>
              ))}
            </div>
            <p className="text-xs mt-4" style={{ color: "var(--text-3)" }}>{t("cc.disconnectedHint")} {t("cc.settingsLink")}</p>
          </div>
        )}

        {/* ── Tools ── */}
        <div className="card">
          <h2 className="t-section mb-2" style={{ color: "var(--text-2)" }}>{t("cc.toolsTitle")}</h2>
          <div className="flex flex-wrap gap-2">
            {TOOLS.map((tool) => (
              <span key={tool.name} className="text-xs px-2.5 py-1.5 rounded-full" style={{ background: "var(--surface-2)", color: "var(--text-2)" }}>
                <span className="font-mono" style={{ color: "var(--gold-deep)" }}>{toolLabel(tool.name)}</span>
                {" · "}{t(tool.key)}
              </span>
            ))}
          </div>
        </div>

        {connected && stats && (
          <>
            {/* ── Panel de eficiencia (hero) — framing HONESTO: coste por consulta vs corpus ── */}
            <div className="card" style={{ background: "var(--surface-1)" }}>
              <div className="flex flex-wrap items-center gap-6">
                <div className="text-center shrink-0">
                  <div className="text-3xl font-bold" style={{ color: "var(--accent)" }}>~{fmtTokens(avgPerCall)}</div>
                  <div className="text-[11px] mt-0.5" style={{ color: "var(--text-3)" }}>tokens por consulta</div>
                </div>
                <div className="text-center shrink-0">
                  <div className="text-3xl font-bold" style={{ color: "var(--text-1)" }}>~{fmtTokens(corpusTokens)}</div>
                  <div className="text-[11px] mt-0.5" style={{ color: "var(--text-3)" }}>corpus servible</div>
                </div>
                <div className="flex-1" style={{ minWidth: 220 }}>
                  <p className="text-sm leading-relaxed" style={{ color: "var(--text-2)" }}>
                    El puente sirve <b>solo lo relevante bajo demanda</b>: Claude accede a un corpus servible de
                    ~{fmtTokens(corpusTokens)} tokens (memoria útil, sin recuerdos privados ni introspección de AION)
                    pagando ~{fmtTokens(avgPerCall)} por consulta, sin cargarla entera en el contexto.
                  </p>
                </div>
              </div>
            </div>

            {/* ── Tokens del puente + ahorro estimado en tiempo real ── */}
            <CostPanel stats={stats} />

            {/* ── Privacidad y datos confidenciales ── */}
            <PrivacyPanel />

            {/* ── Bóveda de secretos (Llavero macOS) ── */}
            <VaultPanel />

            {/* ── Métricas en grid 3×2 ── */}
            <div className="grid grid-cols-3 gap-3">
              {[
                { label: t("cc.metrics.calls"), value: String(stats.total_calls) },
                { label: t("cc.metrics.writes"), value: String(stats.writes) },
                { label: t("cc.metrics.memory"), value: String(stats.memory_count ?? "—") },
                { label: t("cc.metrics.tokens"), value: fmtTokens(stats.tokens_served) },
                { label: t("cc.metrics.avgTokens"), value: fmtTokens(avgPerCall) },
                {
                  label: t("cc.metrics.errors"),
                  value: errorsCount > 0 ? `${errorsCount} (${errorPct}%)` : "0",
                  accent: errorsCount > 0 ? "var(--danger)" : undefined,
                },
              ].map((m) => (
                <div key={m.label} className="card" style={{ padding: "12px 14px" }}>
                  <div className="text-xl font-semibold" style={{ color: m.accent ?? "var(--text-1)" }}>{m.value}</div>
                  <div className="text-xs mt-0.5" style={{ color: "var(--text-3)" }}>{m.label}</div>
                </div>
              ))}
            </div>

            {/* ── Grafo de conocimiento ── */}
            {((stats.graph_concepts ?? 0) > 0 || (stats.graph_communities ?? 0) > 0) && (
              <div className="card">
                <h2 className="t-section mb-3" style={{ color: "var(--text-2)" }}>{t("cc.graphStats")}</h2>
                <div className="flex gap-6">
                  <div>
                    <div className="text-2xl font-semibold" style={{ color: "var(--text-1)" }}>{(stats.graph_concepts ?? 0).toLocaleString()}</div>
                    <div className="text-xs mt-0.5" style={{ color: "var(--text-3)" }}>{t("cc.graphConcepts")}</div>
                  </div>
                  <div>
                    <div className="text-2xl font-semibold" style={{ color: "var(--text-1)" }}>{stats.graph_communities ?? 0}</div>
                    <div className="text-xs mt-0.5" style={{ color: "var(--text-3)" }}>{t("cc.graphComms")}</div>
                  </div>
                  <div className="flex-1 flex items-center">
                    <div className="w-full h-1.5 rounded-full overflow-hidden" style={{ background: "var(--surface-2)" }}>
                      <div className="h-full rounded-full" style={{
                        width: `${Math.min(100, ((stats.graph_concepts ?? 0) / 20000) * 100)}%`,
                        background: "var(--accent)",
                      }} />
                    </div>
                  </div>
                </div>
              </div>
            )}

            {/* ── Uso por herramienta (tabs llamadas / tokens) ── */}
            {toolRows.length > 0 && (
              <div className="card">
                <div className="flex items-center justify-between mb-3">
                  <h2 className="t-section" style={{ color: "var(--text-2)" }}>{t("cc.byTool")}</h2>
                  <div className="flex gap-1">
                    {(["calls", "tokens"] as const).map((k) => (
                      <button key={k} onClick={() => setTab(k)}
                        className="text-xs px-2.5 py-1 rounded-full"
                        style={{
                          background: tab === k ? "var(--accent)" : "var(--surface-2)",
                          color: tab === k ? "#04201f" : "var(--text-3)",
                        }}>
                        {k === "calls" ? t("cc.metrics.calls") : t("cc.byToolTokens")}
                      </button>
                    ))}
                  </div>
                </div>
                <div className="flex flex-col gap-2.5">
                  {toolRows.map(([tool, count]) => (
                    <div key={tool} className="flex items-center gap-3 text-sm">
                      <span className="w-32 shrink-0 truncate font-mono text-xs" style={{ color: "var(--text-2)" }}>
                        {toolLabel(tool)}
                      </span>
                      <Bar value={count} max={maxTool} />
                      <span className="w-12 text-right text-xs tabular-nums" style={{ color: "var(--text-3)" }}>
                        {tab === "tokens" ? fmtTokens(count) : count}
                      </span>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {/* ── Memoria por proyecto (medidor + backup/restore + liberar) ── */}
            <MemoryByProject />

            {/* ── Auditoría ── */}
            <div className="card">
              <h2 className="t-section mb-3" style={{ color: "var(--text-2)" }}>{t("cc.audit")}</h2>
              {audit.length === 0 ? (
                <p className="text-sm" style={{ color: "var(--text-3)" }}>{t("cc.noAudit")}</p>
              ) : (
                <div className="overflow-x-auto">
                  <table className="w-full text-sm">
                    <thead>
                      <tr className="text-left text-xs" style={{ color: "var(--text-3)" }}>
                        <th className="py-1.5 pr-3 font-medium">{t("cc.colTime")}</th>
                        <th className="py-1.5 pr-3 font-medium">{t("cc.colTool")}</th>
                        <th className="py-1.5 pr-3 font-medium">{t("cc.colQuery")}</th>
                        <th className="py-1.5 text-right font-medium">{t("cc.colTokens")}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {audit.map((e, i) => (
                        <tr key={i} style={{ borderTop: "1px solid var(--border)" }}>
                          <td className="py-1.5 pr-3 whitespace-nowrap text-xs" style={{ color: "var(--text-3)" }}>
                            {new Date(e.ts).toLocaleString()}
                          </td>
                          <td className="py-1.5 pr-3 whitespace-nowrap">
                            <span className="text-[11px] px-2 py-0.5 rounded-full"
                              style={{
                                background: e.tool === "aion_remember" ? "var(--accent-subtle)" : "var(--surface-2)",
                                color: e.tool === "aion_remember" ? "var(--gold-deep)" : "var(--text-2)",
                              }}>
                              {toolLabel(e.tool)}
                            </span>
                            {!e.ok && <span className="text-[10px] ml-1.5" style={{ color: "var(--danger)" }}>error</span>}
                          </td>
                          <td className="py-1.5 pr-3 max-w-[260px] truncate" style={{ color: "var(--text-2)" }} title={e.query}>
                            {e.query || "—"}
                          </td>
                          <td className="py-1.5 text-right text-xs whitespace-nowrap tabular-nums" style={{ color: "var(--text-3)" }}>
                            {fmtTokens(e.est_tokens)}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </div>
          </>
        )}

        {/* ── Ejemplos ── */}
        <div className="card">
          <h2 className="t-section mb-2" style={{ color: "var(--text-2)" }}>{t("cc.examplesTitle")}</h2>
          <div className="flex flex-col gap-1.5">
            {(["cc.example1", "cc.example2", "cc.example3"] as const).map((k) => (
              <p key={k} className="text-sm px-3 py-2 rounded-lg" style={{ background: "var(--surface-1)", color: "var(--text-2)" }}>
                «{t(k)}»
              </p>
            ))}
          </div>
        </div>

      </div>
    </AppShell>
  );
}
