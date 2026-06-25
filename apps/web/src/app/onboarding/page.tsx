"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import {
  systemScan,
  providerSet,
  governanceSetup,
  modelsPull,
  status,
  identityNameSet,
  type SystemScan,
  type ModelOption,
} from "@/lib/api";

type Step = "scan" | "model" | "install" | "identity" | "rules";

const PROVIDERS: Record<string, { label: string; base_url: string; model: string }> = {
  groq: { label: "Groq", base_url: "https://api.groq.com/openai/v1", model: "llama-3.3-70b-versatile" },
  openrouter: { label: "OpenRouter", base_url: "https://openrouter.ai/api/v1", model: "openai/gpt-4o-mini" },
  google: { label: "Google Gemini", base_url: "https://generativelanguage.googleapis.com/v1beta/openai", model: "gemini-2.0-flash" },
  openai: { label: "OpenAI", base_url: "https://api.openai.com/v1", model: "gpt-4o-mini" },
};

const POSTURES = [
  { id: "conservative", name: "Conservadora", desc: "Solo lee por su cuenta; escribir, enviar, borrar o gastar pide tu confirmación. (Recomendada)" },
  { id: "balanced", name: "Equilibrada", desc: "Crea y edita por su cuenta; confirma enviar, borrar, instalar y pagar." },
  { id: "max", name: "Máxima autonomía", desc: "Autónomo en casi todo; solo la lista roja pide confirmación." },
];

// Voz por GÉNERO (Piper, género fiable). Mujer = Lucía; hombre = Mateo (el predeterminado).
const VOICE_BY_GENDER: Record<"f" | "m", string> = {
  f: "es_MX-claude-high",
  m: "es_MX-ald-medium",
};

