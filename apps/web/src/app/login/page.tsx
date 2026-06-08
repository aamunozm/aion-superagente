"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";
import { login, register } from "@/lib/api";

export default function LoginPage() {
  const router = useRouter();
  const [mode, setMode] = useState<"login" | "register">("login");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setLoading(true);
    try {
      const fn = mode === "login" ? login : register;
      const res = await fn(email, password);
      localStorage.setItem("aion_token", res.token);
      localStorage.setItem("aion_email", res.email);
      router.push("/chat");
    } catch (err) {
      setError(err instanceof Error ? err.message : "error");
    } finally {
      setLoading(false);
    }
  }

  return (
    <main className="min-h-screen flex items-center justify-center px-6">
      <div className="card w-full max-w-sm" style={{ boxShadow: "var(--shadow-elevated)" }}>
        <h1 className="font-display text-2xl font-bold mb-1">
          {mode === "login" ? "Entrar" : "Crear cuenta"}
        </h1>
        <p className="text-sm mb-6" style={{ color: "var(--text-3)" }}>
          AION · acceso a tu agente
        </p>
        <form onSubmit={submit} className="flex flex-col gap-3">
          <input
            className="input"
            type="email"
            placeholder="email"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            required
          />
          <input
            className="input"
            type="password"
            placeholder="contraseña (mín. 8)"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            required
          />
          {error && (
            <p className="text-sm" style={{ color: "#ef4444" }}>
              {error}
            </p>
          )}
          <button className="btn" disabled={loading}>
            {loading ? "..." : mode === "login" ? "Entrar" : "Registrarme"}
          </button>
        </form>
        <button
          onClick={() => setMode(mode === "login" ? "register" : "login")}
          className="mt-4 text-sm w-full text-center"
          style={{ color: "var(--accent)" }}
        >
          {mode === "login" ? "¿No tienes cuenta? Crear una" : "Ya tengo cuenta"}
        </button>
      </div>
    </main>
  );
}
