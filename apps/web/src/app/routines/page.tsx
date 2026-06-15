"use client";

import { useEffect, useState } from "react";
import AppShell from "@/components/AppShell";
import Icon from "@/components/Icon";
import Markdown from "@/components/Markdown";
import { routinesList, routineSave, routineDelete, routineRun, type Routine } from "@/lib/api";

const EMPTY = { title: "", prompt: "", time: "09:00", enabled: true };

export default function RoutinesPage() {
  const [routines, setRoutines] = useState<Routine[]>([]);
  const [draft, setDraft] = useState<typeof EMPTY & { id?: string }>(EMPTY);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState("");
  const [runMsg, setRunMsg] = useState<{ id: string; text: string } | null>(null);
  const [running, setRunning] = useState("");

  async function load() {
    const r = await routinesList().catch(() => ({ routines: [] as Routine[] }));
    setRoutines(r.routines);
  }
  useEffect(() => {
    load();
  }, []);

  async function save() {
    if (busy) return;
    if (!draft.title.trim() || !draft.prompt.trim() || !draft.time.trim())
      return setMsg("⚠️ Completa título, tarea y hora.");
    setBusy(true);
    const r = await routineSave(draft).catch(() => ({ ok: false, error: "sin respuesta" }));
    setBusy(false);
    if (r.ok) {
      setMsg("✅ Rutina guardada.");
      setDraft(EMPTY);
      await load();
    } else setMsg(`⚠️ ${r.error ?? "error"}`);
  }
  async function remove(id: string) {
    if (!confirm("¿Borrar esta rutina?")) return;
    await routineDelete(id).catch(() => {});
    await load();
  }
  async function toggle(rt: Routine) {
    await routineSave({ ...rt, enabled: !rt.enabled }).catch(() => {});
    await load();
  }
  async function runNow(rt: Routine) {
    if (running) return;
    setRunning(rt.id);
    setRunMsg(null);
    const r = await routineRun(rt.id).catch(() => ({ ok: false, error: "sin respuesta" }) as { ok: boolean; answer?: string; error?: string });
    setRunning("");
    setRunMsg({ id: rt.id, text: r.ok ? r.answer ?? "(sin respuesta)" : `⚠️ ${r.error ?? "error"}` });
    await load();
  }

  return (
    <AppShell title="Rutinas">
      <div className="max-w-5xl mx-auto px-6 py-8">
        <h1 className="t-title flex items-center gap-2" style={{ color: "var(--text-1)" }}>
          <Icon name="clock" size={20} /> Rutinas
        </h1>
        <p className="text-sm mb-5" style={{ color: "var(--text-3)" }}>
          Tareas que AION ejecuta <strong>solo</strong>, cada día a la hora que elijas, y te deja el
          resultado en la Bandeja. Tú la programas → autorizas sus acciones; las peligrosas (sudo, borrados,
          pagos) siguen bloqueadas.
        </p>

        {/* Crear */}
        <div className="card mb-6">
          <h2 className="t-section mb-3" style={{ color: "var(--text-2)" }}>
            {draft.id ? "Editar rutina" : "Nueva rutina"}
          </h2>
          <div className="flex flex-col gap-3">
            <label className="flex flex-col gap-1">
              <span className="text-xs" style={{ color: "var(--text-3)" }}>Título</span>
              <input className="input" value={draft.title} placeholder="Escaneo diario del Mac" onChange={(e) => setDraft({ ...draft, title: e.target.value })} />
            </label>
            <label className="flex flex-col gap-1">
              <span className="text-xs" style={{ color: "var(--text-3)" }}>Qué debe hacer (en tus palabras)</span>
              <textarea className="input" rows={3} value={draft.prompt} placeholder="Haz un escaneo de salud de mi Mac y avísame si algo está en 🔴 o 🟡." onChange={(e) => setDraft({ ...draft, prompt: e.target.value })} />
            </label>
            <div className="flex items-end gap-4">
              <label className="flex flex-col gap-1">
                <span className="text-xs" style={{ color: "var(--text-3)" }}>Cada día a las</span>
                <input className="input" type="time" value={draft.time} onChange={(e) => setDraft({ ...draft, time: e.target.value })} style={{ width: 140 }} />
              </label>
              <label className="flex items-center gap-2 text-sm pb-2" style={{ color: "var(--text-2)" }}>
                <input type="checkbox" checked={draft.enabled} onChange={(e) => setDraft({ ...draft, enabled: e.target.checked })} />
                Activa
              </label>
              <button className="btn ml-auto" disabled={busy} onClick={save} style={{ background: "var(--ink)", color: "#fff", opacity: busy ? 0.5 : 1 }}>
                {busy ? "Guardando…" : draft.id ? "Guardar cambios" : "Crear rutina"}
              </button>
            </div>
            {msg && <p className="text-sm" style={{ color: "var(--accent)" }}>{msg}</p>}
          </div>
        </div>

        {/* Lista */}
        {routines.length === 0 ? (
          <p className="text-sm text-center py-8" style={{ color: "var(--text-3)" }}>
            Aún no tienes rutinas. Crea la primera arriba.
          </p>
        ) : (
          <div className="flex flex-col gap-3">
            {routines.map((rt) => (
              <div key={rt.id} className="card">
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="font-medium" style={{ color: "var(--text-1)" }}>{rt.title}</span>
                      <span className="text-[11px] px-2 py-0.5 rounded-full" style={{ background: "var(--surface-2)", color: "var(--text-2)" }}>
                        🕘 {rt.time}
                      </span>
                      <span className="text-[11px] px-2 py-0.5 rounded-full" style={{ background: rt.enabled ? "var(--accent-subtle)" : "var(--surface-2)", color: rt.enabled ? "var(--gold-deep)" : "var(--text-3)" }}>
                        {rt.enabled ? "activa" : "pausada"}
                      </span>
                    </div>
                    <p className="text-sm mt-1" style={{ color: "var(--text-3)" }}>{rt.prompt}</p>
                    {rt.last_run && <p className="text-[11px] mt-1" style={{ color: "var(--text-3)" }}>última vez: {rt.last_run}</p>}
                  </div>
                  <div className="flex gap-2 shrink-0">
                    <button className="btn" disabled={running === rt.id} onClick={() => runNow(rt)} style={{ background: "var(--surface-1)", opacity: running === rt.id ? 0.5 : 1 }}>
                      {running === rt.id ? "Ejecutando…" : "Ejecutar ahora"}
                    </button>
                    <button className="btn" onClick={() => toggle(rt)} style={{ background: "var(--surface-1)" }}>{rt.enabled ? "Pausar" : "Activar"}</button>
                    <button className="btn" onClick={() => remove(rt.id)} style={{ background: "var(--surface-1)", color: "#dc2626" }}>Borrar</button>
                  </div>
                </div>
                {runMsg?.id === rt.id && (
                  <div className="mt-3 pt-3" style={{ borderTop: "1px solid var(--border)" }}>
                    <p className="text-[11px] mb-1" style={{ color: "var(--text-3)" }}>Resultado (también en tu Bandeja):</p>
                    <Markdown>{runMsg.text}</Markdown>
                  </div>
                )}
              </div>
            ))}
          </div>
        )}
      </div>
    </AppShell>
  );
}
