"use client";

import { useCallback, useEffect, useState } from "react";
import AppShell from "@/components/AppShell";
import Icon from "@/components/Icon";
import { useT } from "@/lib/i18n";
import {
  claudeCodeAudit,
  claudeCodeGet,
  claudeCodeStats,
  type ClaudeCodeAuditEntry,
  type ClaudeCodeStats,
  type ClaudeCodeStatus,
} from "@/lib/api";

/** Etiqueta corta y legible por tool (sin el prefijo aion_). */
function toolLabel(tool: string): string {
  return tool.replace(/^aion_/, "").replace(/_/g, " ");
}

function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
  return String(n);
}

export default function ClaudeCodePage() {
  const { t } = useT();
  const [cc, setCc] = useState<ClaudeCodeStatus | null>(null);
  const [stats, setStats] = useState<ClaudeCodeStats | null>(null);
  const [audit, setAudit] = useState<ClaudeCodeAuditEntry[]>([]);

  const refresh = useCallback(async () => {
    claudeCodeGet().then(setCc).catch(() => {});
    claudeCodeStats().then(setStats).catch(() => {});
    claudeCodeAudit(200).then((e) => setAudit(e.reverse())).catch(() => {});
  }, []);

  useEffect(() => {
    refresh();
    const iv = setInterval(refresh, 15_000); // auto-update: refleja actividad en vivo
    return () => clearInterval(iv);
  }, [refresh]);

  const connected = !!cc && cc.enabled && cc.registered;
  // Ahorro estimado: lo que costaría volcar la memoria completa en CADA sesión
  // frente a lo realmente servido bajo demanda.
  const saved =
    stats && stats.full_dump_tokens > 0
      ? Math.max(0, stats.full_dump_tokens - Math.round(stats.tokens_served / Math.max(1, stats.total_calls)))
      : 0;

  const metrics: { key: string; value: string; note?: string }[] = [
    { key: "cc.metrics.calls", value: String(stats?.total_calls ?? 0) },
    { key: "cc.metrics.writes", value: String(stats?.writes ?? 0) },
    { key: "cc.metrics.tokens", value: fmtTokens(stats?.tokens_served ?? 0) },
    {
      key: "cc.metrics.saved",
      value: `~${fmtTokens(saved)}`,
      note: t("cc.savedNote"),
    },
  ];

  return (
    <AppShell title={t("cc.title")}>
      <div className="max-w-3xl mx-auto px-6 py-8 flex flex-col gap-6">
        {/* Estado de conexión */}
        <div className="card flex items-center justify-between">
          <div>
            <h2 className="t-section flex items-center gap-2" style={{ color: "var(--text-2)" }}>
              <Icon name="code" size={16} /> {t("cc.title")}
            </h2>
            <p className="text-xs mt-1" style={{ color: "var(--text-3)" }}>
              {t("cc.lastSeen")}:{" "}
              {cc?.last_seen_at ? new Date(cc.last_seen_at).toLocaleString() : t("cc.never")}
            </p>
          </div>
          <span
            className="flex items-center gap-1.5 text-sm"
            style={{ color: connected ? "var(--accent)" : "var(--text-3)" }}
          >
            <span
              className="inline-block w-2 h-2 rounded-full"
              style={{ background: connected ? "var(--accent)" : "var(--text-3)" }}
            />
            {connected ? t("cc.connected") : t("cc.notConnected")}
          </span>
        </div>

        {/* Métricas */}
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
          {metrics.map((m) => (
            <div key={m.key} className="card">
              <div className="text-2xl font-semibold" style={{ color: "var(--text-1)" }}>
                {m.value}
              </div>
              <div className="text-xs mt-1" style={{ color: "var(--text-3)" }}>
                {t(m.key)}
                {m.note ? <span className="block text-[10px] mt-0.5">{m.note}</span> : null}
              </div>
            </div>
          ))}
        </div>

        {/* Auditoría */}
        <div className="card">
          <h2 className="t-section mb-3" style={{ color: "var(--text-2)" }}>
            {t("cc.audit")}
          </h2>
          {audit.length === 0 ? (
            <p className="text-sm" style={{ color: "var(--text-3)" }}>
              {t("cc.noAudit")}
            </p>
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
                        <span
                          className="text-[11px] px-2 py-0.5 rounded-full"
                          style={{
                            background: e.tool === "aion_remember" ? "var(--accent-subtle)" : "var(--surface-2)",
                            color: e.tool === "aion_remember" ? "var(--gold-deep)" : "var(--text-2)",
                          }}
                        >
                          {toolLabel(e.tool)}
                        </span>
                        {!e.ok && (
                          <span className="text-[10px] ml-1.5" style={{ color: "#ef4444" }}>
                            error
                          </span>
                        )}
                      </td>
                      <td className="py-1.5 pr-3 max-w-[280px] truncate" style={{ color: "var(--text-2)" }} title={e.query}>
                        {e.query || "—"}
                      </td>
                      <td className="py-1.5 text-right text-xs whitespace-nowrap" style={{ color: "var(--text-3)" }}>
                        {fmtTokens(e.est_tokens)}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      </div>
    </AppShell>
  );
}
