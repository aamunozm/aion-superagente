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
  providerSet,
  systemScan,
  status,
  downloadAgent,
  agentImport,
  agentWipe,
  getIdentity,
  type AionIdentity,
  type CredMeta,
  type InstalledModel,
  type ModelOption,
  type SystemScan,
} from "@/lib/api";

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

  useEffect(() => {
    getIdentity().then(setIdent);
  }, []);

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
  const isInstalled = (ollama: string) =>
    installed.some((i) => i.name === ollama || i.name.startsWith(`${ollama.split(":")[0]}:`));
  const isCurrent = (ollama: string) => {
    const base = ollama.split(":")[0];
    return current === ollama || current.startsWith(`${base}:`) || current === base;
  };

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
      <div className="max-w-2xl mx-auto px-6 py-8 flex flex-col gap-6">
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
      </div>
    </AppShell>
  );
}
