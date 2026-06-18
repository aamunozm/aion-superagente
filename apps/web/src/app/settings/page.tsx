"use client";

import { useEffect, useState } from "react";
import AppShell from "@/components/AppShell";
import Icon from "@/components/Icon";
import { LANGS, useT } from "@/lib/i18n";
import {
  credentialsList,
  credentialRemove,
  credentialSet,
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

  function applyProv(p: ProviderState) {
    setProv(p);
    if (p.kind !== "external") {
      setProvSel("local");
    } else if (p.base_url.includes("googleapis")) {
      setProvSel("google");
    } else if (p.base_url.includes("deepseek")) {
      setProvSel("deepseek");
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
                    if (k !== "local") setProvModel(prov?.kind === "external" && prov.base_url === PROVIDERS[k].base_url ? prov.model : PROVIDERS[k].defaultModel);
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
              <input
                className="input"
                placeholder="Modelo (p. ej. gemini-2.5-flash, deepseek-chat)"
                value={provModel}
                onChange={(e) => setProvModel(e.target.value)}
                autoComplete="off"
              />
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
      </div>
    </AppShell>
  );
}
