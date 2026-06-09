"use client";

import { useEffect, useState } from "react";
import AppShell from "@/components/AppShell";

type Tool = {
  id: string;
  name: string;
  desc: string;
  icon: string;
  status: "activa" | "disponible" | "proximamente";
};

const TOOLS: Tool[] = [
  { id: "memory", name: "Memoria", desc: "Recuerda y asocia conocimiento entre chats.", icon: "🧠", status: "activa" },
  { id: "web", name: "Investigación web", desc: "Busca y lee fuentes en internet.", icon: "🌐", status: "activa" },
  { id: "skills", name: "Skills (WASM)", desc: "El agente se forja herramientas nuevas en sandbox.", icon: "🔧", status: "activa" },
  { id: "calc", name: "Calculadora", desc: "Aritmética precisa para el agente.", icon: "🧮", status: "activa" },
  { id: "screen", name: "Visión de pantalla", desc: "Ver la pantalla para asistirte (requiere permiso).", icon: "👁", status: "disponible" },
  { id: "control", name: "Control del PC", desc: "Teclado y ratón bajo gobernanza (requiere permiso).", icon: "🖐", status: "disponible" },
  { id: "email", name: "Email", desc: "Leer y redactar correo (confirmación al enviar).", icon: "✉️", status: "proximamente" },
  { id: "calendar", name: "Calendario", desc: "Consultar y crear eventos.", icon: "📅", status: "proximamente" },
];

const STATUS_STYLE: Record<Tool["status"], { label: string; color: string }> = {
  activa: { label: "Activa", color: "var(--accent)" },
  disponible: { label: "Disponible", color: "#C49A3D" },
  proximamente: { label: "Próximamente", color: "var(--text-3)" },
};

export default function ToolsPage() {
  const [enabled, setEnabled] = useState<Record<string, boolean>>({});

  useEffect(() => {
    try {
      setEnabled(JSON.parse(localStorage.getItem("aion_tools") ?? "{}"));
    } catch {
      /* vacío */
    }
  }, []);

  function toggle(id: string) {
    const next = { ...enabled, [id]: !enabled[id] };
    setEnabled(next);
    localStorage.setItem("aion_tools", JSON.stringify(next));
  }

  return (
    <AppShell title="Herramientas">
      <div className="max-w-4xl mx-auto px-6 py-8">
        <p className="text-sm mb-6" style={{ color: "var(--text-2)" }}>
          Conecta capacidades a AION. Las activas ya las usa el agente; las disponibles se
          activan concediendo permisos del sistema.
        </p>
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {TOOLS.map((t) => {
            const s = STATUS_STYLE[t.status];
            const on = t.status === "activa" || enabled[t.id];
            return (
              <div key={t.id} className="card flex items-start gap-3">
                <span className="text-2xl shrink-0">{t.icon}</span>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <h3 className="font-display font-semibold">{t.name}</h3>
                    <span className="text-[10px] font-semibold uppercase tracking-wide" style={{ color: s.color }}>
                      {s.label}
                    </span>
                  </div>
                  <p className="text-sm mt-1" style={{ color: "var(--text-2)" }}>
                    {t.desc}
                  </p>
                </div>
                {t.status !== "proximamente" && (
                  <button
                    onClick={() => t.status !== "activa" && toggle(t.id)}
                    className="text-xs px-3 py-1.5 rounded-full shrink-0"
                    style={{
                      background: on ? "var(--accent-subtle)" : "var(--surface-2)",
                      color: on ? "var(--accent)" : "var(--text-2)",
                      cursor: t.status === "activa" ? "default" : "pointer",
                    }}
                  >
                    {t.status === "activa" ? "Activa" : on ? "Conectada" : "Conectar"}
                  </button>
                )}
              </div>
            );
          })}
        </div>
      </div>
    </AppShell>
  );
}