export default function OnboardingPage() {
  const router = useRouter();
  const [step, setStep] = useState<Step>("scan");
  const [scan, setScan] = useState<SystemScan | null>(null);
  const [catalog, setCatalog] = useState<ModelOption[]>([]);
  const [source, setSource] = useState<"local" | "external">("local");
  const [chosen, setChosen] = useState<ModelOption | null>(null);
  const [extProvider, setExtProvider] = useState("groq");
  const [apiKey, setApiKey] = useState("");
  const [extModel, setExtModel] = useState(PROVIDERS.groq.model);
  const [progress, setProgress] = useState(0);
  const [progressMsg, setProgressMsg] = useState("");
  const [ready, setReady] = useState(false); // modelo verificado y disponible
  const [agentName, setAgentName] = useState("");
  const [gender, setGender] = useState<"f" | "m">("m");
  const [posture, setPosture] = useState("conservative");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    systemScan()
      .then((r) => {
        setScan(r.scan);
        setCatalog(r.catalog);
        setChosen(r.catalog.find((m) => m.recommended) ?? r.catalog[0] ?? null);
      })
      .catch((e) => setError(e instanceof Error ? e.message : "error"));
  }, []);

  // Espera ACTIVA a que el modelo esté realmente listo (descargado y cargable) antes de continuar.
  // Resuelve el problema del «Continuar» prematuro: el chat fallaba con Ollama 500 porque el modelo
  // aún se estaba descargando (o preparando en segundo plano, caso del recomendado).
  async function waitModelReady() {
    for (let i = 0; i < 900; i++) {
      // ~30 min de margen
      const s = await status().catch(() => null);
      if (s?.model_ready) {
        setReady(true);
        setProgress(100);
        setProgressMsg("Modelo listo ✓");
        return;
      }
      await new Promise((r) => setTimeout(r, 2000));
    }
    setError("el modelo tarda demasiado en prepararse; revisa que Ollama esté corriendo.");
  }

  async function startInstall() {
    setError(null);
    setReady(false);
    try {
      if (source === "external") {
        const p = PROVIDERS[extProvider];
        await providerSet({ kind: "external", model: extModel, base_url: p.base_url, api_key: apiKey });
        setReady(true);
        setStep("identity");
        return;
      }
      if (!chosen) return;
      await providerSet({ kind: "local", model: chosen.ollama_name });
      setStep("install");
      setProgress(0);
      if (chosen.ollama_name === "gemma4-reason") {
        // El sistema lo provisiona en segundo plano (bootstrap): no hay % de descarga, esperamos.
        setProgressMsg("AION está preparando el modelo recomendado… (no cierres esta ventana)");
      } else {
        setProgressMsg("Descargando el modelo…");
        await modelsPull(chosen.ollama_name, (e) => {
          if (e.kind === "progress") {
            setProgress(e.percent ?? 0);
            setProgressMsg(e.status ?? "Descargando…");
          } else if (e.kind === "error") {
            setError(e.text ?? "error al descargar");
          }
        });
        setProgressMsg("Verificando el modelo…");
      }
      // En TODOS los casos: no dejamos continuar hasta confirmar que el modelo responde.
      await waitModelReady();
    } catch (e) {
      setError(e instanceof Error ? e.message : "error");
    }
  }

  async function finishIdentity() {
    // Nombre elegido por el usuario (si lo dejó vacío, AION elige el suyo solo).
    if (agentName.trim().length >= 2) {
      await identityNameSet(agentName.trim()).catch(() => {});
    }
    // Voz por género (Piper, fiable). Se guarda como preferencia local.
    try {
      localStorage.setItem("aion.voice.name", VOICE_BY_GENDER[gender]);
      localStorage.setItem("aion.voice.engine", "piper");
      localStorage.setItem("aion.voice", "piper"); // != "system"
    } catch {
      /* sin localStorage: no bloquea */
    }
    setStep("rules");
  }

  async function finish() {
    try {
      await governanceSetup(posture);
    } catch {
      /* no bloquea */
    }
    router.push("/chat");
  }

  return (
    <main className="min-h-screen flex items-center justify-center px-6 py-10">
      <div className="w-full max-w-xl">
        <p className="text-[11px] font-semibold uppercase tracking-widest mb-2" style={{ color: "var(--accent)" }}>
          AION · configuración inicial
        </p>

        {/* PASO 1: ESCANEO */}
        {step === "scan" && (
          <div className="card" style={{ boxShadow: "var(--shadow-elevated)" }}>
            <h1 className="font-display text-2xl font-bold mb-1">Analizando tu equipo</h1>
            <p className="text-sm mb-5" style={{ color: "var(--text-3)" }}>
              Detecto tu hardware para recomendarte el mejor modelo de IA.
            </p>
            {!scan ? (
              <p className="text-sm" style={{ color: "var(--text-2)" }}>Escaneando…</p>
            ) : (
              <>
                <div className="grid grid-cols-2 gap-3 text-sm mb-4">
                  <Info label="RAM" value={`${scan.ram_gb} GB`} />
                  <Info label="CPU" value={`${scan.cpu_cores} núcleos`} />
                  <Info label="GPU" value={scan.gpu} />
                  <Info label="Disco libre" value={`${scan.disk_free_gb} GB`} />
                </div>
                <div className="rounded-lg p-4 mb-4" style={{ background: "var(--accent-subtle)" }}>
                  <div className="text-xs uppercase tracking-wide font-semibold" style={{ color: "var(--accent)" }}>
                    Nivel recomendado: {scan.tier}
                  </div>
                  <p className="text-sm mt-1" style={{ color: "var(--text-2)" }}>{scan.tier_reason}</p>
                </div>
                <button className="btn w-full" onClick={() => setStep("model")}>Continuar</button>
              </>
            )}
          </div>
        )}

        {/* PASO 2: ELEGIR MODELO */}
        {step === "model" && (
          <div className="card" style={{ boxShadow: "var(--shadow-elevated)" }}>
            <h1 className="font-display text-2xl font-bold mb-1">Elige tu IA</h1>
            <p className="text-sm mb-4" style={{ color: "var(--text-3)" }}>
              Local (privado, en tu equipo) o una API externa que ya tengas.
            </p>
            <div className="flex gap-1 p-1 rounded-full mb-4 w-fit" style={{ background: "var(--surface-2)" }}>
              {(["local", "external"] as const).map((s) => (
                <button key={s} onClick={() => setSource(s)} className="text-xs px-3 py-1 rounded-full"
                  style={{ background: source === s ? "var(--primary)" : "transparent", color: source === s ? "var(--primary-contrast)" : "var(--text-2)" }}>
                  {s === "local" ? "Local" : "API externa"}
                </button>
              ))}
            </div>

            {source === "local" ? (
              <div className="flex flex-col gap-2 mb-4">
                {catalog.map((m) => (
                  <button key={m.id} onClick={() => setChosen(m)} className="text-left rounded-lg p-3 border transition-all"
                    style={{ borderColor: chosen?.id === m.id ? "var(--accent)" : "var(--border)", background: chosen?.id === m.id ? "var(--accent-subtle)" : "transparent" }}>
                    <div className="flex items-center justify-between">
                      <span className="font-medium text-sm">{m.name}</span>
                      <span className="text-[11px]" style={{ color: "var(--text-3)" }}>
                        {m.size_gb} GB{m.recommended ? " · recomendado" : ""}
                      </span>
                    </div>
                    <p className="text-xs mt-1" style={{ color: "var(--text-2)" }}>{m.note}</p>
                  </button>
                ))}
              </div>
            ) : (
              <div className="flex flex-col gap-3 mb-4">
                <select className="input" value={extProvider}
                  onChange={(e) => { setExtProvider(e.target.value); setExtModel(PROVIDERS[e.target.value].model); }}>
                  {Object.entries(PROVIDERS).map(([k, v]) => <option key={k} value={k}>{v.label}</option>)}
                </select>
                <input className="input" placeholder="Modelo" value={extModel} onChange={(e) => setExtModel(e.target.value)} />
                <input className="input" type="password" placeholder="API key" value={apiKey} onChange={(e) => setApiKey(e.target.value)} />
                <p className="text-[11px]" style={{ color: "var(--text-3)" }}>Tu clave se guarda solo en este equipo.</p>
              </div>
            )}

            {error && <p className="text-sm mb-2" style={{ color: "#ef4444" }}>{error}</p>}
            <div className="flex gap-2">
              <button className="btn flex-1" onClick={startInstall}
                disabled={source === "external" && !apiKey}>
                {source === "local" ? "Instalar y continuar" : "Conectar y continuar"}
              </button>
            </div>
          </div>
        )}

        {/* PASO 3: INSTALACIÓN (espera a que el modelo esté REALMENTE listo) */}
        {step === "install" && (
          <div className="card" style={{ boxShadow: "var(--shadow-elevated)" }}>
            <h1 className="font-display text-2xl font-bold mb-1">Preparando el modelo</h1>
            <p className="text-sm mb-5" style={{ color: "var(--text-3)" }}>
              {chosen?.name} · {chosen?.size_gb} GB. La primera vez tarda según tu conexión. No cierres esta ventana.
            </p>
            <div className="h-3 rounded-full overflow-hidden mb-2" style={{ background: "var(--surface-2)" }}>
              <div className="h-full transition-all" style={{ width: `${ready ? 100 : progress}%`, background: "var(--accent)" }} />
            </div>
            <p className="text-xs mb-5" style={{ color: "var(--text-3)" }}>{ready ? 100 : progress}% · {progressMsg}</p>
            {error && <p className="text-sm mb-2" style={{ color: "#ef4444" }}>{error}</p>}
            <button className="btn w-full" onClick={() => setStep("identity")} disabled={!ready && !error}>
              {ready || error ? "Continuar" : "Preparando…"}
            </button>
          </div>
        )}

        {/* PASO 4: IDENTIDAD — nombre + voz (femenina / masculina) */}
        {step === "identity" && (
          <div className="card" style={{ boxShadow: "var(--shadow-elevated)" }}>
            <h1 className="font-display text-2xl font-bold mb-1">Dale identidad</h1>
            <p className="text-sm mb-4" style={{ color: "var(--text-3)" }}>
              Elige cómo se llama y cómo suena. Podrás cambiarlo en Ajustes.
            </p>

            <label className="text-[11px]" style={{ color: "var(--text-3)" }}>Nombre</label>
            <input
              className="input mt-1 mb-1"
              placeholder="Escribe un nombre…"
              value={agentName}
              onChange={(e) => setAgentName(e.target.value)}
              maxLength={24}
            />
            <p className="text-[11px] mb-4" style={{ color: "var(--text-3)" }}>
              Déjalo vacío y <strong>AION elegirá su propio nombre</strong> al nacer.
            </p>

            <label className="text-[11px]" style={{ color: "var(--text-3)" }}>Voz</label>
            <div className="grid grid-cols-2 gap-2 mt-1 mb-5">
              {([["f", "Femenina", "Lucía"], ["m", "Masculina", "Mateo"]] as const).map(([g, label, who]) => (
                <button
                  key={g}
                  onClick={() => setGender(g)}
                  className="rounded-lg p-3 border text-left transition-all"
                  style={{ borderColor: gender === g ? "var(--accent)" : "var(--border)", background: gender === g ? "var(--accent-subtle)" : "transparent" }}
                >
                  <div className="font-medium text-sm">{label}</div>
                  <div className="text-[11px]" style={{ color: "var(--text-3)" }}>{who} · español (Piper)</div>
                </button>
              ))}
            </div>

            <button className="btn w-full" onClick={finishIdentity}>Continuar</button>
          </div>
        )}

        {/* PASO 5: REGLAS DEL AGENTE */}
        {step === "rules" && (
          <div className="card" style={{ boxShadow: "var(--shadow-elevated)" }}>
            <h1 className="font-display text-2xl font-bold mb-1">Reglas del agente</h1>
            <p className="text-sm mb-4" style={{ color: "var(--text-3)" }}>
              ¿Cuánta autonomía le das a AION? Podrás cambiarlo en Ajustes.
            </p>
            <div className="flex flex-col gap-2 mb-5">
              {POSTURES.map((p) => (
                <button key={p.id} onClick={() => setPosture(p.id)} className="text-left rounded-lg p-3 border transition-all"
                  style={{ borderColor: posture === p.id ? "var(--accent)" : "var(--border)", background: posture === p.id ? "var(--accent-subtle)" : "transparent" }}>
                  <span className="font-medium text-sm">{p.name}</span>
                  <p className="text-xs mt-1" style={{ color: "var(--text-2)" }}>{p.desc}</p>
                </button>
              ))}
            </div>
            <button className="btn w-full" onClick={finish}>Empezar a usar AION</button>
          </div>
        )}
      </div>
    </main>
  );
}

function Info({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg p-3" style={{ background: "var(--surface-1)" }}>
      <div className="text-[11px] uppercase tracking-wide" style={{ color: "var(--text-3)" }}>{label}</div>
      <div className="font-medium mt-0.5">{value}</div>
    </div>
  );
}
