"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import { memoryRemember, memorySleep, memoryStats, type SleepReport } from "@/lib/api";

export default function MemoryPage() {
  const [count, setCount] = useState<number | null>(null);
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<SleepReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function refresh() {
    try {
      setCount((await memoryStats()).count);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : "error");
    }
  }
  useEffect(() => {
    refresh();
  }, []);

  async function remember() {
    if (!text.trim() || busy) return;
    setBusy(true);
    try {
      await memoryRemember(text.trim());
      setText("");
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : "error");
    } finally {
      setBusy(false);
    }
  }

  async function sleep() {
    setBusy(true);
    setReport(null);
    try {
      setReport(await memorySleep());
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : "error");
    } finally {
      setBusy(false);
    }
  }

  return (
    <main className="min-h-screen max-w-2xl mx-auto px-4 py-8">
      <header className="flex items-center gap-3 mb-8">
        <h1 className="font-display text-2xl font-bold">Memoria de AION</h1>
        <Link href="/chat" className="ml-auto text-sm" style={{ color: "var(--accent)" }}>
          ← Chat
        </Link>
      </header>

      <div className="card mb-6" style={{ boxShadow: "var(--shadow-elevated)" }}>
        <p className="text-sm" style={{ color: "var(--text-3)" }}>
          recuerdos en memoria de largo plazo
        </p>
        <p className="font-display text-5xl font-bold mt-1" style={{ color: "var(--accent)" }}>
          {count ?? "—"}
        </p>
      </div>

      <div className="card mb-6">
        <h2 className="t-section mb-3" style={{ color: "var(--text-2)" }}>
          Recordar algo nuevo
        </h2>
        <div className="flex gap-2">
          <input
            className="input"
            placeholder="Un hecho que AION debe recordar…"
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && remember()}
          />
          <button className="btn shrink-0" disabled={busy} onClick={remember}>
            Recordar
          </button>
        </div>
      </div>

      <div className="card">
        <h2 className="t-section mb-1" style={{ color: "var(--text-2)" }}>
          🌙 Sueño — consolidación darwiniana
        </h2>
        <p className="text-sm mb-3" style={{ color: "var(--text-3)" }}>
          Decae la aptitud, fusiona casi-duplicados y poda lo débil sin uso (con snapshot).
        </p>
        <button
          className="btn"
          disabled={busy}
          onClick={sleep}
          style={{ background: "var(--accent)", color: "var(--accent-contrast, #041314)" }}
        >
          {busy ? "consolidando…" : "Entrar en fase de sueño"}
        </button>
        {report && (
          <div className="mt-4 text-sm font-mono" style={{ color: "var(--text-2)" }}>
            {report.before} → {report.after} recuerdos · 🔗 {report.merged} fusionados · ✂️ {report.pruned} podados
          </div>
        )}
      </div>

      {error && (
        <p className="mt-4 text-sm" style={{ color: "#ef4444" }}>
          {error} — ¿está corriendo <code>aion-core serve</code>?
        </p>
      )}
    </main>
  );
}
