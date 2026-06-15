"use client";

import { useEffect, useMemo, useState } from "react";
import AppShell from "@/components/AppShell";
import Icon from "@/components/Icon";
import Markdown from "@/components/Markdown";
import { skillsList, skillSave, skillDelete, type Skill } from "@/lib/api";

const EMPTY: Skill = { name: "", description: "", when_to_use: "", category: "", tools: [], body: "" };

export default function SkillsPage() {
  const [skills, setSkills] = useState<Skill[]>([]);
  const [sel, setSel] = useState<Skill | null>(null);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState<Skill>(EMPTY);
  const [toolsStr, setToolsStr] = useState("");
  const [msg, setMsg] = useState("");
  const [busy, setBusy] = useState(false);

  async function load(selectName?: string) {
    const r = await skillsList().catch(() => ({ skills: [] as Skill[] }));
    setSkills(r.skills);
    if (selectName) {
      const found = r.skills.find((s) => s.name === selectName);
      if (found) setSel(found);
    }
  }
  useEffect(() => {
    load();
  }, []);

  // Agrupa por categoría para la lista lateral.
  const byCategory = useMemo(() => {
    const m: Record<string, Skill[]> = {};
    for (const s of skills) (m[s.category || "otros"] ??= []).push(s);
    return Object.entries(m).sort(([a], [b]) => a.localeCompare(b));
  }, [skills]);

  function startEdit(s: Skill) {
    setDraft({ ...s });
    setToolsStr(s.tools.join(", "));
    setEditing(true);
    setMsg("");
  }
  function startNew() {
    setSel(null);
    setDraft({ ...EMPTY });
    setToolsStr("");
    setEditing(true);
    setMsg("");
  }
  async function save() {
    if (busy) return;
    if (!draft.name.trim()) return setMsg("⚠️ La skill necesita un nombre.");
    if (!draft.body.trim()) return setMsg("⚠️ La skill necesita instrucciones.");
    setBusy(true);
    const tools = toolsStr.split(",").map((t) => t.trim()).filter(Boolean);
    const r = await skillSave({ ...draft, tools }).catch(() => ({ ok: false, error: "sin respuesta" }));
    setBusy(false);
    if (r.ok) {
      setMsg("✅ Guardada.");
      setEditing(false);
      await load(draft.name);
    } else {
      setMsg(`⚠️ ${r.error ?? "no se pudo guardar"}`);
    }
  }
  async function remove(s: Skill) {
    if (!confirm(`¿Borrar la skill «${s.name}»?`)) return;
    await skillDelete(s.name).catch(() => {});
    setSel(null);
    setEditing(false);
    await load();
  }

  return (
    <AppShell title="Skills">
      <div className="max-w-6xl mx-auto px-6 py-8">
        <div className="flex items-center justify-between mb-1">
          <h1 className="t-title flex items-center gap-2" style={{ color: "var(--text-1)" }}>
            <Icon name="bulb" size={20} /> Skills
          </h1>
          <button className="btn" onClick={startNew} style={{ background: "var(--ink)", color: "#fff" }}>
            + Nueva skill
          </button>
        </div>
        <p className="text-sm mb-5" style={{ color: "var(--text-3)" }}>
          Playbooks que AION sabe ejecutar: procedimientos que el agente descubre y sigue, componiendo sus
          herramientas. {skills.length} skills en {byCategory.length} categorías.
        </p>

        <div className="grid grid-cols-1 md:grid-cols-[260px_1fr] gap-5">
          {/* Lista por categoría */}
          <div className="flex flex-col gap-4">
            {byCategory.map(([cat, items]) => (
              <div key={cat}>
                <div className="text-[11px] uppercase tracking-wide mb-1" style={{ color: "var(--text-3)" }}>
                  {cat}
                </div>
                <div className="flex flex-col gap-1">
                  {items.map((s) => (
                    <button
                      key={s.name}
                      onClick={() => { setSel(s); setEditing(false); setMsg(""); }}
                      className="text-left px-3 py-1.5 rounded-lg text-sm transition-all"
                      style={{
                        background: sel?.name === s.name ? "var(--accent-subtle)" : "transparent",
                        color: sel?.name === s.name ? "var(--gold-deep)" : "var(--text-2)",
                        fontWeight: sel?.name === s.name ? 600 : 500,
                      }}
                    >
                      {s.name}
                    </button>
                  ))}
                </div>
              </div>
            ))}
          </div>

          {/* Detalle / editor */}
          <div className="card">
            {editing ? (
              <div className="flex flex-col gap-3">
                <h2 className="t-section" style={{ color: "var(--text-2)" }}>
                  {draft.name ? `Editar «${draft.name}»` : "Nueva skill"}
                </h2>
                <Field label="Nombre (kebab-case)">
                  <input className="input" value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} placeholder="mi-skill" autoComplete="off" />
                </Field>
                <Field label="Descripción (una línea)">
                  <input className="input" value={draft.description} onChange={(e) => setDraft({ ...draft, description: e.target.value })} autoComplete="off" />
                </Field>
                <Field label="Cuándo usarla (pistas de disparo)">
                  <input className="input" value={draft.when_to_use} onChange={(e) => setDraft({ ...draft, when_to_use: e.target.value })} autoComplete="off" />
                </Field>
                <div className="grid grid-cols-2 gap-3">
                  <Field label="Categoría">
                    <input className="input" value={draft.category} onChange={(e) => setDraft({ ...draft, category: e.target.value })} placeholder="sistema, negocio…" autoComplete="off" />
                  </Field>
                  <Field label="Herramientas (coma)">
                    <input className="input" value={toolsStr} onChange={(e) => setToolsStr(e.target.value)} placeholder="run_command, file_write" autoComplete="off" />
                  </Field>
                </div>
                <Field label="Instrucciones (Markdown)">
                  <textarea className="input font-mono text-sm" rows={14} value={draft.body} onChange={(e) => setDraft({ ...draft, body: e.target.value })} />
                </Field>
                <div className="flex items-center gap-2">
                  <button className="btn" disabled={busy} onClick={save} style={{ background: "var(--ink)", color: "#fff", opacity: busy ? 0.5 : 1 }}>
                    {busy ? "Guardando…" : "Guardar"}
                  </button>
                  <button className="btn" onClick={() => { setEditing(false); setMsg(""); }} style={{ background: "var(--surface-1)" }}>
                    Cancelar
                  </button>
                  {msg && <span className="text-sm" style={{ color: "var(--accent)" }}>{msg}</span>}
                </div>
              </div>
            ) : sel ? (
              <div>
                <div className="flex items-start justify-between gap-3 mb-2">
                  <div>
                    <h2 className="t-section" style={{ color: "var(--text-1)" }}>{sel.name}</h2>
                    <p className="text-sm" style={{ color: "var(--text-3)" }}>{sel.description}</p>
                  </div>
                  <div className="flex gap-2 shrink-0">
                    <button className="btn" onClick={() => startEdit(sel)} style={{ background: "var(--surface-1)" }}>Editar</button>
                    <button className="btn" onClick={() => remove(sel)} style={{ background: "var(--surface-1)", color: "#dc2626" }}>Borrar</button>
                  </div>
                </div>
                <div className="flex flex-wrap gap-1.5 mb-3">
                  {sel.category && <Tag>{sel.category}</Tag>}
                  {sel.tools.map((tname) => <Tag key={tname} mono>{tname}</Tag>)}
                </div>
                {sel.when_to_use && (
                  <p className="text-xs mb-3 px-3 py-2 rounded-lg" style={{ background: "var(--surface-1)", color: "var(--text-3)" }}>
                    <strong>Cuándo:</strong> {sel.when_to_use}
                  </p>
                )}
                <Markdown>{sel.body}</Markdown>
              </div>
            ) : (
              <div className="text-sm py-12 text-center" style={{ color: "var(--text-3)" }}>
                Elige una skill de la lista, o crea una nueva.
              </div>
            )}
          </div>
        </div>
      </div>
    </AppShell>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="flex flex-col gap-1">
      <span className="text-xs" style={{ color: "var(--text-3)" }}>{label}</span>
      {children}
    </label>
  );
}
function Tag({ children, mono }: { children: React.ReactNode; mono?: boolean }) {
  return (
    <span className={`text-[11px] px-2 py-0.5 rounded-full ${mono ? "font-mono" : ""}`} style={{ background: "var(--surface-2)", color: "var(--text-2)" }}>
      {children}
    </span>
  );
}
