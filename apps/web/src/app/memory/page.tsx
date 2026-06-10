"use client";

import { useEffect, useRef, useState } from "react";
import AppShell from "@/components/AppShell";
import Icon from "@/components/Icon";
import {
  libraryAsk,
  libraryList,
  libraryRemove,
  libraryUpload,
  memoryExport,
  memoryImport,
  memoryRemember,
  memorySleep,
  memoryStats,
  type LibraryDoc,
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

  // ── Biblioteca (Academias) ──
  const [docs, setDocs] = useState<LibraryDoc[]>([]);
  const [domain, setDomain] = useState("documentos");
  const [progress, setProgress] = useState<{ done: number; total: number; name: string } | null>(null);
  const [dragging, setDragging] = useState(false);
  const [libMsg, setLibMsg] = useState<string | null>(null);
  const [ask, setAsk] = useState("");
  const [askAnswer, setAskAnswer] = useState<string | null>(null);
  const [askBusy, setAskBusy] = useState(false);
  const booksInput = useRef<HTMLInputElement>(null);

  async function refresh() {
    try {
      setCount((await memoryStats()).count);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : "error");
    }
  }
  async function refreshLibrary() {
    try {
      setDocs((await libraryList()).documents);
    } catch {
      /* la biblioteca puede estar vacía */
    }
  }
  useEffect(() => {
    refresh();
    refreshLibrary();
  }, []);

  function readAsBase64(file: File): Promise<string> {
    return new Promise((resolve, reject) => {
      const r = new FileReader();
      r.onload = () => resolve(String(r.result).split(",")[1] ?? "");
      r.onerror = () => reject(new Error("no pude leer el archivo"));
      r.readAsDataURL(file);
    });
  }

  // Subida MASIVA: ingiere muchos libros en cola, con progreso.
  async function uploadBooks(files: FileList | File[]) {
    const list = Array.from(files);
    if (list.length === 0 || progress) return;
    const dom = domain.trim() || "documentos";
    setLibMsg(null);
    let totalPassages = 0;
    let okCount = 0;
    const errors: string[] = [];
    for (let i = 0; i < list.length; i++) {
      const f = list[i];
      setProgress({ done: i, total: list.length, name: f.name });
      try {
        const b64 = await readAsBase64(f);
        const r = await libraryUpload(dom, f.name, b64);
        totalPassages += r.passages;
        okCount++;
      } catch (e) {
        errors.push(`${f.name}: ${e instanceof Error ? e.message : "error"}`);
      }
    }
    setProgress(null);
    await refreshLibrary();
    setLibMsg(
      `✅ ${okCount}/${list.length} libros indexados en «${dom}» · ${totalPassages} pasajes` +
        (errors.length ? ` · ⚠️ ${errors.length} con error` : ""),
    );
  }

  async function removeDoc(d: LibraryDoc) {
    if (!confirm(`¿Eliminar «${d.source}» de la biblioteca?`)) return;
    try {
      await libraryRemove(d.domain, d.source);
      await refreshLibrary();
    } catch (e) {
      setLibMsg(e instanceof Error ? e.message : "error");
    }
  }

  async function askLibrary() {
    if (!ask.trim() || askBusy) return;
    setAskBusy(true);
    setAskAnswer(null);
    try {
      const r = await libraryAsk(ask.trim());
      const cites = r.sources.map((s) => `[${s.n}] ${s.source}`).join("  ");
      setAskAnswer(`${r.answer}\n\n${cites}`);
    } catch (e) {
      setAskAnswer(`⚠️ ${e instanceof Error ? e.message : "error"}`);
    } finally {
      setAskBusy(false);
    }
  }

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
      <div className="max-w-3xl mx-auto px-8 py-8">

      <div className="card mb-6" style={{ boxShadow: "var(--shadow-elevated)" }}>
        <p className="text-sm" style={{ color: "var(--text-3)" }}>
          recuerdos en memoria de largo plazo
        </p>
        <p className="font-display text-5xl font-bold mt-1" style={{ color: "var(--accent)" }}>
          {count ?? "—"}
        </p>
      </div>

      <div className="card mb-6" style={{ boxShadow: "var(--shadow-elevated)" }}>
        <h2 className="t-section mb-1 flex items-center gap-2" style={{ color: "var(--text-2)" }}>
          <Icon name="folder" size={16} /> Biblioteca — subir libros y documentos
        </h2>
        <p className="text-sm mb-3" style={{ color: "var(--text-3)" }}>
          Sube muchos libros a la vez (PDF, TXT, MD). AION los trocea, los entiende en
          cualquier idioma y responde citando la fuente. {docs.length > 0 && (
            <span>· {docs.length} documentos · {docs.reduce((a, d) => a + d.chunks, 0)} pasajes</span>
          )}
        </p>

        <div className="flex gap-2 items-center mb-3">
          <label className="text-sm shrink-0" style={{ color: "var(--text-2)" }}>Dominio:</label>
          <input
            className="input"
            style={{ maxWidth: 220 }}
            value={domain}
            onChange={(e) => setDomain(e.target.value)}
            placeholder="p. ej. derecho, ciencia, negocios"
          />
        </div>

        <div
          onDragOver={(e) => { e.preventDefault(); setDragging(true); }}
          onDragLeave={() => setDragging(false)}
          onDrop={(e) => { e.preventDefault(); setDragging(false); if (e.dataTransfer.files.length) uploadBooks(e.dataTransfer.files); }}
          onClick={() => booksInput.current?.click()}
          className="rounded-xl p-6 text-center cursor-pointer transition-colors"
          style={{
            border: `2px dashed ${dragging ? "var(--accent)" : "var(--border)"}`,
            background: dragging ? "var(--accent-subtle)" : "var(--surface-1)",
          }}
        >
          <div className="flex flex-col items-center gap-1.5" style={{ color: "var(--text-2)" }}>
            <Icon name="upload" size={22} />
            <span className="text-sm font-medium">Arrastra tus libros aquí o haz clic para elegir</span>
            <span className="text-xs" style={{ color: "var(--text-3)" }}>varios archivos a la vez · PDF · TXT · MD</span>
          </div>
        </div>
        <input
          ref={booksInput}
          type="file"
          multiple
          accept=".pdf,.txt,.md,.markdown"
          className="hidden"
          onChange={(e) => { if (e.target.files?.length) uploadBooks(e.target.files); e.target.value = ""; }}
        />

        {progress && (
          <div className="mt-3">
            <div className="flex justify-between text-xs mb-1" style={{ color: "var(--text-3)" }}>
              <span>Indexando: {progress.name}</span>
              <span>{progress.done}/{progress.total}</span>
            </div>
            <div className="h-1.5 rounded-full overflow-hidden" style={{ background: "var(--surface-2)" }}>
              <div className="h-full rounded-full" style={{ width: `${(progress.done / progress.total) * 100}%`, background: "var(--accent)", transition: "width .3s" }} />
            </div>
          </div>
        )}
        {libMsg && <p className="mt-3 text-sm" style={{ color: "var(--accent)" }}>{libMsg}</p>}

        {docs.length > 0 && (
          <div className="mt-4">
            {Object.entries(docs.reduce<Record<string, LibraryDoc[]>>((acc, d) => {
              (acc[d.domain] ??= []).push(d); return acc;
            }, {})).map(([dom, items]) => (
              <div key={dom} className="mb-3">
                <p className="text-xs font-semibold uppercase tracking-wide mb-1.5" style={{ color: "var(--text-3)" }}>{dom}</p>
                <div className="flex flex-col gap-1.5">
                  {items.map((d) => (
                    <div key={d.domain + d.source} className="flex items-center gap-2 px-3 py-2 rounded-lg" style={{ background: "var(--surface-1)" }}>
                      <Icon name="file" size={15} />
                      <span className="text-sm flex-1 truncate">{d.source}</span>
                      <span className="text-xs" style={{ color: "var(--text-3)" }}>{d.chunks} pasajes</span>
                      <button onClick={() => removeDoc(d)} className="text-xs opacity-60 hover:opacity-100" style={{ color: "#ef4444" }} title="Eliminar">✕</button>
                    </div>
                  ))}
                </div>
              </div>
            ))}

            <div className="flex gap-2 mt-3">
              <input
                className="input"
                placeholder="Pregunta a tus libros…"
                value={ask}
                onChange={(e) => setAsk(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && askLibrary()}
              />
              <button className="btn shrink-0" disabled={askBusy} onClick={askLibrary}>
                {askBusy ? "…" : "Preguntar"}
              </button>
            </div>
            {askAnswer && (
              <p className="mt-3 text-sm whitespace-pre-wrap" style={{ color: "var(--text-2)" }}>{askAnswer}</p>
            )}
          </div>
        )}
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
