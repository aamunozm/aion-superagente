"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import AppShell from "@/components/AppShell";
import Icon from "@/components/Icon";
import { projectsList, projectCreate, projectRemove, projectUpdate, type Project } from "@/lib/api";

/** Acentos suaves para diferenciar proyectos de un vistazo (rotan por índice). */
const ACCENTS = ["var(--pastel-gold)", "var(--pastel-teal, #d7f0ee)", "var(--pastel-rose, #f3e0e4)", "var(--pastel-blue, #e0e7f3)"];

export default function ProjectsPage() {
  const router = useRouter();
  const [projects, setProjects] = useState<Project[]>([]);
  const [name, setName] = useState("");
  const [desc, setDesc] = useState("");
  const [busy, setBusy] = useState(false);
  // Edición inline: id del proyecto en edición + borradores de nombre/descripción.
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editName, setEditName] = useState("");
  const [editDesc, setEditDesc] = useState("");
  const [savingEdit, setSavingEdit] = useState(false);

  async function refresh() {
    setProjects(await projectsList());
  }
  useEffect(() => {
    refresh();
  }, []);

  async function add() {
    if (!name.trim() || busy) return;
    setBusy(true);
    const r = await projectCreate(name.trim(), desc.trim(), "");
    setBusy(false);
    if (r.ok && r.project) {
      setName("");
      setDesc("");
      router.push(`/projects/workspace?id=${r.project.id}`);
    }
  }
  async function remove(e: React.MouseEvent, id: string) {
    e.stopPropagation();
    await projectRemove(id);
    refresh();
  }

  function startEdit(e: React.MouseEvent, p: Project) {
    e.stopPropagation();
    setEditingId(p.id);
    setEditName(p.name);
    setEditDesc(p.desc);
  }
  function cancelEdit(e?: React.MouseEvent) {
    e?.stopPropagation();
    setEditingId(null);
    setEditName("");
    setEditDesc("");
  }
  async function saveEdit(e: React.MouseEvent | React.KeyboardEvent, id: string) {
    e.stopPropagation();
    if (!editName.trim() || savingEdit) return;
    setSavingEdit(true);
    const r = await projectUpdate(id, editName.trim(), editDesc.trim());
    setSavingEdit(false);
    if (r.ok) {
      cancelEdit();
      refresh();
    }
  }

  return (
    <AppShell title="Proyectos">
      <div className="max-w-6xl mx-auto px-3 py-6">
        <p className="text-[15px] mb-7 max-w-2xl" style={{ color: "var(--text-2)" }}>
          Cada proyecto es un espacio de trabajo: reúne <strong>fuentes</strong> (tu conocimiento),
          un <strong>chat con foco</strong> en ellas y un <strong>Studio</strong> de salidas que AION
          genera. Ábrelo para trabajar dentro.
        </p>

        <div className="card mb-10 max-w-3xl">
          <h2 className="t-section mb-4" style={{ color: "var(--text-3)" }}>
            NUEVO PROYECTO
          </h2>
          <div className="grid md:grid-cols-2 gap-3 mb-3">
            <input
              className="input"
              placeholder="Nombre del proyecto"
              value={name}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && add()}
            />
            <input
              className="input"
              placeholder="Descripción / objetivo (opcional)"
              value={desc}
              onChange={(e) => setDesc(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && add()}
            />
          </div>
          <button className="btn inline-flex items-center gap-1.5" onClick={add} disabled={busy}>
            <Icon name="plus" size={16} /> {busy ? "Creando…" : "Crear proyecto"}
          </button>
        </div>

        {projects.length === 0 ? (
          <div className="flex flex-col items-center text-center py-16" style={{ color: "var(--text-3)" }}>
            <span className="icon-chip mb-3" style={{ width: 52, height: 52, background: "var(--pastel-gold)", color: "var(--on-gold)" }}>
              <Icon name="folder" size={26} />
            </span>
            <p>Aún no tienes proyectos. Crea el primero arriba.</p>
          </div>
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
            {projects.map((p, i) => {
              const editing = editingId === p.id;
              if (editing) {
                // Tarjeta en modo edición: NO navega al abrir; edita nombre + descripción.
                return (
                  <div key={p.id} className="card" style={{ boxShadow: "var(--shadow-elevated)", borderColor: "var(--accent)" }}>
                    <div className="flex items-center gap-2 mb-3" style={{ color: "var(--gold-deep)" }}>
                      <Icon name="pencil" size={15} />
                      <span className="t-section" style={{ color: "var(--text-3)" }}>EDITAR PROYECTO</span>
                    </div>
                    <input
                      className="input mb-2"
                      placeholder="Nombre del proyecto"
                      value={editName}
                      autoFocus
                      onChange={(e) => setEditName(e.target.value)}
                      onKeyDown={(e) => { if (e.key === "Enter") saveEdit(e, p.id); if (e.key === "Escape") cancelEdit(); }}
                    />
                    <input
                      className="input mb-3"
                      placeholder="Descripción / objetivo (opcional)"
                      value={editDesc}
                      onChange={(e) => setEditDesc(e.target.value)}
                      onKeyDown={(e) => { if (e.key === "Enter") saveEdit(e, p.id); if (e.key === "Escape") cancelEdit(); }}
                    />
                    <div className="flex items-center gap-2">
                      <button
                        className="btn inline-flex items-center gap-1.5"
                        onClick={(e) => saveEdit(e, p.id)}
                        disabled={savingEdit || !editName.trim()}
                      >
                        <Icon name="check" size={15} /> {savingEdit ? "Guardando…" : "Guardar"}
                      </button>
                      <button
                        className="text-sm px-3 py-2 rounded-full"
                        style={{ background: "var(--surface-2)", color: "var(--text-2)" }}
                        onClick={cancelEdit}
                      >
                        Cancelar
                      </button>
                    </div>
                  </div>
                );
              }
              return (
                <div
                  key={p.id}
                  onClick={() => router.push(`/projects/workspace?id=${p.id}`)}
                  className="card cursor-pointer transition-transform hover:-translate-y-0.5 group"
                  style={{ boxShadow: "var(--shadow-elevated)" }}
                >
                  <div className="flex items-start gap-3">
                    <span
                      className="icon-chip shrink-0"
                      style={{ width: 40, height: 40, background: ACCENTS[i % ACCENTS.length], color: "var(--on-gold)" }}
                    >
                      <Icon name="folder" size={20} />
                    </span>
                    <div className="min-w-0 flex-1">
                      <h3 className="font-display font-semibold truncate">{p.name}</h3>
                      {p.desc && (
                        <p className="text-sm mt-0.5 line-clamp-2" style={{ color: "var(--text-2)" }}>
                          {p.desc}
                        </p>
                      )}
                    </div>
                    {/* Acciones: editar + eliminar. Visibles tenues (descubribles) y nítidas al pasar el ratón. */}
                    <div className="flex items-center gap-1 shrink-0 opacity-60 group-hover:opacity-100 transition-opacity">
                      <button
                        onClick={(e) => startEdit(e, p)}
                        className="p-1 rounded-md hover:bg-[var(--surface-2)]"
                        style={{ color: "var(--text-3)" }}
                        title="Editar proyecto"
                        aria-label="Editar proyecto"
                      >
                        <Icon name="pencil" size={15} />
                      </button>
                      <button
                        onClick={(e) => remove(e, p.id)}
                        className="p-1 rounded-md hover:bg-[var(--surface-2)]"
                        style={{ color: "var(--text-3)" }}
                        title="Eliminar proyecto"
                        aria-label="Eliminar proyecto"
                      >
                        <Icon name="trash" size={15} />
                      </button>
                    </div>
                  </div>
                  <p className="text-[11px] mt-3" style={{ color: "var(--text-3)" }}>
                    {new Date(p.updated || p.created).toLocaleDateString()}
                  </p>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </AppShell>
  );
}
