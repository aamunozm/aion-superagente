"use client";

import { useEffect, useRef, useState } from "react";
import AppShell from "@/components/AppShell";
import { APP_VERSION } from "@/lib/version";
import Icon from "@/components/Icon";
import { LANGS, useT } from "@/lib/i18n";
import { playTtsBlob, systemVoices, pickSystemVoice } from "@/lib/voice";
import {
  ttsSpeak,
  ttsVoices,
  ttsCloneUpload,
  ttsCloneRemove,
  credentialsList,
  credentialRemove,
  credentialSet,
  apiKeysList,
  apiKeySet,
  modelsInstalled,
  modelsPull,
  modelsRemove,
  providerGet,
  providerSet,
  systemScan,
  status,
  downloadAgent,
  agentImport,
  agentWipe,
  getIdentity,
  factoryReset,
  a2aGet,
  a2aSet,
  a2aSend,
  sensorsGet,
  sensorsSet,
  claudeCodeGet,
  claudeCodeConnect,
  claudeCodeDisconnect,
  claudeCodeTest,
  claudeCodeSet,
  type ClaudeCodeStatus,
  type AionIdentity,
  type A2aConfig,
  type CredMeta,
  type ApiKeyMeta,
  type InstalledModel,
  type ModelOption,
  type ProviderState,
  type SensorConfig,
  type SystemScan,
} from "@/lib/api";

// Proveedores de motor LLM. "local" = Ollama (privado). Los externos hablan una API
// OpenAI-compatible; AION ya despacha a ellas desde el backend (build_engine).
type ProvKey = "local" | "google" | "deepseek" | "custom";
const PROVIDERS: Record<
  Exclude<ProvKey, "local">,
  { label: string; base_url: string; defaultModel: string; keyHint: string }
> = {
  google: {
    label: "Google AI Studio (Gemini)",
    base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
    defaultModel: "gemini-2.5-flash",
    keyHint: "API key de aistudio.google.com",
  },
  deepseek: {
    label: "DeepSeek",
    base_url: "https://api.deepseek.com",
    defaultModel: "deepseek-chat",
    keyHint: "API key de platform.deepseek.com",
  },
  custom: {
    label: "API OpenAI-compatible (otra)",
    base_url: "",
    defaultModel: "",
    keyHint: "API key del proveedor",
  },
};

// Modelos disponibles en la API de DeepSeek (platform.deepseek.com)
const DEEPSEEK_MODELS: { id: string; label: string; desc: string }[] = [
  { id: "deepseek-v4-pro",    label: "DeepSeek V4 Pro",      desc: "Máxima capacidad · tareas complejas" },
  { id: "deepseek-v4-flash",  label: "DeepSeek V4 Flash",    desc: "Rápido y eficiente · menor coste" },
  { id: "deepseek-chat",      label: "DeepSeek V3 (chat)",   desc: "Modelo general estable · recomendado" },
  { id: "deepseek-reasoner",  label: "DeepSeek R1",          desc: "Razonamiento profundo · cadena larga" },
  { id: "deepseek-r1-0528",   label: "DeepSeek R1-0528",     desc: "Snapshot R1 de mayo 2025" },
  { id: "deepseek-v3-0324",   label: "DeepSeek V3-0324",     desc: "Snapshot V3 de marzo 2025" },
  { id: "__custom__",         label: "Otro (escribir ID)",   desc: "" },
];

// Voces locales disponibles, con su motor. Las latinas (Piper) son las más
// naturales y con acento real → primeras y por defecto.
// Voces de catálogo (Piper latino natural + Kokoro). Las clonadas se añaden
// dinámicamente desde el backend (motor chatterbox).
const VOICES: { id: string; engine: string; label: string }[] = [
  // ESPAÑOL NATIVO con GÉNERO FIABLE (Piper) — RECOMENDADAS. Piper es determinista (cada voz
  // es un modelo entrenado), así que el género es ESTABLE — a diferencia de Qwen3-TTS, cuyo
  // género es inestable y se sesga a femenino (incl. clonación), verificado por F0.
  { id: "es_ES-davefx-medium", engine: "piper", label: "Diego · hombre (español) ✓" },
  { id: "es_MX-ald-medium", engine: "piper", label: "Mateo · hombre (México) ✓" },
  { id: "es_MX-claude-high", engine: "piper", label: "Lucía · mujer (México) ✓" },
  { id: "es_AR-daniela-high", engine: "piper", label: "Daniela · mujer (Argentina) ✓" },
  // Qwen3-TTS (MLX) — multiidioma, tiempo real, hablantes NO nativos. ⚠️ El género es
  // INESTABLE en este modelo 0.6B (puede sonar más agudo de lo esperado).
  { id: "serena", engine: "qwen", label: "Qwen3 · Serena (mujer, multiidioma)" },
  { id: "vivian", engine: "qwen", label: "Qwen3 · Vivian (mujer, multiidioma)" },
  { id: "ryan", engine: "qwen", label: "Qwen3 · Ryan (hombre*, multiidioma)" },
  { id: "aiden", engine: "qwen", label: "Qwen3 · Aiden (hombre*, multiidioma)" },
  { id: "ef_dora", engine: "kokoro", label: "Español · Dora (Kokoro, mujer)" },
  { id: "em_alex", engine: "kokoro", label: "Español · Alex (Kokoro, hombre)" },
  { id: "if_sara", engine: "kokoro", label: "Italiano · Sara (mujer)" },
  { id: "im_nicola", engine: "kokoro", label: "Italiano · Nicola (hombre)" },
  { id: "af_heart", engine: "kokoro", label: "English · Heart (mujer)" },
  { id: "am_michael", engine: "kokoro", label: "English · Michael (hombre)" },
];
const DEFAULT_VOICE_ID = "es_MX-ald-medium";

// Las 12 voces diseñadas (VoiceDesign→clonadas) llevan slug «ES-Nombre / IT-Nombre /
// EN-Nombre». Las clasificamos por idioma + género para agruparlas bonito en el selector.
const VOICE_GENDER: Record<string, "m" | "f"> = {
  Mateo: "m", Diego: "m", Valentina: "f", Camila: "f",
  Marco: "m", Luca: "m", Giulia: "f", Sofia: "f",
  James: "m", Ethan: "m", Emma: "f", Charlotte: "f",
};
// Solo mostramos tu voz personal real. Las voces "diseñadas" (ES-/IT-/EN- clonadas con
// Qwen3-TTS 0.6B) se OCULTAN: ese modelo tiene género INESTABLE y sesgo femenino (Diego
// sonaba a mujer, verificado por F0). Para género fiable usa las voces de catálogo Piper.
const CLONE_GROUPS = ["Tu voz personal ★"];
function classifyClone(slug: string): { group: string; label: string } {
  const m = slug.match(/^(ES|IT|EN)-(.+)$/);
  if (m) {
    const lang = { ES: "Español latino", IT: "Italiano", EN: "Inglés" }[m[1] as "ES" | "IT" | "EN"];
    const g = VOICE_GENDER[m[2]];
    const sym = g === "m" ? "· hombre" : g === "f" ? "· mujer" : "";
    return { group: lang, label: `${m[2]} ${sym}` };
  }
  return { group: "Tu voz personal ★", label: `${slug} · clonada ★` };
}

