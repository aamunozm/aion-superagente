"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import {
  systemScan,
  providerSet,
  governanceSetup,
  modelsPull,
  type SystemScan,
  type ModelOption,
} from "@/lib/api";

type Step = "scan" | "model" | "install" | "rules" | "done";

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

  async function startInstall() {
    setError(null);
    try {
      if (source === "external") {
        const p = PROVIDERS[extProvider];
        await providerSet({ kind: "external", model: extModel, base_url: p.base_url, api_key: apiKey });
        setStep("rules");
        return;
      }
      if (!chosen) return;
      await providerSet({ kind: "local", model: chosen.ollama_name });
      setStep("install");
      // gemma4-reason lo provisiona el propio sistema (bootstrap); el resto se descarga.
      if (chosen.ollama_name === "gemma4-reason") {
        setProgressMsg("AION está preparando el modelo recomendado en segundo plano…");
        setProgress(100);
        return;
      }
      await modelsPull(chosen.ollama_name, (e) => {
        if (e.kind === "progress") {
          setProgress(e.percent ?? 0);
          setProgressMsg(e.status ?? "");
        } else if (e.kind === "error") {
          setError(e.text ?? "error al descargar");
        }
      });
      setProgress(100);
      setProgressMsg("Modelo instalado.");
    } catch (e) {
      setError(e instanceof Error ? e.message : "error");
    }
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

        {/* PASO 3: INSTALACIÓN */}
        {step === "install" && (
          <div className="card" style={{ boxShadow: "var(--shadow-elevated)" }}>
            <h1 className="font-display text-2xl font-bold mb-1">Instalando el modelo</h1>
            <p className="text-sm mb-5" style={{ color: "var(--text-3)" }}>
              {chosen?.name} · {chosen?.size_gb} GB. La primera vez tarda según tu conexión.
            </p>
            <div className="h-3 rounded-full overflow-hidden mb-2" style={{ background: "var(--surface-2)" }}>
              <div className="h-full transition-all" style={{ width: `${progress}%`, background: "var(--accent)" }} />
            </div>
            <p className="text-xs mb-5" style={{ color: "var(--text-3)" }}>{progress}% · {progressMsg}</p>
            {error && <p className="text-sm mb-2" style={{ color: "#ef4444" }}>{error}</p>}
            <button className="btn w-full" onClick={() => setStep("rules")} disabled={progress < 100 && !error}>
              {progress >= 100 || error ? "Continuar" : "Descargando…"}
            </button>
          </div>
        )}

        {/* PASO 4: REGLAS DEL AGENTE */}
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
