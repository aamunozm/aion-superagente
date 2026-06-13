"use client";

import { useCallback, useEffect, useState } from "react";
import AppShell from "@/components/AppShell";
import Icon from "@/components/Icon";
import { useT } from "@/lib/i18n";
import {
  claudeCodeAudit,
  claudeCodeConnect,
  claudeCodeDisconnect,
  claudeCodeGet,
  claudeCodeSet,
  claudeCodeStats,
  claudeCodeTest,
  type ClaudeCodeAuditEntry,
  type ClaudeCodeStats,
  type ClaudeCodeStatus,
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

/** Medidor circular de eficiencia */
function SavingsMeter({ pct }: { pct: number }) {
  const r = 28;
  const circ = 2 * Math.PI * r;
  const fill = circ * (1 - pct / 100);
  const color = pct >= 85 ? "var(--accent)" : pct >= 60 ? "#f59e0b" : "var(--text-3)";
  return (
    <div className="relative flex items-center justify-center" style={{ width: 72, height: 72 }}>
      <svg width="72" height="72" viewBox="0 0 72 72" style={{ transform: "rotate(-90deg)" }}>
        <circle cx="36" cy="36" r={r} fill="none" stroke="var(--surface-2)" strokeWidth="6" />
        <circle
          cx="36" cy="36" r={r} fill="none"
          stroke={color} strokeWidth="6"
          strokeDasharray={circ}
          strokeDashoffset={fill}
          strokeLinecap="round"
          style={{ transition: "stroke-dashoffset 0.6s ease" }}
        />
      </svg>
      <span className="absolute text-sm font-bold" style={{ color }}>
        {pct}%
      </span>
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
  const savingsPct = stats?.savings_pct ?? 0;
  const errorsCount = stats?.errors ?? 0;
  const errorPct = stats && stats.total_calls > 0
    ? Math.round((errorsCount / stats.total_calls) * 100)
    : 0;
  // Calcular avg/savings en frontend si el backend viejo no los devuelve aún
  const avgPerCall = stats?.avg_tokens_per_call
    ?? (stats && stats.total_calls > 0 ? Math.round(stats.tokens_served / stats.total_calls) : 0);
  const totalSavingsEst = stats?.total_savings_est
    ?? (stats ? Math.max(0, stats.full_dump_tokens - avgPerCall) * stats.total_calls : 0);

  // Veredicto "¿vale la pena?"
  function verdictText(): { text: string; color: string } {
    if (!stats || stats.total_calls < 5) {
      return { text: t("cc.verdictLow"), color: "var(--text-3)" };
    }
    if (errorsCount > 0 && errorPct >= 10) {
      return {
        text: t("cc.verdictWarn").replace("{n}", String(errorsCount)).replace("{pct}", String(errorPct)),
        color: "#ef4444",
      };
    }
    if (savingsPct >= 85) {
      return {
        text: t("cc.verdictHigh").replace("{pct}", String(savingsPct)),
        color: "var(--accent)",
      };
    }
    return {
      text: t("cc.verdictGood").replace("{pct}", String(savingsPct)),
      color: "#f59e0b",
    };
  }
  const verdict = verdictText();

  // Datos para el gráfico por herramienta
  const byToolCalls: [string, number][] = Object.entries(stats?.by_tool ?? {}).sort((a, b) => b[1] - a[1]);
  const byToolTokens: [string, number][] = Object.entries(stats?.by_tool_tokens ?? stats?.by_tool ?? {}).sort((a, b) => b[1] - a[1]);
  const toolRows = tab === "calls" ? byToolCalls : byToolTokens;
  const maxTool = toolRows.length ? toolRows[0][1] : 0;

  return (
    <AppShell title={t("cc.title")}>
      <div className="max-w-6xl mx-auto px-3 py-6 flex flex-col gap-4">

        {/* ── Estado + control ── */}
        <div className="card">
          <div className="flex items-center justify-between mb-1">
            <h2 className="t-section flex items-center gap-2" style={{ color: "var(--text-2)" }}>
              <Icon name="code" size={16} /> {t("cc.title")}
            </h2>
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
            {/* ── Panel de eficiencia (hero) ── */}
            <div className="card" style={{ background: "var(--surface-1)" }}>
              <div className="flex flex-wrap items-center gap-4">
                {/* medidor */}
                <SavingsMeter pct={savingsPct} />
                {/* texto central */}
                <div className="flex-1" style={{ minWidth: 180 }}>
                  <div className="text-xs font-medium uppercase tracking-wide mb-1" style={{ color: "var(--text-3)" }}>
                    {t("cc.savingsMeter")}
                  </div>
                  <p className="text-sm" style={{ color: "var(--text-2)" }}>
                    <span className="font-semibold" style={{ color: "var(--text-1)" }}>{fmtTokens(avgPerCall)}</span>
                    {" "}{t("cc.perCall")}
                  </p>
                  <p className="text-xs mt-0.5" style={{ color: "var(--text-3)" }}>
                    {fmtTokens(stats.full_dump_tokens)} {t("cc.dumpSize")}
                  </p>
                  <p className="text-xs mt-1.5 leading-relaxed" style={{ color: verdict.color }}>{verdict.text}</p>
                </div>
                {/* total ahorrado */}
                <div className="shrink-0 text-right" style={{ minWidth: 90 }}>
                  <div className="text-2xl font-bold" style={{ color: "var(--accent)" }}>
                    ~{fmtTokens(totalSavingsEst)}
                  </div>
                  <div className="text-[10px] mt-0.5 leading-tight" style={{ color: "var(--text-3)" }}>
                    {t("cc.metrics.totalSaved")}
                  </div>
                  <div className="text-[10px]" style={{ color: "var(--text-3)" }}>({t("cc.estLabel")})</div>
                </div>
              </div>
            </div>

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
                  accent: errorsCount > 0 ? "#ef4444" : undefined,
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
                            {!e.ok && <span className="text-[10px] ml-1.5" style={{ color: "#ef4444" }}>error</span>}
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