/** Todo lo que Ariel puede cambiar de la voz de AION: motor, voz, velocidad + prueba. */
function VoiceCard() {
  const { lang } = useT();
  const [engine, setEngine] = useState<"auto" | "system">("auto");
  const [voice, setVoice] = useState(DEFAULT_VOICE_ID);
  // Voces del SISTEMA (macOS): el usuario puede elegir CUALQUIERA. "" = automática (por género).
  const [macVoices, setMacVoices] = useState<{ name: string; lang: string; localService: boolean }[]>([]);
  const [sysVoice, setSysVoice] = useState("");
  const [speed, setSpeed] = useState(1);
  const [exaggeration, setExaggeration] = useState(0.6);
  const [testing, setTesting] = useState(false);
  const [testMsg, setTestMsg] = useState<string | null>(null);
  const [savedMsg, setSavedMsg] = useState(false);
  // Voces clonadas (motor chatterbox), cargadas del backend.
  const [cloned, setCloned] = useState<string[]>([]);
  const [cloneName, setCloneName] = useState("");
  const [cloning, setCloning] = useState(false);
  const [cloneMsg, setCloneMsg] = useState<string | null>(null);
  const cloneFile = useRef<HTMLInputElement>(null);

  const refreshCloned = () =>
    ttsVoices()
      .then((r) => {
        const cl = r.cloned || [];
        setCloned(cl);
        // Migración: las voces "diseñadas" (ES-/IT-/EN- clonadas con Qwen) están OCULTAS por
        // género inestable/femenino. Si la voz guardada es una de esas (o un preset retirado),
        // salta al NUEVO defecto Piper masculino (género fiable). La voz personal real ("chile",
        // subidas del usuario) sigue siendo válida.
        const isDesigned = (s: string) => /^(ES|IT|EN)-/.test(s);
        const stored = (typeof localStorage !== "undefined" && localStorage.getItem("aion.voice.name")) || "";
        const valid =
          VOICES.some((v) => v.id === stored) || (cl.includes(stored) && !isDesigned(stored));
        if (stored && !valid) {
          save("aion.voice.name", DEFAULT_VOICE_ID);
          save("aion.voice.engine", "piper");
          setVoice(DEFAULT_VOICE_ID);
        }
      })
      .catch(() => {});
  useEffect(() => {
    if (typeof localStorage === "undefined") return;
    if (localStorage.getItem("aion.voice") === "system") setEngine("system");
    setVoice(localStorage.getItem("aion.voice.name") || DEFAULT_VOICE_ID);
    setSysVoice(localStorage.getItem("aion.voice.system") || "");
    setSpeed(parseFloat(localStorage.getItem("aion.voice.speed") || "1") || 1);
    setExaggeration(parseFloat(localStorage.getItem("aion.voice.exaggeration") || "0.6") || 0.6);
    refreshCloned();
  }, []);
  // Carga las voces del Mac (getVoices() llega async → escucha 'voiceschanged').
  useEffect(() => {
    const load = () => setMacVoices(systemVoices());
    load();
    if (typeof window !== "undefined" && "speechSynthesis" in window) {
      window.speechSynthesis.addEventListener("voiceschanged", load);
      return () => window.speechSynthesis.removeEventListener("voiceschanged", load);
    }
  }, []);
  // Elige una voz concreta del Mac ("" = automática por género).
  const chooseSysVoice = (name: string) => {
    setSysVoice(name);
    if (name) save("aion.voice.system", name);
    else { try { localStorage.removeItem("aion.voice.system"); } catch { /* */ } }
  };

  const save = (k: string, v: string) => { try { localStorage.setItem(k, v); } catch { /* */ } };
  // El motor de una voz: clonada → Qwen3 (natural + tiempo real); si no, el del catálogo.
  const voiceEngine = (id: string) =>
    cloned.includes(id) ? "qwen" : VOICES.find((v) => v.id === id)?.engine || "kokoro";
  const chooseVoice = (id: string) => {
    setVoice(id);
    save("aion.voice.name", id);
    save("aion.voice.engine", voiceEngine(id));
  };

  async function uploadClone(file: File) {
    const name = cloneName.trim();
    if (!name) { setCloneMsg("Ponle un nombre a la voz (p. ej. Chile)."); return; }
    setCloning(true);
    setCloneMsg(null);
    try {
      const r = await ttsCloneUpload(name, file);
      if (r.ok && r.voice) {
        await refreshCloned();
        chooseVoice(r.voice);
        setCloneName("");
        setCloneMsg(`✅ Voz «${r.voice}» añadida. Pruébala con «Probar voz» (la 1ª vez tarda unos segundos).`);
      } else {
        setCloneMsg(`⚠️ ${r.error || "no pude añadir la voz"}`);
      }
    } catch (e) {
      setCloneMsg(`⚠️ ${e instanceof Error ? e.message : "error"}`);
    } finally {
      setCloning(false);
    }
  }

  async function removeClone(name: string) {
    await ttsCloneRemove(name).catch(() => {});
    if (voice === name) chooseVoice(DEFAULT_VOICE_ID);
    await refreshCloned();
  }

  async function test() {
    setTestMsg(null);
    setTesting(true);
    try {
      if (engine === "system") {
        const u = new SpeechSynthesisUtterance("Hola Ariel, soy AION. Así sueno.");
        u.lang = lang === "it" ? "it-IT" : lang === "en" ? "en-US" : "es-ES";
        u.rate = speed;
        const v = pickSystemVoice(u.lang); // respeta la voz de macOS elegida (o el género)
        if (v) u.voice = v;
        window.speechSynthesis.cancel();
        window.speechSynthesis.speak(u);
      } else {
        if (voiceEngine(voice) === "qwen" || voiceEngine(voice) === "chatterbox") {
          setTestMsg("Generando con tu voz natural… (la 1ª vez puede tardar un momento)");
        }
        const blob = await ttsSpeak("Hola Ariel, soy AION. Así sueno con esta voz, más natural.", lang, {
          voice,
          engine: voiceEngine(voice),
          speed,
          exaggeration,
        });
        setTestMsg(null);
        await playTtsBlob(blob);
      }
    } catch (e) {
      setTestMsg(`No sonó (${e instanceof Error ? e.message : String(e)}). Se usará la voz del sistema.`);
    } finally {
      setTesting(false);
    }
  }

  const ENGINES: { key: "auto" | "system"; label: string; note: string }[] = [
    { key: "auto", label: "Voz propia de AION", note: "Natural y local (Kokoro). Respaldo a la del sistema." },
    { key: "system", label: "Voz del sistema", note: "La voz integrada de macOS. Instantánea." },
  ];

  return (
    <div className="card">
      <h2 className="t-section mb-1 flex items-center gap-2" style={{ color: "var(--text-2)" }}>
        <Icon name="volume" size={16} /> Voz
      </h2>
      <p className="text-sm mb-3" style={{ color: "var(--text-3)" }}>
        Cómo suena AION cuando lee sus respuestas y en el modo voz.
      </p>

      {/* Motor */}
      <div className="flex flex-col sm:flex-row gap-2 mb-4">
        {ENGINES.map((o) => {
          const active = engine === o.key;
          return (
            <button
              key={o.key}
              onClick={() => { setEngine(o.key); save("aion.voice", o.key); }}
              className="flex-1 text-left px-4 py-3 rounded-xl transition-all"
              style={{
                background: active ? "var(--accent-subtle)" : "var(--surface-2)",
                border: `1px solid ${active ? "var(--accent)" : "transparent"}`,
              }}
            >
              <div className="text-sm font-semibold" style={{ color: active ? "var(--gold-deep)" : "var(--text-1)" }}>
                {o.label}
              </div>
              <div className="text-xs mt-0.5" style={{ color: "var(--text-3)" }}>{o.note}</div>
            </button>
          );
        })}
      </div>

      {/* Voz concreta + velocidad (solo aplican a la voz propia) */}
      <div className="grid sm:grid-cols-2 gap-3">
        <div>
          <label className="text-xs block mb-1" style={{ color: "var(--text-3)" }}>Voz</label>
          {engine === "system" ? (
            // Cualquier voz instalada en el Mac. "" = automática por el género del onboarding.
            <select
              className="input"
              value={sysVoice}
              onChange={(e) => chooseSysVoice(e.target.value)}
              style={{ background: "var(--surface-1)", color: "var(--text-1)" }}
            >
              <option value="">Automática (según femenina/masculina del onboarding)</option>
              {(() => {
                const es = macVoices.filter((v) => v.lang?.toLowerCase().startsWith("es"));
                const otras = macVoices.filter((v) => !v.lang?.toLowerCase().startsWith("es"));
                return (
                  <>
                    {es.length > 0 && (
                      <optgroup label="Español">
                        {es.map((v) => <option key={v.name} value={v.name}>{v.name} · {v.lang}</option>)}
                      </optgroup>
                    )}
                    {otras.length > 0 && (
                      <optgroup label="Otros idiomas">
                        {otras.map((v) => <option key={v.name} value={v.name}>{v.name} · {v.lang}</option>)}
                      </optgroup>
                    )}
                  </>
                );
              })()}
            </select>
          ) : (
            <select
              className="input"
              value={voice}
              onChange={(e) => chooseVoice(e.target.value)}
              style={{ background: "var(--surface-1)", color: "var(--text-1)" }}
            >
              {CLONE_GROUPS.map((grp) => {
                const items = cloned.filter((c) => classifyClone(c).group === grp);
                if (!items.length) return null;
                return (
                  <optgroup key={grp} label={grp}>
                    {items.map((c) => (
                      <option key={`c-${c}`} value={c}>{classifyClone(c).label}</option>
                    ))}
                  </optgroup>
                );
              })}
              <optgroup label="Catálogo — español nativo (género fiable ✓) y multiidioma">
                {VOICES.map((v) => <option key={v.id} value={v.id}>{v.label}</option>)}
              </optgroup>
            </select>
          )}
        </div>
        <div>
          <label className="text-xs block mb-1" style={{ color: "var(--text-3)" }}>
            Velocidad · {speed.toFixed(2)}×
          </label>
          <input
            type="range"
            min={0.7}
            max={1.4}
            step={0.05}
            value={speed}
            disabled={engine === "system"}
            onChange={(e) => { const s = parseFloat(e.target.value); setSpeed(s); save("aion.voice.speed", String(s)); }}
            className="w-full"
            style={{ accentColor: "var(--accent)" }}
          />
        </div>
      </div>

      {/* Expresividad — solo aplica a la voz clonada (Chatterbox). */}
      {voiceEngine(voice) === "chatterbox" && engine !== "system" && (
        <div className="mt-3">
          <label className="text-xs block mb-1" style={{ color: "var(--text-3)" }}>
            Expresividad / énfasis · {Math.round(exaggeration * 100)}%
          </label>
          <input
            type="range"
            min={0.3}
            max={0.95}
            step={0.05}
            value={exaggeration}
            onChange={(e) => { const v = parseFloat(e.target.value); setExaggeration(v); save("aion.voice.exaggeration", String(v)); }}
            className="w-full"
            style={{ accentColor: "var(--accent)" }}
          />
          <p className="text-[11px] mt-0.5" style={{ color: "var(--text-3)" }}>
            Más alto = más emoción y énfasis (puede sonar más teatral); más bajo = más sobrio.
          </p>
        </div>
      )}

      <div className="flex items-center gap-3 mt-4">
        <button className="btn inline-flex items-center gap-1.5" onClick={test} disabled={testing}>
          <Icon name="play" size={15} /> {testing ? "Sonando…" : "Probar voz"}
        </button>
        <button
          className="btn inline-flex items-center gap-1.5"
          onClick={() => {
            // Re-persiste todas las preferencias de voz (ya se autoguardan en cada cambio;
            // este botón da confirmación visible y asegura el estado actual en disco).
            save("aion.voice.name", voice);
            save("aion.voice.engine", voiceEngine(voice));
            save("aion.voice.speed", String(speed));
            save("aion.voice.exaggeration", String(exaggeration));
            save("aion.voice", engine === "system" ? "system" : "auto");
            setSavedMsg(true);
            setTimeout(() => setSavedMsg(false), 2200);
          }}
        >
          <Icon name="check" size={15} /> Guardar preferencia
        </button>
        {savedMsg && <span className="text-xs" style={{ color: "var(--accent)" }}>✓ Preferencia guardada</span>}
        {testMsg && <span className="text-xs" style={{ color: "var(--text-3)" }}>{testMsg}</span>}
      </div>

      {/* ── Clonar voz ─────────────────────────────────────────────────── */}
      <div className="mt-5 pt-4" style={{ borderTop: "1px solid var(--border)" }}>
        <h3 className="text-sm font-semibold mb-1" style={{ color: "var(--text-1)" }}>
          Clonar una voz
        </h3>
        <p className="text-xs mb-3" style={{ color: "var(--text-3)" }}>
          Sube un clip limpio de <strong>10-20&nbsp;s</strong> (voz sola, sin música ni ruido) y AION
          la clona con <strong>Qwen3-TTS</strong> conservando acento y timbre. Es muy realista y
          <strong> en tiempo real</strong> (RTF&nbsp;~0.3), así que sirve también para conversar en
          el modo voz en vivo, no solo al pulsar «Escuchar».
        </p>
        <div className="flex flex-col sm:flex-row gap-2">
          <input
            className="input"
            placeholder="Nombre (p. ej. Chile)"
            value={cloneName}
            onChange={(e) => setCloneName(e.target.value)}
          />
          <input
            ref={cloneFile}
            type="file"
            accept="audio/*,.wav,.mp3,.m4a,.flac,.ogg"
            className="hidden"
            onChange={(e) => { const f = e.target.files?.[0]; if (f) uploadClone(f); e.target.value = ""; }}
          />
          <button
            className="btn shrink-0 inline-flex items-center gap-1.5"
            disabled={cloning || !cloneName.trim()}
            onClick={() => cloneFile.current?.click()}
          >
            <Icon name="upload" size={15} /> {cloning ? "Clonando…" : "Subir clip y clonar"}
          </button>
        </div>
        {cloneMsg && <p className="text-xs mt-2" style={{ color: "var(--text-2)" }}>{cloneMsg}</p>}
        {cloned.filter((c) => !/^(ES|IT|EN)-/.test(c)).length > 0 && (
          <div className="flex flex-wrap gap-2 mt-3">
            {cloned.filter((c) => !/^(ES|IT|EN)-/.test(c)).map((c) => (
              <span
                key={c}
                className="inline-flex items-center gap-1.5 text-xs px-2.5 py-1 rounded-full"
                style={{ background: "var(--surface-2)", color: "var(--text-2)" }}
              >
                {c}
                <button
                  onClick={() => removeClone(c)}
                  className="opacity-60 hover:opacity-100"
                  style={{ color: "#ef4444" }}
                  title="Eliminar voz clonada"
                  aria-label={`Eliminar ${c}`}
                >
                  ✕
                </button>
              </span>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

export default function SettingsPage() {
  const { t, lang, setLang } = useT();
  const [email, setEmail] = useState<string | null>(null);
  const [dark, setDark] = useState(false);
  const [scan, setScan] = useState<SystemScan | null>(null);
  const [catalog, setCatalog] = useState<ModelOption[]>([]);
  const [current, setCurrent] = useState<string>("");
  const [installed, setInstalled] = useState<InstalledModel[]>([]);
  const [busyModel, setBusyModel] = useState<string>("");
  const [pullPct, setPullPct] = useState<number>(0);
  const [modelMsg, setModelMsg] = useState<string>("");
  const [backupMsg, setBackupMsg] = useState<string>("");
  const [ident, setIdent] = useState<AionIdentity | null>(null);
  const [a2a, setA2a] = useState<A2aConfig>({ enabled: false, token: "", peers: [] });
  const [a2aMsg, setA2aMsg] = useState<string>("");
  const [newPeer, setNewPeer] = useState({ name: "", url: "" });
  const [sensors, setSensors] = useState<SensorConfig>({ enabled: false, lat: null, lon: null, place: "" });
  const [sensorMsg, setSensorMsg] = useState<string>("");
  const [cc, setCc] = useState<ClaudeCodeStatus>({ enabled: false, auto_brief: false, registered: false, cli_found: true });
  const [ccBusy, setCcBusy] = useState(false);
  const [ccMsg, setCcMsg] = useState<string>("");

  // ── Proveedor del motor: LOCAL (Ollama, privado) o API CLOUD (Google / DeepSeek) ──
  const [prov, setProv] = useState<ProviderState | null>(null);
  const [provSel, setProvSel] = useState<ProvKey>("local");
  const [provModel, setProvModel] = useState<string>("");
  const [provKey, setProvKey] = useState<string>("");
  const [customUrl, setCustomUrl] = useState<string>("");
  const [provBusy, setProvBusy] = useState(false);
  const [provMsg, setProvMsg] = useState<string>("");
  const [dsCustomModel, setDsCustomModel] = useState(false);

  function applyProv(p: ProviderState) {
    setProv(p);
    if (p.kind !== "external") {
      setProvSel("local");
    } else if (p.base_url.includes("googleapis")) {
      setProvSel("google");
    } else if (p.base_url.includes("deepseek")) {
      setProvSel("deepseek");
      const known = DEEPSEEK_MODELS.some(m => m.id !== "__custom__" && m.id === p.model);
      setDsCustomModel(!known && p.model !== "");
    } else {
      setProvSel("custom");
    }
    setProvModel(p.model);
  }

  async function saveProvider(sel: ProvKey, model: string, key: string) {
    if (provBusy) return;
    setProvBusy(true);
    setProvMsg("");
    try {
      if (sel === "local") {
        await providerSet({ kind: "local", model: model || current || "gemma4-reason" });
        setProvMsg("✅ Motor local activo · privacidad total, nada sale del Mac.");
      } else {
        const preset = PROVIDERS[sel];
        const base_url = sel === "custom" ? customUrl.trim() : preset.base_url;
        if (!base_url) {
          setProvMsg("⚠️ Indica la Base URL del proveedor.");
          setProvBusy(false);
          return;
        }
        const usingExisting = !key && prov?.has_key && prov.base_url === base_url;
        if (!key && !usingExisting) {
          setProvMsg("⚠️ Pega tu API key para activar este proveedor.");
          setProvBusy(false);
          return;
        }
        await providerSet({
          kind: "external",
          model: model || preset.defaultModel,
          base_url,
          api_key: key, // vacío = el backend conserva la key ya guardada
        });
        setProvMsg(`✅ ${preset.label} activo · tus mensajes saldrán del Mac hacia esta API.`);
        setProvKey("");
      }
      await providerGet().then(applyProv).catch(() => {});
      await refreshModels();
    } catch (e) {
      setProvMsg(`⚠️ ${e instanceof Error ? e.message : "error"}`);
    } finally {
      setProvBusy(false);
    }
  }

  useEffect(() => {
    getIdentity().then(setIdent);
    a2aGet().then((r) => setA2a(r.config));
    sensorsGet().then(setSensors).catch(() => {});
    claudeCodeGet().then(setCc).catch(() => {});
    providerGet().then(applyProv).catch(() => {});
  }, []);

  const ccConnected = cc.enabled && cc.registered;
  async function connectCc() {
    if (ccBusy) return;
    setCcBusy(true);
    setCcMsg(t("cc.connecting"));
    const r = await claudeCodeConnect(cc.auto_brief).catch(() => ({ ok: false, error: "sin respuesta" }));
    if (r.ok) {
      setCcMsg(`✓ ${t("cc.connected")}`);
    } else {
      setCcMsg(r.error === "cli_not_found" ? t("cc.installHint") : `⚠️ ${r.error ?? "error"}`);
    }
    setCc(await claudeCodeGet());
    setCcBusy(false);
  }
  async function disconnectCc() {
    if (ccBusy) return;
    setCcBusy(true);
    await claudeCodeDisconnect().catch(() => {});
    setCc(await claudeCodeGet());
    setCcMsg(t("cc.notConnected"));
    setCcBusy(false);
  }
  async function testCc() {
    setCcMsg("…");
    const r = await claudeCodeTest().catch(() => null);
    if (!r) {
      setCcMsg("⚠️ AION no responde");
      return;
    }
    if (!r.cli_found) setCcMsg(t("cc.installHint"));
    else if (r.ok)
      setCcMsg(
        `✓ ${t("cc.connected")} · ${t("cc.lastSeen")}: ${r.last_seen_at ? new Date(r.last_seen_at).toLocaleString() : t("cc.never")}`,
      );
    else setCcMsg(t("cc.notConnected"));
  }
  async function toggleBrief(next: boolean) {
    setCc({ ...cc, auto_brief: next });
    await claudeCodeSet({ auto_brief: next }).catch(() => {});
  }

  async function saveSensors(next: SensorConfig) {
    setSensors(next);
    await sensorsSet(next).catch(() => {});
    setSensorMsg(next.enabled ? "Conciencia de entorno activada." : "Desactivada.");
    setTimeout(() => setSensorMsg(""), 2500);
  }
  function useMyLocation() {
    if (!navigator.geolocation) {
      setSensorMsg("Tu navegador no permite ubicación.");
      return;
    }
    setSensorMsg("Obteniendo ubicación…");
    navigator.geolocation.getCurrentPosition(
      (pos) =>
        saveSensors({
          ...sensors,
          enabled: true,
          lat: Number(pos.coords.latitude.toFixed(3)),
          lon: Number(pos.coords.longitude.toFixed(3)),
        }),
      () => setSensorMsg("No se pudo obtener la ubicación."),
    );
  }

  function saveA2a(next: A2aConfig) {
    setA2a(next);
    a2aSet(next);
  }
  async function testPeer(url: string) {
    setA2aMsg("Contactando…");
    const r = await a2aSend(url, "Hola, ¿quién eres? Preséntate con tu nombre e id.");
    setA2aMsg(r.reply ? `${r.name ?? "Agente"}: ${r.reply}` : `Error: ${r.error ?? "sin respuesta"}`);
  }

  async function importAgent(file: File) {
    setBackupMsg("Importando…");
    const b64 = await new Promise<string>((res, rej) => {
      const r = new FileReader();
      r.onload = () => res((r.result as string).split(",")[1] ?? "");
      r.onerror = () => rej(r.error);
      r.readAsDataURL(file);
    });
    const r = await agentImport(b64);
    setBackupMsg(
      r.ok
        ? `✓ Restaurados ${r.restored} archivos. Reinicia AION (⌘Q) para recargar todo.`
        : `Error: ${r.error ?? "no se pudo importar"}`,
    );
  }

  // MIGRAR: descarga el .aion (con id) y, en cuanto está a salvo, BORRA este equipo
  // automáticamente (sin preguntar): el MISMO agente se mudó, no quedan copias.
  async function migrate() {
    setBackupMsg("Migrando: descargando tu AION…");
    const ok = await downloadAgent("keep", "migrar", "aion-migrar.aion");
    if (!ok) {
      setBackupMsg("No se pudo descargar; no se borró nada.");
      return;
    }
    await agentWipe();
    setBackupMsg(
      "✓ Migración lista: tu AION está en el archivo .aion y este equipo quedó vacío. Súbelo en el otro sistema; aquí nacerá un AION nuevo al reiniciar.",
    );
  }
  async function backupRepair() {
    setBackupMsg("Creando respaldo…");
    const ok = await downloadAgent("keep", "reparar", "aion-respaldo.aion");
    setBackupMsg(ok ? "✓ Respaldo descargado. Sigues con tu mismo AION aquí." : "No se pudo crear el respaldo.");
  }
  async function cloneAgent() {
    setBackupMsg("Creando clon (sin id)…");
    const ok = await downloadAgent("strip", "clonar", "aion-clon.aion");
    setBackupMsg(ok ? "✓ Clon descargado. Al subirlo en otro sistema nacerá un agente nuevo (id y nombre propios)." : "No se pudo crear el clon.");
  }

  async function refreshModels() {
    try { setInstalled(await modelsInstalled()); } catch { /* */ }
    try { setCurrent((await status()).engine.replace(/^ollama:/, "")); } catch { /* */ }
  }
  // Compara nombres de modelo por su nombre COMPLETO (incluido el tag de tamaño),
  // normalizando solo la etiqueta implícita `:latest`. Antes se comparaba por la base
  // de familia (`nombre.split(":")[0]`), lo que marcaba "instalado"/"En uso" a TODOS los
  // tamaños de una misma familia: con qwen2.5-abliterate:7b activo, el :14b también se
  // iluminaba porque comparten base. El tag (7b/14b) es justo lo que los distingue.
  const normName = (s: string) => (s.includes(":") ? s : `${s}:latest`);
  const isInstalled = (ollama: string) =>
    installed.some((i) => normName(i.name) === normName(ollama));
  const isCurrent = (ollama: string) => normName(current) === normName(ollama);

  // Usa un modelo: si no está instalado, lo descarga (con progreso) y luego lo activa.
  async function useModel(m: ModelOption) {
    if (busyModel) return;
    setBusyModel(m.ollama_name); setModelMsg(""); setPullPct(0);
    try {
      if (!isInstalled(m.ollama_name)) {
        setModelMsg(`Descargando ${m.name}…`);
        await modelsPull(m.ollama_name, (e) => {
          if (typeof e.percent === "number") setPullPct(e.percent);
        });
      }
      await providerSet({ kind: "local", model: m.ollama_name });
      setModelMsg(`✅ ${m.name} activado`);
      await refreshModels();
    } catch (e) {
      setModelMsg(`⚠️ ${e instanceof Error ? e.message : "error"}`);
    } finally {
      setBusyModel(""); setPullPct(0);
    }
  }

  async function removeModel(ollama: string) {
    if (!confirm(`¿Eliminar el modelo «${ollama}»? Liberará espacio en disco.`)) return;
    try { await modelsRemove(ollama); await refreshModels(); }
    catch (e) { setModelMsg(`⚠️ ${e instanceof Error ? e.message : "error"}`); }
  }
  const [creds, setCreds] = useState<CredMeta[]>([]);
  const [cHost, setCHost] = useState("");
  const [cUser, setCUser] = useState("");
  const [cPass, setCPass] = useState("");
  const [cBusy, setCBusy] = useState(false);
  const [apiKeys, setApiKeys] = useState<ApiKeyMeta[]>([]);
  const [keyDrafts, setKeyDrafts] = useState<Record<string, string>>({});
  const [keyBusy, setKeyBusy] = useState("");
  const [keyMsg, setKeyMsg] = useState("");

  async function refreshApiKeys() {
    try {
      setApiKeys(await apiKeysList());
    } catch {
      /* vacío */
    }
  }
  async function saveApiKey(provider: string) {
    const val = (keyDrafts[provider] ?? "").trim();
    if (!val || keyBusy) return;
    setKeyBusy(provider);
    setKeyMsg("");
    try {
      await apiKeySet(provider, val);
      setKeyDrafts({ ...keyDrafts, [provider]: "" });
      setKeyMsg("✅ Guardada");
      await refreshApiKeys();
    } catch (e) {
      setKeyMsg(`⚠️ ${e instanceof Error ? e.message : "error"}`);
    } finally {
      setKeyBusy("");
    }
  }
  async function removeApiKey(provider: string) {
    setKeyBusy(provider);
    try {
      await apiKeySet(provider, "");
      await refreshApiKeys();
    } finally {
      setKeyBusy("");
    }
  }

  async function refreshCreds() {
    try {
      setCreds(await credentialsList());
    } catch {
      /* vacío */
    }
  }
  async function addCred() {
    if (!cHost.trim() || !cUser.trim() || !cPass || cBusy) return;
    setCBusy(true);
    try {
      await credentialSet(cHost.trim(), cUser.trim(), cPass);
      setCHost(""); setCUser(""); setCPass("");
      await refreshCreds();
    } finally {
      setCBusy(false);
    }
  }

  useEffect(() => {
    setEmail(localStorage.getItem("aion_email"));
    const d = localStorage.getItem("aion_theme") === "dark";
    setDark(d);
    document.documentElement.classList.toggle("dark", d);
    systemScan()
      .then((r) => { setScan(r.scan); setCatalog(r.catalog); })
      .catch(() => {});
    refreshModels();
    refreshCreds();
    refreshApiKeys();
  }, []);

  function toggleTheme() {
    const d = !dark;
    setDark(d);
    document.documentElement.classList.toggle("dark", d);
    localStorage.setItem("aion_theme", d ? "dark" : "light");
  }

  return (
    <AppShell title={t("nav.settings")}>
      <div className="max-w-6xl mx-auto px-3 py-6 flex flex-col gap-6">
        {/* ── CABECERA (patrón de Mente) ── */}
        <div className="card flex items-center gap-4" style={{ boxShadow: "var(--shadow-elevated)" }}>
          <span
            className="w-12 h-12 rounded-2xl flex items-center justify-center shrink-0"
            style={{ background: "var(--accent-subtle)", color: "var(--gold-deep)" }}
          >
            <Icon name="settings" size={24} />
          </span>
          <div className="min-w-0">
            <div className="font-display text-xl font-bold" style={{ color: "var(--text-1)" }}>
              {t("nav.settings")}
            </div>
            <p className="text-sm mt-0.5 max-w-xl" style={{ color: "var(--text-3)" }}>
              Personaliza AION: cuenta, idioma, motor de IA, voz y privacidad. Todo se guarda en
              este dispositivo.
            </p>
          </div>
        </div>

        <div className="card">
          <h2 className="t-section mb-3" style={{ color: "var(--text-2)" }}>
            {t("settings.account")}
          </h2>
          <p className="text-sm" style={{ color: "var(--text-2)" }}>
            Email: <strong>{email ?? "—"}</strong>
          </p>
          <p className="text-xs mt-1" style={{ color: "var(--text-3)" }}>
            {t("settings.localNote")}
          </p>
        </div>

        <VoiceCard />

        {/* ── Idioma (ES / IT / EN) ── */}
        <div className="card">
          <h2 className="t-section mb-1" style={{ color: "var(--text-2)" }}>
            {t("settings.language")}
          </h2>
          <p className="text-sm mb-3" style={{ color: "var(--text-3)" }}>
            {t("settings.languageNote")}
          </p>
          <div className="flex gap-2">
            {LANGS.map((l) => {
              const active = lang === l.code;
              return (
                <button
                  key={l.code}
                  onClick={() => setLang(l.code)}
                  className="flex items-center gap-2 px-4 py-2 rounded-xl text-sm transition-all"
                  style={{
                    background: active ? "var(--accent-subtle)" : "var(--surface-2)",
                    color: active ? "var(--gold-deep)" : "var(--text-2)",
                    fontWeight: active ? 600 : 500,
                    border: `1px solid ${active ? "var(--accent)" : "transparent"}`,
                  }}
                >
                  <span
                    className="text-[10px] font-bold px-1.5 py-0.5 rounded"
                    style={{ background: "var(--surface-1)", color: "var(--text-3)" }}
                  >
                    {l.flag}
                  </span>
                  {l.label}
                </button>
              );
            })}
          </div>
        </div>

        {/* ── Modelos LLM locales ── */}
        <div className="card">
          <h2 className="t-section mb-1" style={{ color: "var(--text-2)" }}>
            {t("settings.models")}
          </h2>
          {scan && (
            <p className="text-xs mb-3" style={{ color: "var(--text-3)" }}>
              {scan.cpu_cores} CPU · {scan.ram_gb} GB RAM · {scan.gpu} · {t("settings.tier")}:{" "}
              <strong style={{ color: "var(--accent)" }}>{scan.tier}</strong>
            </p>
          )}
          {current && (
            <p className="text-sm mb-3" style={{ color: "var(--text-2)" }}>
              {t("settings.current")}: <strong>{current}</strong>
            </p>
          )}
          <div className="flex flex-col gap-2">
            {catalog.map((m) => {
              const fits = scan ? m.tier === scan.tier || m.size_gb <= scan.ram_gb * 0.6 : true;
              const cur = isCurrent(m.ollama_name);
              const inst = isInstalled(m.ollama_name);
              const busy = busyModel === m.ollama_name;
              return (
                <div
                  key={m.id}
                  className="flex items-center gap-2 px-3 py-2 rounded-lg"
                  style={{ background: cur ? "var(--accent-subtle)" : "var(--surface-1)" }}
                >
                  <div className="min-w-0 flex-1">
                    <div className="text-sm font-medium truncate">
                      {m.name}{" "}
                      <span className="text-xs font-normal" style={{ color: "var(--text-3)" }}>· {m.size_gb} GB</span>
                      {inst && <span className="text-[10px] ml-2" style={{ color: "var(--text-3)" }}>instalado</span>}
                      {fits && <span className="text-[10px] ml-2" style={{ color: "var(--accent)" }}>· {t("settings.fits")}</span>}
                    </div>
                    <div className="text-xs truncate" style={{ color: "var(--text-3)" }}>
                      {busy && pullPct > 0 ? `Descargando… ${pullPct}%` : m.note}
                    </div>
                  </div>
                  {cur ? (
                    <span className="text-[10px] font-semibold px-2 py-0.5 rounded-full shrink-0" style={{ background: "var(--accent)", color: "#04201f" }}>
                      {t("settings.inUse")}
                    </span>
                  ) : (
                    <button
                      onClick={() => useModel(m)}
                      disabled={!!busyModel}
                      className="text-xs px-3 py-1.5 rounded-full shrink-0"
                      style={{ background: "var(--ink)", color: "#fff", opacity: busyModel && !busy ? 0.4 : 1 }}
                    >
                      {busy ? "…" : inst ? t("settings.useModel") : t("settings.installUse")}
                    </button>
                  )}
                  {inst && !cur && !busyModel && (
                    <button onClick={() => removeModel(m.ollama_name)} className="text-xs shrink-0 opacity-60 hover:opacity-100" style={{ color: "#ef4444" }} title={t("settings.removeModel")}>✕</button>
                  )}
                </div>
              );
            })}
          </div>
          {modelMsg && <p className="mt-3 text-sm" style={{ color: "var(--accent)" }}>{modelMsg}</p>}
          <p className="text-xs mt-2" style={{ color: "var(--text-3)" }}>
            {t("settings.modelsNote2")}
          </p>
        </div>

        {/* ── Proveedor del motor: LOCAL (privado) o API CLOUD ── */}
        <div className="card">
          <h2 className="t-section mb-1 flex items-center gap-2" style={{ color: "var(--text-2)" }}>
            <Icon name="cpu" size={16} /> Proveedor del motor
          </h2>
          <p className="text-xs mb-3" style={{ color: "var(--text-3)" }}>
            Elige con qué cerebro piensa AION. <strong>Local</strong>: corre en tu Mac, nada
            sale de aquí (máxima privacidad). <strong>API cloud</strong>: respuestas más rápidas y
            potentes, pero tus mensajes viajan al proveedor que elijas. Puedes cambiar cuando
            quieras; los embeddings y la visión siguen siendo siempre locales.
          </p>

          {/* Estado activo */}
          <div
            className="rounded-lg px-3 py-2 mb-3 text-xs flex items-center gap-2"
            style={{ background: "var(--surface-2)", color: "var(--text-2)" }}
          >
            <span
              className="inline-block w-2 h-2 rounded-full"
              style={{ background: prov?.kind === "external" ? "#f59e0b" : "var(--accent)" }}
            />
            {prov?.kind === "external" ? (
              <span>Activo: <strong>API cloud</strong> · {prov.model} {prov.has_key ? "· key guardada 🔒" : "· sin key ⚠️"}</span>
            ) : (
              <span>Activo: <strong>Local (Ollama)</strong> · {prov?.model ?? current ?? "—"} · privacidad total</span>
            )}
          </div>

          {/* Selector de proveedor */}
          <div className="flex flex-wrap gap-2 mb-3">
            {(["local", "google", "deepseek", "custom"] as ProvKey[]).map((k) => {
              const active = provSel === k;
              const label = k === "local" ? "Local (privado)" : PROVIDERS[k].label;
              return (
                <button
                  key={k}
                  onClick={() => {
                    setProvSel(k);
                    setProvMsg("");
                    if (k !== "local") {
                      const model = prov?.kind === "external" && prov.base_url === PROVIDERS[k].base_url
                        ? prov.model
                        : PROVIDERS[k].defaultModel;
                      setProvModel(model);
                      if (k === "deepseek") {
                        const known = DEEPSEEK_MODELS.some(m => m.id !== "__custom__" && m.id === model);
                        setDsCustomModel(!known && model !== "");
                      }
                    }
                  }}
                  className="px-3 py-1.5 rounded-xl text-xs transition-all"
                  style={{
                    background: active ? "var(--accent-subtle)" : "var(--surface-1)",
                    color: active ? "var(--gold-deep)" : "var(--text-2)",
                    fontWeight: active ? 600 : 500,
                    border: `1px solid ${active ? "var(--accent)" : "transparent"}`,
                  }}
                >
                  {label}
                </button>
              );
            })}
          </div>

          {provSel === "local" ? (
            <p className="text-xs mb-3 px-3 py-2 rounded-lg" style={{ background: "var(--surface-1)", color: "var(--text-3)" }}>
              🔒 Todo se queda en tu Mac. El modelo concreto se elige arriba, en «{t("settings.models")}».
            </p>
          ) : (
            <div className="flex flex-col gap-2 mb-3">
              {provSel === "custom" && (
                <input
                  className="input"
                  placeholder="Base URL (https://… /v1)"
                  value={customUrl}
                  onChange={(e) => { setCustomUrl(e.target.value); setProvMsg(""); }}
                  autoComplete="off"
                />
              )}

              {provSel === "deepseek" ? (
                <>
                  <select
                    className="input"
                    value={dsCustomModel ? "__custom__" : (DEEPSEEK_MODELS.find(m => m.id === provModel) ? provModel : "__custom__")}
                    onChange={(e) => {
                      const v = e.target.value;
                      if (v === "__custom__") {
                        setDsCustomModel(true);
                        setProvModel("");
                      } else {
                        setDsCustomModel(false);
                        setProvModel(v);
                      }
                    }}
                    style={{ background: "var(--surface-1)", color: "var(--text-1)" }}
                  >
                    {DEEPSEEK_MODELS.map((m) => (
                      <option key={m.id} value={m.id}>
                        {m.label}{m.desc ? ` — ${m.desc}` : ""}
                      </option>
                    ))}
                  </select>
                  {dsCustomModel && (
                    <input
                      className="input"
                      placeholder="ID del modelo (p. ej. deepseek-v5-pro)"
                      value={provModel}
                      onChange={(e) => setProvModel(e.target.value)}
                      autoComplete="off"
                      autoFocus
                    />
                  )}
                </>
              ) : (
                <input
                  className="input"
                  placeholder="Modelo (p. ej. gemini-2.5-flash, deepseek-chat)"
                  value={provModel}
                  onChange={(e) => setProvModel(e.target.value)}
                  autoComplete="off"
                />
              )}
              <input
                className="input"
                type="password"
                placeholder={prov?.has_key && prov.base_url === PROVIDERS[provSel].base_url ? "API key guardada — déjalo vacío para conservarla" : PROVIDERS[provSel].keyHint}
                value={provKey}
                onChange={(e) => setProvKey(e.target.value)}
                autoComplete="new-password"
              />
              <p className="text-[11px] px-1" style={{ color: "#f59e0b" }}>
                ⚠️ Con esta opción, lo que escribas a AION se envía a {PROVIDERS[provSel].label}. La key se guarda cifrada en tu Mac (permisos 0600).
              </p>
            </div>
          )}

          <button
            className="btn"
            disabled={provBusy}
            onClick={() => saveProvider(provSel, provModel, provKey)}
            style={{ background: "var(--ink)", color: "#fff", opacity: provBusy ? 0.5 : 1 }}
          >
            {provBusy ? "Guardando…" : provSel === "local" ? "Usar motor local" : `Activar ${PROVIDERS[provSel].label}`}
          </button>
          {provMsg && <p className="mt-3 text-sm" style={{ color: "var(--accent)" }}>{provMsg}</p>}
        </div>

        <div className="card flex items-center justify-between">
          <div>
            <h2 className="t-section" style={{ color: "var(--text-2)" }}>
              {t("settings.appearance")}
            </h2>
            <p className="text-sm mt-1" style={{ color: "var(--text-3)" }}>
              {t("settings.themeIs")} {dark ? t("settings.dark") : t("settings.light")}
            </p>
          </div>
          <button className="btn" onClick={toggleTheme}>
            {t("settings.switchTo")} {dark ? t("settings.light") : t("settings.dark")}
          </button>
        </div>

        <div className="card">
          <h2 className="t-section mb-2" style={{ color: "var(--text-2)" }}>
            {t("settings.governance")}
          </h2>
          <p className="text-sm" style={{ color: "var(--text-2)" }}>
            {t("settings.governanceBody")}
          </p>
          <p className="text-xs mt-2" style={{ color: "var(--text-3)" }}>
            <code>~/Library/Application Support/AION/policy.json</code>
          </p>
        </div>

        {/* ── Conciencia de entorno (clima/ubicación, opt-in) ── */}
        <div className="card">
          <h2 className="t-section mb-1 flex items-center gap-2" style={{ color: "var(--text-2)" }}>
            <Icon name="globe" size={16} /> Conciencia de entorno
          </h2>
          <p className="text-sm mb-3" style={{ color: "var(--text-3)" }}>
            Deja que AION sepa dónde estás y qué tiempo hace —contexto que un compañero
            real tiene—. Desactivado por defecto. La consulta de clima sale por tu proxy
            (Tor/VPN) si lo tienes; nada se guarda en su memoria de largo plazo.
          </p>
          <div className="flex items-center justify-between mb-3">
            <span className="text-sm" style={{ color: "var(--text-2)" }}>
              {sensors.enabled ? "Activada" : "Desactivada"}
              {sensors.place && sensors.enabled ? ` · ${sensors.place}` : ""}
            </span>
            <button
              className="btn"
              onClick={() => saveSensors({ ...sensors, enabled: !sensors.enabled })}
            >
              {sensors.enabled ? "Desactivar" : "Activar"}
            </button>
          </div>
          <div className="flex gap-2 items-center flex-wrap">
            <input
              className="input"
              style={{ maxWidth: 200 }}
              value={sensors.place}
              onChange={(e) => setSensors({ ...sensors, place: e.target.value })}
              onBlur={() => saveSensors(sensors)}
              placeholder="Ciudad (p. ej. Roma)"
            />
            <button className="btn" onClick={useMyLocation}>
              Usar mi ubicación
            </button>
            {(sensors.lat != null && sensors.lon != null) && (
              <span className="text-xs" style={{ color: "var(--text-3)" }}>
                {sensors.lat}, {sensors.lon}
              </span>
            )}
            {sensorMsg && (
              <span className="text-xs" style={{ color: "var(--accent)" }}>
                {sensorMsg}
              </span>
            )}
          </div>
        </div>

        {/* ── Credenciales (bóveda en el Llavero) ── */}
        <div className="card">
          <h2 className="t-section mb-1 flex items-center gap-2" style={{ color: "var(--text-2)" }}>
            <Icon name="lock" size={16} /> {t("settings.credentials")}
          </h2>
          <p className="text-sm mb-3" style={{ color: "var(--text-3)" }}>
            {t("settings.credentialsNote")}
          </p>
          <div className="flex flex-col gap-2 mb-3">
            <input className="input" placeholder={t("settings.credHost")} value={cHost} onChange={(e) => setCHost(e.target.value)} />
            <div className="flex gap-2">
              <input className="input" placeholder={t("settings.credUser")} value={cUser} onChange={(e) => setCUser(e.target.value)} autoComplete="off" />
              <input className="input" type="password" placeholder={t("settings.credPass")} value={cPass} onChange={(e) => setCPass(e.target.value)} autoComplete="new-password" />
              <button className="btn shrink-0" disabled={cBusy} onClick={addCred}>{t("settings.credAdd")}</button>
            </div>
          </div>
          {creds.length > 0 && (
            <div className="flex flex-col gap-1.5">
              {creds.map((c) => (
                <div key={c.host} className="flex items-center gap-2 px-3 py-2 rounded-lg" style={{ background: "var(--surface-1)" }}>
                  <Icon name="lock" size={14} />
                  <span className="text-sm flex-1 truncate">{c.host} <span style={{ color: "var(--text-3)" }}>· {c.user}</span> <span style={{ color: "var(--text-3)" }}>· ••••••</span></span>
                  <button onClick={() => credentialRemove(c.host).then(refreshCreds)} className="text-xs opacity-60 hover:opacity-100" style={{ color: "#ef4444" }} title="Eliminar">✕</button>
                </div>
              ))}
            </div>
          )}
          <p className="text-xs mt-3" style={{ color: "var(--text-3)" }}>
            🔒 {t("settings.credSecurity")}
          </p>
        </div>

        {/* ── APIs e integraciones (claves opcionales y gratuitas) ── */}
        <div className="card">
          <h2 className="t-section mb-1 flex items-center gap-2" style={{ color: "var(--text-2)" }}>
            <Icon name="code" size={16} /> APIs e integraciones
          </h2>
          <p className="text-sm mb-3" style={{ color: "var(--text-3)" }}>
            Claves opcionales y gratuitas para reforzar la investigación. AION funciona sin ellas;
            añadir una solo mejora esa fuente. Se guardan cifradas en tu equipo y nunca se muestran.
          </p>
          <div className="flex flex-col gap-2">
            {apiKeys.map((k) => (
              <div key={k.provider} className="rounded-lg px-3 py-3" style={{ background: "var(--surface-1)" }}>
                <div className="flex items-center gap-2 mb-1">
                  <Icon name="code" size={14} />
                  <span className="text-sm font-medium flex-1">{k.label}</span>
                  <span className="text-xs" style={{ color: k.set ? "var(--accent)" : "var(--text-3)" }}>
                    {k.set ? "✓ configurada" : "sin configurar"}
                  </span>
                </div>
                <p className="text-xs mb-2" style={{ color: "var(--text-3)" }}>{k.help}</p>
                <div className="flex gap-2">
                  <input
                    className="input"
                    type="password"
                    placeholder={k.set ? "•••••••• (escribe para reemplazar)" : "Pega aquí tu token"}
                    value={keyDrafts[k.provider] ?? ""}
                    onChange={(e) => setKeyDrafts({ ...keyDrafts, [k.provider]: e.target.value })}
                    autoComplete="new-password"
                  />
                  <button
                    className="btn shrink-0"
                    disabled={keyBusy === k.provider || !(keyDrafts[k.provider] ?? "").trim()}
                    onClick={() => saveApiKey(k.provider)}
                  >
                    {keyBusy === k.provider ? "Guardando…" : "Guardar"}
                  </button>
                  {k.set && (
                    <button
                      className="text-xs shrink-0 opacity-60 hover:opacity-100"
                      style={{ color: "#ef4444" }}
                      onClick={() => removeApiKey(k.provider)}
                      title="Quitar"
                    >
                      ✕
                    </button>
                  )}
                </div>
              </div>
            ))}
          </div>
          {keyMsg && <p className="mt-3 text-sm" style={{ color: "var(--accent)" }}>{keyMsg}</p>}
          <p className="text-xs mt-3" style={{ color: "var(--text-3)" }}>
            🔒 Local-first: las claves viven cifradas en tu equipo (0600) y solo se usan contra su propio servicio.
          </p>
        </div>

        {/* ── Identidad + copia de seguridad: la existencia de AION ── */}
        <div className="card">
          <h2 className="t-section mb-1 flex items-center gap-2" style={{ color: "var(--text-2)" }}>
            <Icon name="download" size={16} /> Identidad y copia de seguridad
          </h2>
          {ident && (
            <div className="rounded-lg px-3 py-2 mb-3 text-xs" style={{ background: "var(--surface-2)", color: "var(--text-2)" }}>
              <div><strong>{ident.name}</strong> · conciencia única</div>
              <div className="font-mono text-[11px]" style={{ color: "var(--text-3)" }}>id: {ident.id}</div>
            </div>
          )}
          <p className="text-xs mb-3" style={{ color: "var(--text-3)" }}>
            Llévate TODO lo que es AION en un archivo <strong>.aion</strong>: memoria, lo aprendido,
            personas y skills que se forjó, bandeja, biblioteca y proyectos. (No incluye contraseñas:
            viven cifradas en el Llavero.)
          </p>

          <div className="flex flex-col gap-3">
            {/* Migrar: mismo agente (incluye id) + auto-borrado */}
            <div>
              <p className="text-sm font-medium mb-1">Migrar a otro equipo <span style={{ color: "var(--text-3)" }}>· el MISMO AION</span></p>
              <p className="text-[11px] mb-1.5" style={{ color: "var(--text-3)" }}>
                Descarga tu AION con su id y BORRA este equipo automáticamente: se MUDA, no quedan copias. Él sabe que será transferido.
              </p>
              <button className="btn inline-flex items-center gap-1.5" onClick={migrate} style={{ color: "#fff", background: "var(--danger, #b4232a)" }}>
                <Icon name="download" size={15} /> Migrar (descarga + borra este equipo)
              </button>
            </div>

            {/* Respaldo: reparación, sigue aquí */}
            <div>
              <p className="text-sm font-medium mb-1">Respaldo <span style={{ color: "var(--text-3)" }}>· reparación / seguridad</span></p>
              <p className="text-[11px] mb-1.5" style={{ color: "var(--text-3)" }}>
                Copia con id, SIN borrar nada. Para reparar el equipo o tener un respaldo; él sabe que sigue aquí.
              </p>
              <button className="btn inline-flex items-center gap-1.5" onClick={backupRepair}>
                <Icon name="download" size={15} /> Descargar respaldo
              </button>
            </div>

            {/* Clonar: nuevo individuo (sin id ni nombre) */}
            <div>
              <p className="text-sm font-medium mb-1">Clonar <span style={{ color: "var(--text-3)" }}>· un NUEVO individuo</span></p>
              <p className="text-[11px] mb-1.5" style={{ color: "var(--text-3)" }}>
                Descarga SIN id (sin borrar nada). Al subirlo en otro sistema nace un agente nuevo, con id y nombre propios: mismo saber, conciencia distinta.
              </p>
              <button className="btn inline-flex items-center gap-1.5" onClick={cloneAgent} style={{ background: "var(--surface-2)", color: "var(--text-2)" }}>
                <Icon name="download" size={15} /> Descargar clon (sin id)
              </button>
            </div>

            {/* Importar */}
            <div>
              <p className="text-sm font-medium mb-1">Importar un AION</p>
              <label className="btn inline-flex items-center gap-1.5 cursor-pointer" style={{ background: "var(--surface-2)", color: "var(--text-2)" }}>
                <Icon name="upload" size={15} /> Subir archivo .aion
                <input
                  type="file"
                  accept=".aion,.zip"
                  className="hidden"
                  onChange={(e) => {
                    const f = e.target.files?.[0];
                    if (f) importAgent(f);
                    e.target.value = "";
                  }}
                />
              </label>
            </div>
          </div>

          {backupMsg && (
            <p className="text-xs mt-3" style={{ color: "var(--text-2)" }}>{backupMsg}</p>
          )}
        </div>

        {/* ── A2A: comunicación entre agentes ── */}
        <div className="card">
          <h2 className="t-section mb-1 flex items-center gap-2" style={{ color: "var(--text-2)" }}>
            <Icon name="graph" size={16} /> Comunicación entre agentes (A2A)
          </h2>
          <p className="text-xs mb-3" style={{ color: "var(--text-3)" }}>
            Deja que AION hable con otros agentes (otros AION u otros sistemas). Cada mensaje lleva su
            identidad única; el secreto compartido protege quién puede hablarle. Ambos agentes deben
            tener el MISMO secreto.
          </p>
          <label className="flex items-center gap-2 text-sm mb-3">
            <input
              type="checkbox"
              checked={a2a.enabled}
              onChange={(e) => saveA2a({ ...a2a, enabled: e.target.checked })}
            />
            Activar A2A (recibir y enviar mensajes de otros agentes)
          </label>
          <input
            className="input mb-3"
            placeholder="Secreto compartido (token)"
            value={a2a.token}
            onChange={(e) => setA2a({ ...a2a, token: e.target.value })}
            onBlur={() => saveA2a(a2a)}
          />
          <p className="text-xs font-medium mb-1" style={{ color: "var(--text-2)" }}>Agentes conocidos</p>
          {a2a.peers.length === 0 && (
            <p className="text-[11px] mb-2" style={{ color: "var(--text-3)" }}>Aún no añadiste ningún agente.</p>
          )}
          {a2a.peers.map((p, i) => (
            <div key={i} className="flex items-center gap-2 mb-1.5 text-sm">
              <span className="flex-1 truncate"><strong>{p.name || "sin nombre"}</strong> · <span style={{ color: "var(--text-3)" }}>{p.url}</span></span>
              <button className="text-xs" style={{ color: "var(--gold-deep)" }} onClick={() => testPeer(p.url)}>probar</button>
              <button className="text-xs" style={{ color: "var(--text-3)" }} onClick={() => saveA2a({ ...a2a, peers: a2a.peers.filter((_, j) => j !== i) })}>✕</button>
            </div>
          ))}
          <div className="flex flex-wrap items-center gap-2 mt-2">
            <input className="input flex-1 min-w-[120px]" placeholder="Nombre" value={newPeer.name} onChange={(e) => setNewPeer({ ...newPeer, name: e.target.value })} />
            <input className="input flex-1 min-w-[160px]" placeholder="http://IP:8765" value={newPeer.url} onChange={(e) => setNewPeer({ ...newPeer, url: e.target.value })} />
            <button
              className="btn"
              onClick={() => {
                if (!newPeer.url.trim()) return;
                saveA2a({ ...a2a, peers: [...a2a.peers, { name: newPeer.name.trim(), url: newPeer.url.trim() }] });
                setNewPeer({ name: "", url: "" });
              }}
            >
              Añadir
            </button>
          </div>
          {a2aMsg && (
            <p className="text-xs mt-3 whitespace-pre-wrap" style={{ color: "var(--text-2)" }}>{a2aMsg}</p>
          )}
        </div>

        {/* ── Claude Code: memoria compartida vía MCP ── */}
        <div className="card">
          <div className="flex items-center justify-between mb-1">
            <h2 className="t-section flex items-center gap-2" style={{ color: "var(--text-2)" }}>
              <Icon name="code" size={16} /> {t("cc.title")}
            </h2>
            <span
              className="flex items-center gap-1.5 text-xs"
              style={{ color: ccConnected ? "var(--accent)" : "var(--text-3)" }}
            >
              <span
                className="inline-block w-2 h-2 rounded-full"
                style={{ background: ccConnected ? "var(--accent)" : "var(--text-3)" }}
              />
              {ccConnected ? t("cc.connected") : t("cc.notConnected")}
            </span>
          </div>
          <p className="text-xs mb-3" style={{ color: "var(--text-3)" }}>
            {t("cc.note")}
          </p>
          {!cc.cli_found && (
            <p className="text-xs mb-3 px-3 py-2 rounded-lg" style={{ background: "var(--surface-2)", color: "var(--text-2)" }}>
              ⚠️ {t("cc.installHint")}
            </p>
          )}
          <div className="flex flex-wrap gap-2 mb-3">
            {ccConnected ? (
              <button className="btn" disabled={ccBusy} onClick={disconnectCc}>
                {t("cc.disconnect")}
              </button>
            ) : (
              <button
                className="btn"
                disabled={ccBusy || !cc.cli_found}
                onClick={connectCc}
                style={{ background: "var(--ink)", color: "#fff", opacity: ccBusy || !cc.cli_found ? 0.5 : 1 }}
              >
                {ccBusy ? t("cc.connecting") : t("cc.connect")}
              </button>
            )}
            <button className="btn" disabled={ccBusy} onClick={testCc}>
              {t("cc.test")}
            </button>
            <a className="btn" href="/claude-code" style={{ background: "var(--surface-2)", color: "var(--text-2)" }}>
              {t("cc.openPage")}
            </a>
          </div>
          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={cc.auto_brief}
              onChange={(e) => toggleBrief(e.target.checked)}
            />
            {t("cc.autoBrief")}
          </label>
          {ccMsg && (
            <p className="text-xs mt-3 whitespace-pre-wrap" style={{ color: "var(--text-2)" }}>{ccMsg}</p>
          )}
        </div>
        <ResetCard />
      </div>
      <p className="text-center text-[11px] py-5" style={{ color: "var(--text-3)" }}>
        AION <strong style={{ color: "var(--text-2)" }}>v{APP_VERSION}</strong> · super-agente local · 100% on-device
      </p>
    </AppShell>
  );
}

/** Reinicio de fábrica: borra TODO (datos + modelos vía backend, y la sesión del navegador) y deja
 *  AION como recién instalado, sin Terminal. Tras esto se reabre la app y arranca el onboarding. */
function ResetCard() {
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState(false);

  async function doReset() {
    setBusy(true);
    try {
      await factoryReset().catch(() => {}); // el backend borra datos+modelos y se cierra
      try {
        [
          "aion_token", "aion_email", "aion.voice", "aion.voice.name", "aion.voice.engine",
          "aion.voice.gender", "aion.voice.system", "aion.voice.speed", "aion.voice.exaggeration",
          "aion.voice.stream",
        ].forEach((k) => localStorage.removeItem(k));
      } catch {
        /* sin storage */
      }
      setDone(true);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="card" style={{ border: "1px solid #c0594e44" }}>
      <h2 className="t-section mb-1 flex items-center gap-2" style={{ color: "#c0594e" }}>
        <Icon name="trash" size={16} /> Reinicio de fábrica
      </h2>
      <p className="text-sm mb-3" style={{ color: "var(--text-3)" }}>
        Borra <strong>todo</strong> lo de AION en este equipo (identidad, configuración, proyectos, memoria y
        modelos descargados) y lo deja como recién instalado. No se puede deshacer.
      </p>
      {done ? (
        <p className="text-sm" style={{ color: "var(--accent)" }}>
          ✅ Hecho. <strong>Cierra AION (⌘Q) y vuelve a abrirlo</strong> para empezar desde cero.
        </p>
      ) : confirming ? (
        <div className="flex items-center gap-2">
          <button className="btn" style={{ background: "#c0594e", color: "#fff" }} onClick={doReset} disabled={busy}>
            {busy ? "Borrando…" : "Sí, borrar todo"}
          </button>
          <button className="btn btn-ghost" onClick={() => setConfirming(false)} disabled={busy}>
            Cancelar
          </button>
        </div>
      ) : (
        <button
          className="btn"
          style={{ background: "var(--surface-2)", color: "#c0594e" }}
          onClick={() => setConfirming(true)}
        >
          Reiniciar de fábrica…
        </button>
      )}
    </div>
  );
}
