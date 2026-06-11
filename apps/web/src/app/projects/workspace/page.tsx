"use client";

import { useEffect, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import AppShell from "@/components/AppShell";
import Icon from "@/components/Icon";
import {
  projectGet,
  projectSourceAdd,
  projectSourceToggle,
  projectSourceRemove,
  projectStudioGenerate,
  projectStudioRemove,
  chatStream,
  type Project,
  type ProjectSource,
  type ProjectOutput,
} from "@/lib/api";

type Msg = { role: "user" | "assistant"; text: string };

const STUDIO = [
  { kind: "resumen", label: "Resumen", icon: "memory" as const },
  { kind: "informe", label: "Informe", icon: "folder" as const },
  { kind: "mapa", label: "Mapa mental", icon: "graph" as const },
];

export default function ProjectWorkspace() {
  const router = useRouter();
  // Static export: el id viene por query (?id=…), leído en cliente.
  const [id, setId] = useState<string>("");

  const [project, setProject] = useState<Project | null>(null);
  const [sources, setSources] = useState<ProjectSource[]>([]);
  const [outputs, setOutputs] = useState<ProjectOutput[]>([]);
  const [notFound, setNotFound] = useState(false);

  // Fuentes
  const [adding, setAdding] = useState(false);
  const [srcKind, setSrcKind] = useState("nota");
  const [srcTitle, setSrcTitle] = useState("");
  const [srcContent, setSrcContent] = useState("");

  // Chat
  const [messages, setMessages] = useState<Msg[]>([]);
  const [input, setInput] = useState("");
  const [streaming, setStreaming] = useState(false);
  const endRef = useRef<HTMLDivElement>(null);

  // Studio
  const [generating, setGenerating] = useState<string | null>(null);
  const [viewing, setViewing] = useState<ProjectOutput | null>(null);

  useEffect(() => {
    setId(new URLSearchParams(window.location.search).get("id") ?? "");
  }, []);

  async function load() {
    const r = await projectGet(id);
    if (!r.ok || !r.project) {
      setNotFound(true);
      return;
    }
    setProject(r.project);
    setSources(r.sources ?? []);
    setOutputs(r.outputs ?? []);
  }
  useEffect(() => {
    if (id) load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages]);

  async function addSource() {
    const title = srcTitle.trim();
    if (!title) return;
    const r = await projectSourceAdd(id, title, srcKind, srcContent.trim());
    if (r.ok && r.source) {
      setSources((s) => [r.source!, ...s]);
      setSrcTitle("");
      setSrcContent("");
      setAdding(false);
    }
  }
  async function toggleSource(s: ProjectSource) {
    setSources((arr) => arr.map((x) => (x.id === s.id ? { ...x, active: !x.active } : x)));
    await projectSourceToggle(id, s.id, !s.active);
  }
  async function removeSource(sid: string) {
    setSources((arr) => arr.filter((x) => x.id !== sid));
    await projectSourceRemove(id, sid);
  }

  async function send() {
    const q = input.trim();
    if (!q || streaming) return;
    setInput("");
    setMessages((m) => [...m, { role: "user", text: q }, { role: "assistant", text: "" }]);
    setStreaming(true);
    await chatStream(
      q,
      false,
      (ev) => {
        if (ev.kind === "answer")
          setMessages((m) => {
            const copy = [...m];
            copy[copy.length - 1] = { role: "assistant", text: copy[copy.length - 1].text + ev.text };
            return copy;
          });
        else if (ev.kind === "error")
          setMessages((m) => {
            const copy = [...m];
            copy[copy.length - 1] = { role: "assistant", text: `⚠️ ${ev.text}` };
            return copy;
          });
      },
      `proj:${id}`,
      id,
    );
    setStreaming(false);
  }

  async function generate(kind: string) {
    if (generating) return;
    setGenerating(kind);
    const r = await projectStudioGenerate(id, kind);
    setGenerating(null);
    if (r.ok && r.output) {
      setOutputs((o) => [r.output!, ...o]);
      setViewing(r.output);
    } else if (r.error) {
      alert(r.error);
    }
  }
  async function removeOutput(oid: string) {
    setOutputs((o) => o.filter((x) => x.id !== oid));
    if (viewing?.id === oid) setViewing(null);
    await projectStudioRemove(id, oid);
  }

  if (notFound) {
    return (
      <AppShell title="Proyecto">
        <div className="flex flex-col items-center justify-center h-full gap-3" style={{ color: "var(--text-3)" }}>
          <p>Este proyecto no existe.</p>
          <button className="btn" onClick={() => router.push("/projects")}>
            Volver a Proyectos
          </button>
        </div>
      </AppShell>
    );
  }

  const activeCount = sources.filter((s) => s.active).length;

  return (
    <AppShell title={project?.name ?? "Proyecto"}>
      <div className="h-full flex">
        {/* ── FUENTES ─────────────────────────────────── */}
        <aside className="w-72 shrink-0 flex flex-col min-h-0" style={{ borderRight: "1px solid var(--border)" }}>
          <div className="flex items-center gap-2 px-4 h-12 shrink-0" style={{ borderBottom: "1px solid var(--border)" }}>
            <button onClick={() => router.push("/projects")} className="text-sm opacity-60 hover:opacity-100" title="Volver">
              ‹
            </button>
            <span className="text-xs font-semibold uppercase tracking-[0.12em]" style={{ color: "var(--text-3)" }}>
              Fuentes ({sources.length})
            </span>
            <button
              onClick={() => setAdding((a) => !a)}
              className="ml-auto rounded-md p-1 hover:opacity-100 opacity-70"
              style={{ background: "var(--surface-2)" }}
              title="Añadir fuente"
            >
              <Icon name="plus" size={14} />
            </button>
          </div>

          {adding && (
            <div className="p-3 flex flex-col gap-2" style={{ borderBottom: "1px solid var(--border)", background: "var(--surface-1)" }}>
              <div className="flex gap-1">
                {["nota", "texto", "web"].map((k) => (
                  <button
                    key={k}
                    onClick={() => setSrcKind(k)}
                    className="text-[11px] px-2 py-1 rounded-md capitalize"
                    style={{
                      background: srcKind === k ? "var(--accent)" : "var(--surface-2)",
                      color: srcKind === k ? "#04201f" : "var(--text-2)",
                    }}
                  >
                    {k}
                  </button>
                ))}
              </div>
              <input
                className="input text-sm"
                placeholder={srcKind === "web" ? "https://…" : "Título"}
                value={srcTitle}
                onChange={(e) => setSrcTitle(e.target.value)}
              />
              {srcKind !== "web" && (
                <textarea
                  className="input text-sm"
                  rows={4}
                  placeholder="Contenido…"
                  value={srcContent}
                  onChange={(e) => setSrcContent(e.target.value)}
                />
              )}
              <button className="btn text-sm" onClick={addSource}>
                Añadir
              </button>
            </div>
          )}

          <div className="flex-1 overflow-y-auto p-3 flex flex-col gap-2">
            {sources.length === 0 && (
              <p className="text-xs text-center mt-6" style={{ color: "var(--text-3)" }}>
                Añade documentos, notas o webs. El chat se basará en las activas.
              </p>
            )}
            {sources.map((s) => (
              <div
                key={s.id}
                className="rounded-lg p-2.5 flex items-start gap-2 group"
                style={{ background: "var(--surface-1)", border: "1px solid var(--border)", opacity: s.active ? 1 : 0.5 }}
              >
                <button
                  onClick={() => toggleSource(s)}
                  className="mt-0.5 w-4 h-4 rounded shrink-0 flex items-center justify-center text-[10px]"
                  style={{
                    background: s.active ? "var(--accent)" : "transparent",
                    border: s.active ? "none" : "1px solid var(--border)",
                    color: "#04201f",
                  }}
                  title={s.active ? "Activa (en uso)" : "Inactiva"}
                >
                  {s.active ? "✓" : ""}
                </button>
                <div className="min-w-0 flex-1">
                  <p className="text-sm truncate" style={{ color: "var(--text-1)" }}>{s.title}</p>
                  <p className="text-[10px] uppercase tracking-wide" style={{ color: "var(--text-3)" }}>{s.kind}</p>
                </div>
                <button
                  onClick={() => removeSource(s.id)}
                  className="text-xs opacity-0 group-hover:opacity-60 hover:!opacity-100"
                  style={{ color: "var(--text-3)" }}
                >
                  ✕
                </button>
              </div>
            ))}
          </div>
        </aside>

        {/* ── CHAT ─────────────────────────────────────── */}
        <section className="flex-1 flex flex-col min-w-0 min-h-0">
          <div className="flex-1 overflow-y-auto px-6 py-5 flex flex-col gap-4">
            {messages.length === 0 && (
              <div className="m-auto text-center max-w-sm" style={{ color: "var(--text-3)" }}>
                <span className="icon-chip mb-3 mx-auto" style={{ width: 48, height: 48, background: "var(--pastel-gold)", color: "var(--on-gold)" }}>
                  <Icon name="chat" size={24} />
                </span>
                <p className="text-sm">
                  Pregúntale a AION sobre este proyecto. Responderá basándose en las{" "}
                  <strong>{activeCount}</strong> fuente(s) activa(s) y su objetivo.
                </p>
              </div>
            )}
            {messages.map((m, i) => (
              <div key={i} className={m.role === "user" ? "self-end max-w-[80%]" : "self-start max-w-[85%]"}>
                <div
                  className="rounded-2xl px-4 py-2.5 text-sm whitespace-pre-wrap"
                  style={{
                    background: m.role === "user" ? "var(--ink)" : "var(--surface-1)",
                    color: m.role === "user" ? "#fff" : "var(--text-1)",
                    border: m.role === "user" ? "none" : "1px solid var(--border)",
                  }}
                >
                  {m.text || (streaming && i === messages.length - 1 ? "…" : "")}
                </div>
              </div>
            ))}
            <div ref={endRef} />
          </div>
          <form
            onSubmit={(e) => {
              e.preventDefault();
              send();
            }}
            className="px-6 py-4 flex gap-2 items-center shrink-0"
            style={{ borderTop: "1px solid var(--border)" }}
          >
            <input
              className="input flex-1"
              placeholder="Pregunta sobre el proyecto…"
              value={input}
              onChange={(e) => setInput(e.target.value)}
              disabled={streaming}
            />
            <button type="submit" className="btn shrink-0" disabled={streaming || !input.trim()}>
              {streaming ? "…" : "Enviar"}
            </button>
          </form>
        </section>

        {/* ── STUDIO ───────────────────────────────────── */}
        <aside className="w-80 shrink-0 flex flex-col min-h-0" style={{ borderLeft: "1px solid var(--border)" }}>
          <div className="flex items-center px-4 h-12 shrink-0" style={{ borderBottom: "1px solid var(--border)" }}>
            <span className="text-xs font-semibold uppercase tracking-[0.12em]" style={{ color: "var(--text-3)" }}>
              Studio
            </span>
          </div>
          <div className="p-3 grid grid-cols-1 gap-2" style={{ borderBottom: "1px solid var(--border)" }}>
            {STUDIO.map((it) => (
              <button
                key={it.kind}
                onClick={() => generate(it.kind)}
                disabled={!!generating}
                className="flex items-center gap-2.5 rounded-lg px-3 py-2.5 text-sm text-left transition-colors"
                style={{ background: "var(--surface-1)", border: "1px solid var(--border)", color: "var(--text-1)" }}
              >
                <Icon name={it.icon} size={16} />
                <span>{generating === it.kind ? "Generando…" : it.label}</span>
              </button>
            ))}
          </div>
          <div className="flex-1 overflow-y-auto p-3 flex flex-col gap-2">
            {outputs.length === 0 && (
              <p className="text-xs text-center mt-6" style={{ color: "var(--text-3)" }}>
                Las salidas que generes (resumen, informe, mapa) aparecerán aquí.
              </p>
            )}
            {outputs.map((o) => (
              <div
                key={o.id}
                onClick={() => setViewing(o)}
                className="rounded-lg p-2.5 cursor-pointer group"
                style={{ background: "var(--surface-1)", border: "1px solid var(--border)" }}
              >
                <div className="flex items-start gap-2">
                  <p className="text-sm flex-1 truncate" style={{ color: "var(--text-1)" }}>{o.title}</p>
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      removeOutput(o.id);
                    }}
                    className="text-xs opacity-0 group-hover:opacity-60 hover:!opacity-100"
                    style={{ color: "var(--text-3)" }}
                  >
                    ✕
                  </button>
                </div>
                <p className="text-[11px] mt-1" style={{ color: "var(--text-3)" }}>
                  {new Date(o.created).toLocaleString()}
                </p>
              </div>
            ))}
          </div>
        </aside>
      </div>

      {/* Visor de salida de Studio */}
      {viewing && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center p-8"
          style={{ background: "rgba(0,0,0,0.4)" }}
          onClick={() => setViewing(null)}
        >
          <div className="card max-w-2xl w-full max-h-[80vh] overflow-y-auto" onClick={(e) => e.stopPropagation()}>
            <div className="flex items-center gap-2 mb-3">
              <h3 className="font-display font-semibold flex-1">{viewing.title}</h3>
              <button onClick={() => setViewing(null)} className="opacity-60 hover:opacity-100">
                ✕
              </button>
            </div>
            <div className="text-sm whitespace-pre-wrap" style={{ color: "var(--text-1)" }}>
              {viewing.content}
            </div>
          </div>
        </div>
      )}
    </AppShell>
  );
}
