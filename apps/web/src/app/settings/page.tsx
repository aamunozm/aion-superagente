"use client";

import { useEffect, useState } from "react";
import AppShell from "@/components/AppShell";

export default function SettingsPage() {
  const [email, setEmail] = useState<string | null>(null);
  const [dark, setDark] = useState(false);

  useEffect(() => {
    setEmail(localStorage.getItem("aion_email"));
    const d = localStorage.getItem("aion_theme") === "dark";
    setDark(d);
    document.documentElement.classList.toggle("dark", d);
  }, []);

  function toggleTheme() {
    const d = !dark;
    setDark(d);
    document.documentElement.classList.toggle("dark", d);
    localStorage.setItem("aion_theme", d ? "dark" : "light");
  }

  return (
    <AppShell title="Ajustes">
      <div className="max-w-2xl mx-auto px-6 py-8 flex flex-col gap-6">
        <div className="card">
          <h2 className="t-section mb-3" style={{ color: "var(--text-2)" }}>
            Cuenta
          </h2>
          <p className="text-sm" style={{ color: "var(--text-2)" }}>
            Email: <strong>{email ?? "—"}</strong>
          </p>
          <p className="text-xs mt-1" style={{ color: "var(--text-3)" }}>
            Tu cuenta y tus datos viven solo en este dispositivo.
          </p>
        </div>

        <div className="card flex items-center justify-between">
          <div>
            <h2 className="t-section" style={{ color: "var(--text-2)" }}>
              Apariencia
            </h2>
            <p className="text-sm mt-1" style={{ color: "var(--text-3)" }}>
              Tema {dark ? "oscuro" : "claro"}
            </p>
          </div>
          <button className="btn" onClick={toggleTheme}>
            Cambiar a {dark ? "claro" : "oscuro"}
          </button>
        </div>

        <div className="card">
          <h2 className="t-section mb-2" style={{ color: "var(--text-2)" }}>
            Gobernanza del agente
          </h2>
          <p className="text-sm" style={{ color: "var(--text-2)" }}>
            Postura por defecto: <strong>Conservadora</strong> (acciones que escriben, envían,
            borran o gastan piden tu confirmación). Papelera reversible 30 días, kill switch y
            registro de auditoría activos.
          </p>
          <p className="text-xs mt-2" style={{ color: "var(--text-3)" }}>
            La configuración fina se gestiona en <code>~/Library/Application Support/AION/policy.json</code>.
          </p>
        </div>
      </div>
    </AppShell>
  );
}
