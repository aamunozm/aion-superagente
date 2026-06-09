"use client";

import { useEffect, useRef, useState } from "react";
import AppShell from "@/components/AppShell";
import Icon from "@/components/Icon";
import {
  memoryExport,
  memoryImport,
  memoryRemember,
  memorySleep,
  memoryStats,
  type SleepReport,
} from "@/lib/api";

export default function MemoryPage() {
  const [count, setCount] = useState<number | null>(null);
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<SleepReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [transfer, setTransfer] = useState<string | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);

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

  async function exportMemory() {
    setTransfer(null);
    setError(null);
    try {
      const res = await memoryExport();
      const blob = await res.blob();
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = "aion-memory.jsonl";
      a.click();
      URL.revokeObjectURL(url);
      setTransfer("Memoria descargada como aion-memory.jsonl");
    } catch (e) {
      setError(e instanceof Error ? e.message : "error");
    }
  }

  async function importMemory(file: File) {
    setTransfer(null);
    setError(null);
    setBusy(true);
    try {
      const jsonl = await file.text();
      const r = await memoryImport(jsonl);
      setTransfer(`Importados ${r.added} recuerdos nuevos · total ${r.count}`);
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
    <AppShell title="Memoria">
      <div className="max-w-2xl mx-auto px-4 py-8">

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

      <div className="card mb-6">
        <h2 className="t-section mb-1 flex items-center gap-2" style={{ color: "var(--text-2)" }}>
          <Icon name="refresh" size={16} /> Transferir memoria
        </h2>
        <p className="text-sm mb-3" style={{ color: "var(--text-3)" }}>
          Descarga la memoria en un archivo para llevarla a otro PC/Mac, o súbela
          aquí para importarla (fusiona, sin duplicar).
        </p>
        <div className="flex gap-2">
          <button className="btn shrink-0 inline-flex items-center gap-2" disabled={busy} onClick={exportMemory}>
            <Icon name="download" size={16} /> Descargar memoria
          </button>
          <button
            className="btn shrink-0 inline-flex items-center gap-2"
            disabled={busy}
            onClick={() => fileInput.current?.click()}
          >
            <Icon name="upload" size={16} /> Subir memoria
          </button>
          <input
            ref={fileInput}
            type="file"
            accept=".jsonl,application/x-ndjson,application/json,text/plain"
            className="hidden"
            onChange={(e) => {
              const f = e.target.files?.[0];
              if (f) importMemory(f);
              e.target.value = "";
            }}
          />
        </div>
        {transfer && (
          <p className="mt-3 text-sm" style={{ color: "var(--accent)" }}>
            {transfer}
          </p>
        )}
      </div>

      <div className="card">
        <h2 className="t-section mb-1 flex items-center gap-2" style={{ color: "var(--text-2)" }}>
          <Icon name="moon" size={16} /> Sueño — consolidación darwiniana
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
            {report.before} → {report.after} recuerdos · {report.merged} fusionados · {report.pruned} podados
          </div>
        )}
      </div>

      {error && (
        <p className="mt-4 text-sm" style={{ color: "#ef4444" }}>
          {error} — ¿está corriendo <code>aion-core serve</code>?
        </p>
      )}
      </div>
    </AppShell>
  );
}
