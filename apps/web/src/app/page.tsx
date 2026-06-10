"use client";

import { useEffect, useState } from "react";
import Link from "next/link";

export default function Home() {
  // Un SOLO CTA contextual: si ya empezaste (hay sesión), "Abrir chat"; si es la
  // primera vez, "Empezar". Evita el "uno u otro" confuso de dos botones.
  const [ready, setReady] = useState(false);
  const [returning, setReturning] = useState(false);

  useEffect(() => {
    try {
      setReturning(!!localStorage.getItem("aion_token"));
    } catch {
      /* sin acceso a storage */
    }
    setReady(true);
  }, []);

  const href = returning ? "/chat" : "/login";
  const label = returning ? "Abrir chat" : "Empezar";

  return (
    <main className="min-h-screen flex flex-col items-center justify-center px-6 text-center">
      <div className="max-w-xl">
        <div
          className="inline-flex items-center gap-2 mb-6 px-3 py-1 rounded-full text-[11px] font-semibold uppercase tracking-wide"
          style={{ background: "var(--accent-subtle)", color: "var(--accent)" }}
        >
          <span className="w-2 h-2 rounded-full" style={{ background: "var(--accent)" }} />
          local-first · privado · auto-evolutivo
        </div>
        <h1 className="font-display text-5xl font-bold mb-4" style={{ color: "var(--text-1)" }}>
          AION
        </h1>
        <p className="text-lg mb-8" style={{ color: "var(--text-2)" }}>
          Tu super-agente de IA que razona, recuerda y evoluciona — toda la
          cognición en tu dispositivo.
        </p>
        <div className="flex flex-col items-center gap-3">
          {/* Botón principal único; invisible hasta saber el estado (evita parpadeo). */}
          <Link
            href={href}
            className="btn"
            style={{ minWidth: 200, opacity: ready ? 1 : 0, transition: "opacity .15s" }}
          >
            {label}
          </Link>
          {/* Si ya empezaste, un enlace secundario y discreto para reconfigurar. */}
          {ready && returning && (
            <Link href="/login" className="text-sm" style={{ color: "var(--text-3)" }}>
              o volver a configurar
            </Link>
          )}
        </div>
      </div>
    </main>
  );
}
