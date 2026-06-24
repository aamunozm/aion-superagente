"use client";

import { useEffect, useState } from "react";
import { AppShell, Icon, IconChip, Badge, Button, Input } from "@/components";
import {
  workflowsList,
  workflowsSet,
  workflowsRemove,
  workflowsRun,
  type Workflow,
  type WorkflowRun,
} from "@/lib/api";

// Herramientas EJECUTABLES en un flujo (deben coincidir con el registro seguro del
// backend: solo lectura/cálculo/investigación + agenda y contactos de solo-lectura).
// Un flujo nunca dispara acciones irreversibles sin el bucle HITL del agente.
const RUNNABLE: { tool: string; label: string }[] = [
  { tool: "calculator", label: "Calcular (aritmética)" },
  { tool: "web_search", label: "Buscar en internet" },
  { tool: "web_fetch", label: "Leer una URL" },
  { tool: "weather", label: "Clima" },
  { tool: "memory_search", label: "Buscar en memoria" },
  { tool: "remember", label: "Guardar recuerdo" },
  { tool: "library_search", label: "Buscar en biblioteca" },
  { tool: "graph_search", label: "Buscar en el grafo" },
  { tool: "files_list", label: "Listar archivos" },
  { tool: "file_read", label: "Leer un archivo" },
  { tool: "calendar_list", label: "Mirar la agenda" },
  { tool: "contacts_search", label: "Buscar contacto" },
];
const labelFor = (tool: string) => RUNNABLE.find((r) => r.tool === tool)?.label ?? tool;

function blankWorkflow(): Workflow {
  return {
    id: `wf_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 6)}`,
    name: "",
    description: "",
    trigger: { type: "manual" },
    steps: [{ tool: "calculator", input: "" }],
    enabled: true,
  };
}

