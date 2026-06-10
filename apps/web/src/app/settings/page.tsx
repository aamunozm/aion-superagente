"use client";

import { useEffect, useState } from "react";
import AppShell from "@/components/AppShell";
import { LANGS, useT } from "@/lib/i18n";

export default function SettingsPage() {
  const { t, lang, setLang } = useT();
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
      </div>
    </AppShell>
  );
}
