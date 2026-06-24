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
  memoryNormalize,
  memoryProjects,
  type ClaudeCodeAuditEntry,
  type ClaudeCodeStats,
  type ClaudeCodeStatus,
  type MemoryProjects,
  type ProjectMemory,
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
                  <div className="text-[11px] mt-0.5" style={{ color: "var(--text-3)" }}>memoria accesible</div>
                </div>
                <div className="flex-1" style={{ minWidth: 220 }}>
                  <p className="text-sm leading-relaxed" style={{ color: "var(--text-2)" }}>
                    El puente sirve <b>solo lo relevante bajo demanda</b>: Claude accede a un corpus de
                    ~{fmtTokens(corpusTokens)} tokens de memoria pagando ~{fmtTokens(avgPerCall)} por consulta,
                    sin cargarla entera en el contexto.
                  </p>
                </div>
              </div>
            </div>

            {/* ── Privacidad y datos confidenciales ── */}
            <PrivacyPanel />

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
