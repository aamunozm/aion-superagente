"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";
import Icon from "@/components/Icon";
import { login, register, resetPassword } from "@/lib/api";

type Mode = "login" | "register" | "forgot";

export default function LoginPage() {
  const router = useRouter();
  const [mode, setMode] = useState<Mode>("login");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [code, setCode] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [recovery, setRecovery] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  function enter(token: string, mail: string) {
    localStorage.setItem("aion_token", token);
    localStorage.setItem("aion_email", mail);
    router.push("/chat");
  }

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setNotice(null);
    setLoading(true);
    try {
      if (mode === "login") {
        const r = await login(email, password);
        enter(r.token, r.email);
      } else if (mode === "register") {
        const r = await register(email, password);
        if (r.recovery_code) {
          setRecovery(r.recovery_code); // mostrar una vez (local-first, sin email)
          localStorage.setItem("aion_token", r.token);
          localStorage.setItem("aion_email", r.email);
        } else {
          enter(r.token, r.email);
        }
      } else {
        await resetPassword(email, code, password);
        setNotice("Contraseña actualizada. Ya puedes entrar.");
        setMode("login");
        setPassword("");
        setCode("");
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "error");
    } finally {
      setLoading(false);
    }
  }

  // Post-registro: mostrar el código de recuperación a guardar.
  if (recovery) {
    return (
      <main className="min-h-screen flex items-center justify-center px-6">
        <div className="card w-full max-w-md" style={{ boxShadow: "var(--shadow-float)" }}>
          <h1 className="font-display text-xl font-bold mb-2">Guarda tu código de recuperación</h1>
          <p className="text-sm mb-4" style={{ color: "var(--text-2)" }}>
            AION es local y privado: no enviamos emails. Este código es la ÚNICA forma de
            recuperar tu contraseña si la olvidas. Guárdalo en un lugar seguro.
          </p>
          <div
            className="rounded-lg p-4 text-center font-mono text-lg tracking-widest mb-4 select-all"
            style={{ background: "var(--surface-2)", color: "var(--accent)" }}
          >
            {recovery}
          </div>
          <button className="btn w-full" onClick={() => router.push("/onboarding")}>
            Lo he guardado — configurar AION
          </button>
        </div>
      </main>
    );
  }

  const titles: Record<Mode, string> = {
    login: "Bienvenido de vuelta",
    register: "Crea tu cuenta",
    forgot: "Recuperar contraseña",
  };

  return (
    <main className="min-h-screen flex">
      {/* Panel de marca (oculto en móvil) */}
      <div
        className="hidden lg:flex flex-col justify-between w-[44%] p-12"
        style={{ background: "var(--ink)", color: "#fff" }}
      >
        <div className="flex items-center gap-2">
          <span
            className="w-9 h-9 rounded-lg flex items-center justify-center font-bold"
            style={{ background: "var(--accent)", color: "#04201f" }}
          >
            A
          </span>
          <span className="font-display font-bold text-lg">AION</span>
        </div>
        <div>
          <h2 className="font-display text-4xl font-bold leading-tight mb-4">
            Tu super-agente
            <br />
            de IA, privado.
          </h2>
          <p className="text-base mb-8" style={{ color: "rgba(255,255,255,0.7)" }}>
            Razona, recuerda, investiga y evoluciona — toda la cognición en tu dispositivo.
          </p>
          <ul className="space-y-3 text-sm" style={{ color: "rgba(255,255,255,0.85)" }}>
            {[
              "100% local: tus datos nunca salen del equipo",
              "Memoria que aprende y se conecta entre chats",
              "Se forja sus propias herramientas y se auto-mejora",
            ].map((f) => (
              <li key={f} className="flex items-center gap-2">
                <span style={{ color: "var(--gold)" }} className="shrink-0"><Icon name="check" size={16} /></span> {f}
              </li>
            ))}
          </ul>
        </div>
        <div className="text-xs" style={{ color: "rgba(255,255,255,0.4)" }}>
          local-first · privado · auto-evolutivo
        </div>
      </div>

      {/* Panel de formulario */}
      <div className="flex-1 flex items-center justify-center px-6">
        <div className="w-full max-w-sm">
          <p
            className="text-[11px] font-semibold uppercase tracking-widest mb-2"
            style={{ color: "var(--accent)" }}
          >
            AION · acceso
          </p>
          <h1 className="font-display text-2xl font-bold mb-1">{titles[mode]}</h1>
          <p className="text-sm mb-6" style={{ color: "var(--text-3)" }}>
            {mode === "forgot"
              ? "Introduce tu email, tu código de recuperación y una nueva contraseña."
              : "Accede a tu agente local."}
          </p>

          <form onSubmit={submit} className="flex flex-col gap-3">
            <input
              className="input"
              type="email"
              placeholder="Email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              required
            />
            {mode === "forgot" && (
              <input
                className="input"
                type="text"
                placeholder="Código de recuperación"
                value={code}
                onChange={(e) => setCode(e.target.value)}
                required
              />
            )}
            <input
              className="input"
              type="password"
              placeholder={mode === "forgot" ? "Nueva contraseña (mín. 8)" : "Contraseña (mín. 8)"}
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              required
            />

            {error && <p className="text-sm" style={{ color: "#ef4444" }}>{error}</p>}
            {notice && <p className="text-sm" style={{ color: "var(--accent)" }}>{notice}</p>}

            <button className="btn mt-1" disabled={loading}>
              {loading
                ? "…"
                : mode === "login"
                  ? "Entrar"
                  : mode === "register"
                    ? "Crear cuenta"
                    : "Actualizar contraseña"}
            </button>
          </form>

          <div className="mt-5 flex items-center justify-between text-sm">
            {mode !== "login" ? (
              <button onClick={() => { setMode("login"); setError(null); }} style={{ color: "var(--accent)" }}>
                ← Volver a entrar
              </button>
            ) : (
              <button onClick={() => { setMode("forgot"); setError(null); }} style={{ color: "var(--text-3)" }}>
                ¿Olvidaste tu contraseña?
              </button>
            )}
            {mode !== "register" && (
              <button onClick={() => { setMode("register"); setError(null); }} style={{ color: "var(--accent)" }}>
                Crear cuenta
              </button>
            )}
          </div>
        </div>
      </div>
    </main>
  );
}