export default function WorkflowsPage() {
  const [list, setList] = useState<Workflow[]>([]);
  const [draft, setDraft] = useState<Workflow | null>(null);
  const [runs, setRuns] = useState<Record<string, WorkflowRun>>({});
  const [busy, setBusy] = useState<string>("");
  const [err, setErr] = useState(false);

  async function load() {
    try {
      const r = await workflowsList();
      setList(r.workflows ?? []);
    } catch {
      setErr(true);
    }
  }
  useEffect(() => {
    load();
  }, []);

  async function save() {
    if (!draft || !draft.name.trim()) return;
    setBusy("save");
    try {
      await workflowsSet(draft);
      setDraft(null);
      await load();
    } finally {
      setBusy("");
    }
  }

  async function run(id: string) {
    setBusy(`run:${id}`);
    try {
      const r = await workflowsRun(id);
      setRuns((p) => ({ ...p, [id]: r }));
    } finally {
      setBusy("");
    }
  }

  async function del(id: string) {
    setBusy(`del:${id}`);
    try {
      await workflowsRemove(id);
      await load();
    } finally {
      setBusy("");
    }
  }

  // ── Edición del borrador ──
  const patch = (p: Partial<Workflow>) => setDraft((d) => (d ? { ...d, ...p } : d));
  const setStep = (i: number, p: Partial<{ tool: string; input: string }>) =>
    setDraft((d) => (d ? { ...d, steps: d.steps.map((s, j) => (j === i ? { ...s, ...p } : s)) } : d));
  const addStep = () =>
    setDraft((d) => (d ? { ...d, steps: [...d.steps, { tool: "calculator", input: "" }] } : d));
  const removeStep = (i: number) =>
    setDraft((d) => (d ? { ...d, steps: d.steps.filter((_, j) => j !== i) } : d));

  return (
    <AppShell title="Flujos de trabajo">
      <div className="max-w-6xl mx-auto px-3 py-6 flex flex-col gap-6">
        {/* ── CABECERA: qué son + recuento + crear (patrón de Mente) ── */}
        <div
          className="card flex flex-wrap items-center justify-between gap-4"
          style={{ boxShadow: "var(--shadow-elevated)" }}
        >
          <div className="flex items-center gap-4 min-w-0">
            <span
              className="w-12 h-12 rounded-2xl flex items-center justify-center shrink-0"
              style={{ background: "var(--accent-subtle)", color: "var(--gold-deep)" }}
            >
              <Icon name="network" size={24} />
            </span>
            <div className="min-w-0">
              <div className="font-display text-xl font-bold" style={{ color: "var(--text-1)" }}>
                Flujos de trabajo
              </div>
              <p className="text-sm mt-0.5 max-w-xl" style={{ color: "var(--text-3)" }}>
                Encadena herramientas de AION (estilo n8n): la salida de un paso alimenta al siguiente
                con <code>{"{{prev}}"}</code>. Solo usan herramientas de lectura/cálculo: nunca disparan
                acciones irreversibles sin tu OK.
              </p>
            </div>
          </div>
          <div className="flex items-center gap-5">
            {list.length > 0 && (
              <div className="min-w-0 text-right">
                <div className="font-display text-2xl font-bold leading-tight" style={{ color: "var(--text-1)" }}>
                  {list.length}
                </div>
                <div className="text-xs" style={{ color: "var(--text-2)" }}>
                  {list.length === 1 ? "flujo" : "flujos"}
                </div>
              </div>
            )}
            {!draft && (
              <Button variant="gold" onClick={() => setDraft(blankWorkflow())} className="shrink-0">
                <span className="inline-flex items-center gap-1.5">
                  <Icon name="plus" size={15} /> Nuevo flujo
                </span>
              </Button>
            )}
          </div>
        </div>

        {err && (
          <div className="card text-sm" style={{ color: "var(--text-2)", boxShadow: "var(--shadow-elevated)" }}>
            No pude leer los flujos. ¿Está AION en marcha (puerto 8765)?
          </div>
        )}

        {/* ── Editor ── */}
        {draft && (
          <div className="card mb-6" style={{ borderColor: "var(--accent)" }}>
            <div className="flex items-center gap-2 mb-3">
              <IconChip icon="network" tint="sky" />
              <h2 className="t-section" style={{ color: "var(--text-2)" }}>
                {list.some((w) => w.id === draft.id) ? "Editar flujo" : "Nuevo flujo"}
              </h2>
            </div>
            <Input
              placeholder="Nombre del flujo (p. ej. Resumen diario)"
              value={draft.name}
              onChange={(e) => patch({ name: e.target.value })}
              className="mb-2"
            />
            <Input
              placeholder="Descripción (opcional)"
              value={draft.description}
              onChange={(e) => patch({ description: e.target.value })}
              className="mb-4"
            />

            {/* Disparador */}
            <div className="flex items-center gap-2 mb-4 flex-wrap">
              <span className="text-xs" style={{ color: "var(--text-3)" }}>
                Se ejecuta:
              </span>
              {[
                { t: "manual", label: "Manual" },
                { t: "interval", label: "Cada X min" },
              ].map((o) => {
                const active = draft.trigger.type === o.t;
                return (
                  <button
                    key={o.t}
                    onClick={() =>
                      patch({
                        trigger: o.t === "interval" ? { type: "interval", minutes: 60 } : { type: "manual" },
                      })
                    }
                    className="text-xs px-3 py-1 rounded-full"
                    style={{
                      background: active ? "var(--accent-subtle)" : "var(--surface-2)",
                      color: active ? "var(--gold-deep)" : "var(--text-3)",
                      border: `1px solid ${active ? "var(--accent)" : "transparent"}`,
                    }}
                  >
                    {o.label}
                  </button>
                );
              })}
              {draft.trigger.type === "interval" && (
                <input
                  type="number"
                  min={5}
                  value={draft.trigger.minutes}
                  onChange={(e) =>
                    patch({ trigger: { type: "interval", minutes: Math.max(5, +e.target.value || 60) } })
                  }
                  className="input"
                  style={{ width: 90 }}
                />
              )}
            </div>

            {/* Pasos */}
            <div className="flex flex-col gap-2 mb-3">
              {draft.steps.map((s, i) => (
                <div
                  key={i}
                  className="rounded-xl p-3 flex flex-col gap-2"
                  style={{ background: "var(--surface-1)", border: "1px solid var(--border)" }}
                >
                  <div className="flex items-center gap-2">
                    <span
                      className="w-6 h-6 rounded-full flex items-center justify-center text-[11px] font-bold shrink-0"
                      style={{ background: "var(--accent-subtle)", color: "var(--gold-deep)" }}
                    >
                      {i + 1}
                    </span>
                    <select
                      className="input"
                      value={s.tool}
                      onChange={(e) => setStep(i, { tool: e.target.value })}
                      style={{ background: "var(--surface-1)", color: "var(--text-1)" }}
                    >
                      {RUNNABLE.map((r) => (
                        <option key={r.tool} value={r.tool}>
                          {r.label}
                        </option>
                      ))}
                    </select>
                    {draft.steps.length > 1 && (
                      <button
                        onClick={() => removeStep(i)}
                        className="shrink-0 opacity-50 hover:opacity-100"
                        style={{ color: "#ef4444" }}
                        aria-label="Quitar paso"
                      >
                        <Icon name="trash" size={15} />
                      </button>
                    )}
                  </div>
                  <Input
                    placeholder={i === 0 ? "Entrada (p. ej. 12*9)" : "Entrada · usa {{prev}} para la salida anterior"}
                    value={s.input}
                    onChange={(e) => setStep(i, { input: e.target.value })}
                  />
                </div>
              ))}
            </div>
            <button
              onClick={addStep}
              className="text-xs inline-flex items-center gap-1.5 mb-4"
              style={{ color: "var(--gold-deep)" }}
            >
              <Icon name="plus" size={13} /> Añadir paso
            </button>

            <div className="flex items-center gap-2">
              <Button onClick={save} disabled={busy === "save" || !draft.name.trim()}>
                {busy === "save" ? "Guardando…" : "Guardar flujo"}
              </Button>
              <Button variant="subtle" onClick={() => setDraft(null)}>
                Cancelar
              </Button>
            </div>
          </div>
        )}

        {/* ── Lista de flujos ── */}
        <div className="flex flex-col gap-3">
          {list.length === 0 && !draft && (
            <div className="card text-sm text-center" style={{ color: "var(--text-3)" }}>
              <div className="flex justify-center mb-2">
                <IconChip icon="network" tint="sky" />
              </div>
              Aún no hay flujos. Crea el primero con «Nuevo flujo».
            </div>
          )}

          {list.map((w) => {
            const r = runs[w.id];
            return (
              <div key={w.id} className="card">
                <div className="flex items-center gap-3">
                  <IconChip icon="network" tint="sky" />
                  <div className="min-w-0 flex-1">
                    <div className="font-display font-semibold text-[15px] truncate">{w.name}</div>
                    <div className="flex items-center gap-2 mt-0.5">
                      <Badge tone="neutral">
                        {w.trigger.type === "interval" ? `cada ${w.trigger.minutes} min` : "manual"}
                      </Badge>
                      <span className="text-xs" style={{ color: "var(--text-3)" }}>
                        {w.steps.length} {w.steps.length === 1 ? "paso" : "pasos"}
                      </span>
                    </div>
                  </div>
                  <Button size="sm" onClick={() => run(w.id)} disabled={busy === `run:${w.id}`}>
                    <span className="inline-flex items-center gap-1.5">
                      <Icon name="play" size={13} /> {busy === `run:${w.id}` ? "…" : "Ejecutar"}
                    </span>
                  </Button>
                  <Button size="sm" variant="subtle" onClick={() => setDraft(w)}>
                    Editar
                  </Button>
                  <button
                    onClick={() => del(w.id)}
                    className="shrink-0 opacity-50 hover:opacity-100"
                    style={{ color: "#ef4444" }}
                    aria-label="Eliminar flujo"
                  >
                    <Icon name="trash" size={16} />
                  </button>
                </div>

                {w.description && (
                  <p className="text-xs mt-2" style={{ color: "var(--text-3)" }}>
                    {w.description}
                  </p>
                )}

                {/* Resultado de la última ejecución */}
                {r && (
                  <div className="mt-3 pt-3" style={{ borderTop: "1px solid var(--border)" }}>
                    <div className="flex items-center gap-2 mb-2">
                      <Badge tone={r.ok ? "success" : r.stopped_for_approval ? "warn" : "danger"}>
                        {r.ok ? "✓ completado" : r.stopped_for_approval ? "pausado: requiere tu OK" : "falló"}
                      </Badge>
                    </div>
                    <div className="flex flex-col gap-1.5">
                      {r.steps.map((s, i) => (
                        <div key={i} className="text-xs flex gap-2" style={{ color: "var(--text-2)" }}>
                          <span style={{ color: s.ok ? "var(--on-mint)" : "#ef4444" }}>
                            <Icon name={s.ok ? "check" : "warn"} size={13} />
                          </span>
                          <span className="font-mono shrink-0" style={{ color: "var(--text-3)" }}>
                            {labelFor(s.tool)}
                          </span>
                          <span className="truncate">→ {s.output}</span>
                        </div>
                      ))}
                    </div>
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </div>
    </AppShell>
  );
}
