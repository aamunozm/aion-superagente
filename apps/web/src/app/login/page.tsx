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
  const [remember, setRemember] = useState(true);
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

          <form onSubmit={submit} className="flex flex-col gap-4">
            <label className="flex flex-col gap-1.5">
              <span className="text-sm font-medium">Email</span>
              <input
                className="input"
                type="email"
                placeholder="name@aion.app"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                required
              />
            </label>

            {mode === "forgot" && (
              <label className="flex flex-col gap-1.5">
                <span className="text-sm font-medium">Código de recuperación</span>
                <input
                  className="input"
                  type="text"
                  placeholder="XXXX-XXXX-XXXX"
                  value={code}
                  onChange={(e) => setCode(e.target.value)}
                  required
                />
              </label>
            )}

            <label className="flex flex-col gap-1.5">
              <div className="flex items-center justify-between">
                <span className="text-sm font-medium">
                  {mode === "forgot" ? "Nueva contraseña" : "Contraseña"}
                </span>
                {mode === "login" && (
                  <button
                    type="button"
                    onClick={() => { setMode("forgot"); setError(null); }}
                    className="text-sm"
                    style={{ color: "var(--gold-deep)" }}
                  >
                    ¿Olvidaste?
                  </button>
                )}
              </div>
              <input
                className="input"
                type="password"
                placeholder="••••••••"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                required
              />
            </label>

            {mode === "login" && (
              <label className="flex items-center gap-2 text-sm cursor-pointer" style={{ color: "var(--text-2)" }}>
                <input
                  type="checkbox"
                  checked={remember}
                  onChange={(e) => setRemember(e.target.checked)}
                  className="w-4 h-4 rounded"
                  style={{ accentColor: "var(--ink)" }}
                />
                Mantener sesión iniciada
              </label>
            )}

            {error && <p className="text-sm" style={{ color: "#ef4444" }}>{error}</p>}
            {notice && <p className="text-sm" style={{ color: "var(--accent-hover)" }}>{notice}</p>}

            <button className="btn inline-flex items-center justify-center gap-2" disabled={loading}>
              {loading ? "…" : (
                <>
                  {mode === "login" ? "Iniciar sesión" : mode === "register" ? "Crear cuenta" : "Actualizar contraseña"}
                  {mode !== "forgot" && <Icon name="send" size={16} className="-rotate-90" />}
                </>
              )}
            </button>
          </form>

          {mode === "login" && (
            <>
              <div className="flex items-center gap-3 my-5">
                <span className="h-px flex-1" style={{ background: "var(--border)" }} />
                <span className="text-xs" style={{ color: "var(--text-3)" }}>o continuá con</span>
                <span className="h-px flex-1" style={{ background: "var(--border)" }} />
              </div>
              <div className="grid grid-cols-2 gap-3">
                <button type="button" onClick={() => setNotice("Acceso con Google: próximamente.")}
                  className="card card-hover flex items-center justify-center gap-2 py-3 text-sm font-medium" style={{ padding: "12px" }}>
                  <GoogleMark /> Google
                </button>
                <button type="button" onClick={() => setNotice("Acceso con Apple: próximamente.")}
                  className="card card-hover flex items-center justify-center gap-2 py-3 text-sm font-medium" style={{ padding: "12px" }}>
                  <AppleMark /> Apple
                </button>
              </div>
            </>
          )}

          <div className="mt-6 text-sm text-center" style={{ color: "var(--text-3)" }}>
            {mode === "register" ? "¿Ya tienes cuenta? " : mode === "forgot" ? "" : "¿No tienes cuenta? "}
            {mode !== "register" ? (
              <button onClick={() => { setMode(mode === "forgot" ? "login" : "register"); setError(null); }}
                style={{ color: "var(--gold-deep)", fontWeight: 600 }}>
                {mode === "forgot" ? "← Volver a entrar" : "Crear una"}
              </button>
            ) : (
              <button onClick={() => { setMode("login"); setError(null); }} style={{ color: "var(--gold-deep)", fontWeight: 600 }}>
                Iniciar sesión
              </button>
            )}
          </div>

          <div className="mt-8 pt-4 flex items-center justify-between text-[11px]" style={{ color: "var(--text-3)", borderTop: "1px solid var(--border)" }}>
            <span className="inline-flex items-center gap-1.5"><Icon name="lock" size={13} /> JWT · E2E cifrado · 100% local</span>
            <span>TLS 1.3</span>
          </div>
        </div>
      </div>
    </main>
  );
}

function GoogleMark() {
  return (
    <svg width="16" height="16" viewBox="0 0 48 48" aria-hidden="true">
      <path fill="#EA4335" d="M24 9.5c3.5 0 6.6 1.2 9 3.6l6.7-6.7C35.6 2.6 30.2 0 24 0 14.6 0 6.5 5.4 2.6 13.3l7.8 6c1.9-5.6 7.1-9.8 13.6-9.8z" />
      <path fill="#4285F4" d="M46.1 24.5c0-1.6-.1-3.1-.4-4.5H24v9h12.4c-.5 2.9-2.1 5.3-4.6 7l7.1 5.5c4.1-3.8 6.5-9.4 6.5-17z" />
      <path fill="#FBBC05" d="M10.4 28.3a14.5 14.5 0 0 1 0-8.6l-7.8-6A24 24 0 0 0 0 24c0 3.9.9 7.5 2.6 10.3l7.8-6z" />
      <path fill="#34A853" d="M24 48c6.2 0 11.5-2 15.3-5.6l-7.1-5.5c-2 1.4-4.6 2.2-8.2 2.2-6.5 0-11.7-4.2-13.6-9.8l-7.8 6C6.5 42.6 14.6 48 24 48z" />
    </svg>
  );
}
function AppleMark() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <path d="M16.4 12.7c0-2.3 1.9-3.4 2-3.5-1.1-1.6-2.8-1.8-3.4-1.8-1.5-.2-2.8.8-3.5.8s-1.9-.8-3.1-.8c-1.6 0-3.1.9-3.9 2.4-1.7 2.9-.4 7.2 1.2 9.6.8 1.2 1.7 2.5 3 2.4 1.2-.05 1.7-.8 3.1-.8s1.9.8 3.1.8c1.3 0 2.1-1.2 2.9-2.3.9-1.3 1.3-2.6 1.3-2.7-.03-.01-2.5-1-2.5-3.9zM14 5.6c.7-.8 1.1-2 1-3.1-1 0-2.1.7-2.8 1.5-.6.7-1.2 1.8-1 2.9 1.1.1 2.2-.5 2.8-1.3z" />
    </svg>
  );
}
