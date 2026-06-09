"use client";

import { useEffect, useState } from "react";
import AppShell from "@/components/AppShell";
import Icon from "@/components/Icon";

type Project = { id: string; name: string; desc: string; created: string };

export default function ProjectsPage() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [name, setName] = useState("");
  const [desc, setDesc] = useState("");

  useEffect(() => {
    try {
      setProjects(JSON.parse(localStorage.getItem("aion_projects") ?? "[]"));
    } catch {
      /* vacío */
    }
  }, []);

  function save(list: Project[]) {
    setProjects(list);
    localStorage.setItem("aion_projects", JSON.stringify(list));
  }
  function add() {
    if (!name.trim()) return;
    save([
      { id: crypto.randomUUID(), name: name.trim(), desc: desc.trim(), created: new Date().toISOString() },
      ...projects,
    ]);
    setName("");
    setDesc("");
  }
  function remove(id: string) {
    save(projects.filter((p) => p.id !== id));
  }

  return (
    <AppShell title="Proyectos">
      <div className="max-w-4xl mx-auto px-6 py-8">
        <p className="text-sm mb-6" style={{ color: "var(--text-2)" }}>
          Organiza el trabajo de AION en proyectos. Cada proyecto agrupa contexto, tareas y
          conocimiento para que el agente actúe con foco.
        </p>

        <div className="card mb-8">
          <h2 className="t-section mb-3" style={{ color: "var(--text-2)" }}>
            Nuevo proyecto
          </h2>
          <div className="flex flex-col gap-3">
            <input
              className="input"
              placeholder="Nombre del proyecto"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
            <input
              className="input"
              placeholder="Descripción / objetivo (opcional)"
              value={desc}
              onChange={(e) => setDesc(e.target.value)}
            />
            <button className="btn self-start" onClick={add}>
              + Crear proyecto
            </button>
          </div>
        </div>

        {projects.length === 0 ? (
          <div className="flex flex-col items-center text-center py-16" style={{ color: "var(--text-3)" }}>
            <span className="icon-chip mb-3" style={{ width: 52, height: 52, background: "var(--pastel-gold)", color: "var(--on-gold)" }}>
              <Icon name="folder" size={26} />
            </span>
            <p>Aún no tienes proyectos. Crea el primero arriba.</p>
          </div>
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {projects.map((p) => (
              <div key={p.id} className="card" style={{ boxShadow: "var(--shadow-elevated)" }}>
                <div className="flex items-start justify-between">
                  <h3 className="font-display font-semibold">{p.name}</h3>
                  <button
                    onClick={() => remove(p.id)}
                    className="text-xs"
                    style={{ color: "var(--text-3)" }}
                  >
                    ✕
                  </button>
                </div>
                {p.desc && (
                  <p className="text-sm mt-1" style={{ color: "var(--text-2)" }}>
                    {p.desc}
                  </p>
                )}
                <p className="text-[11px] mt-3" style={{ color: "var(--text-3)" }}>
                  {new Date(p.created).toLocaleDateString()}
                </p>
              </div>
            ))}
          </div>
        )}
      </div>
    </AppShell>
  );
}
