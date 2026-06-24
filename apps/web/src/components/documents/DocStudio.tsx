"use client";

import { useCallback, useEffect, useState, type ReactNode } from "react";
import {
  docStylesList,
  docStyleSave,
  docStyleRemove,
  docStyleExtract,
  documentsOfferta,
  offertaPreviewHtml,
  documentsGenerate,
  type DocStyleT,
  type StyleEntry,
} from "@/lib/api";

const FONTS: { label: string; stack: string }[] = [
  { label: "Sans", stack: '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif' },
  { label: "Serif", stack: 'Georgia, Cambria, "Times New Roman", Times, serif' },
  { label: "Mono", stack: 'ui-monospace, "SF Mono", Menlo, Consolas, monospace' },
];

/** Mini-previsualización de un estilo: una "hoja" con su papel, tinta y acento. */
function Swatch({ s, h = 80 }: { s: DocStyleT; h?: number }) {
  return (
    <div
      className="w-full rounded-lg overflow-hidden flex flex-col"
      style={{ height: h, background: s.paper, border: `1px solid ${s.hair}`, fontFamily: s.font_display }}
    >
      <div style={{ background: s.ink, height: h * 0.28, display: "flex", alignItems: "center", padding: "0 8px" }}>
        <span style={{ color: s.accent, fontSize: 7, fontWeight: 800, letterSpacing: "0.1em", textTransform: "uppercase" }}>Aa</span>
      </div>
      <div className="flex-1 p-2 flex flex-col gap-1 justify-center">
        <div style={{ height: 5, width: "60%", background: s.ink, borderRadius: s.radius / 2 }} />
        <div style={{ height: 3, width: "85%", background: s.muted, opacity: 0.5, borderRadius: 2 }} />
        <div style={{ display: "flex", gap: 4, marginTop: 2 }}>
          <span style={{ width: 14, height: 8, background: s.accent, borderRadius: s.radius / 2 }} />
          <span style={{ width: 24, height: 8, background: s.soft, border: `1px solid ${s.hair}`, borderRadius: s.radius / 2 }} />
        </div>
      </div>
    </div>
  );
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="text-xs flex flex-col gap-1" style={{ color: "var(--text-2)" }}>
      <span>{label}</span>
      {children}
    </label>
  );
}

/**
 * Estudio de documentos y estilos (galería tipo Canva): elegir/extraer/ajustar/duplicar/guardar/
 * eliminar un estilo, y generar ofertas o documentos simples con esa apariencia. Reutilizable:
 * vive como pantalla, dentro de un Proyecto (modal) y como acción rápida del Chat (modal).
 */
export default function DocStudio() {
  const [entries, setEntries] = useState<StyleEntry[]>([]);
  const [selected, setSelected] = useState<DocStyleT | null>(null);
  const [busy, setBusy] = useState("");
  const [note, setNote] = useState("");

  // Extracción
  const [palette, setPalette] = useState<string[]>([]);
  const [fonts, setFonts] = useState<string[]>([]);
  const [saveName, setSaveName] = useState("");

  // Oferta — datos
  const [client, setClient] = useState("");
  const [subtitle, setSubtitle] = useState("Crescita digitale per la tua azienda");
  const [heroKicker, setHeroKicker] = useState("La tua presenza online che lavora per te");
  const [heroTitle, setHeroTitle] = useState("Più clienti, in automatico.");
  const [heroPitch, setHeroPitch] = useState(
    "Sito **trovato su Google**, app che **risponde subito** e dashboard dei risultati. Tutto gestito, a canone fisso.",
  );
  const [cardsText, setCardsText] = useState(
    "Sito & SEO | Gestione e posizionamento ogni mese\nApp con AI | Risponde ai contatti in modo **naturale**\nDashboard | Risultati trasparenti, nero su bianco",
  );
  const [servicesText, setServicesText] = useState(
    "Primo mese — tutto incluso | € 300,00 | \nDal secondo mese | € 200,00 | / mese",
  );
  const [recurring, setRecurring] = useState("€ 200,00 / mese");
  const [benefitsText, setBenefitsText] = useState(
    "Si ripaga da sola. | Basta un cliente in più al mese.\nZero vincoli. | Interrompi quando vuoi, senza penali.",
  );
  const [compareText, setCompareText] = useState(
    "Persona dedicata | € 1.800+/mese | 95\nAgenzia tradizionale | € 800–1.500/mese | 60\nLa nostra offerta | € 200/mese | 14",
  );
  const [validity, setValidity] = useState(30);
  const [deductible, setDeductible] = useState(true);

  // Documento simple (markdown → PDF/Word con el estilo elegido)
  const [simpleTitle, setSimpleTitle] = useState("Informe");
  const [simpleMd, setSimpleMd] = useState("## Introducción\n\nEscribe aquí tu **documento** en Markdown. Usa tablas, listas y negritas.\n\n## Conclusión\n\n…");

  const load = useCallback(() => {
    docStylesList()
      .then((e) => {
        setEntries(e);
        setSelected((cur) => cur ?? e[0]?.style ?? null);
      })
      .catch(() => {});
  }, []);
  useEffect(() => {
    load();
  }, [load]);

  function patchStyle(patch: Partial<DocStyleT>) {
    setSelected((s) => (s ? { ...s, ...patch } : s));
  }

  async function onExtract(file: File) {
    setBusy("extract");
    setNote("");
    const b64 = await new Promise<string>((res, rej) => {
      const r = new FileReader();
      r.onload = () => res(String(r.result).split(",")[1] ?? "");
      r.onerror = () => rej(new Error("no pude leer"));
      r.readAsDataURL(file);
    });
    const kind = file.name.toLowerCase().endsWith(".pdf") ? "pdf" : "image";
    const r = await docStyleExtract(b64, kind, `Estilo de ${file.name}`).catch(() => null);
    setBusy("");
    if (r?.ok && r.style) {
      setSelected(r.style);
      setPalette(r.palette ?? []);
      setFonts(r.fonts ?? []);
      setSaveName(r.style.name);
      setNote("✓ Estilo extraído. Ajústalo abajo si quieres y guárdalo.");
    } else {
      setNote(`⚠️ ${r?.error ?? "no pude extraer el estilo"}`);
    }
  }

  /** Nombre de copia único frente a la galería actual («Copia de X», «… (2)»…). */
  function uniqueCopyName(base: string): string {
    const taken = new Set(entries.map((e) => e.name.toLowerCase()));
    let n = `Copia de ${base}`;
    if (!taken.has(n.toLowerCase())) return n;
    let i = 2;
    while (taken.has((n = `Copia de ${base} (${i})`).toLowerCase())) i++;
    return n;
  }

  async function saveStyle(nameOverride?: string) {
    if (!selected) return;
    setBusy("save");
    const style = { ...selected, name: (nameOverride ?? saveName).trim() || selected.name };
    const r = await docStyleSave(style).catch(() => null);
    setBusy("");
    if (r?.ok) {
      setNote(`✓ Estilo «${style.name}» guardado.`);
      setPalette([]);
      setFonts([]);
      setSelected(style);
      setSaveName(style.name);
      load();
    } else {
      setNote(`⚠️ ${r?.error ?? "no pude guardar"}`);
    }
  }

  /** Duplica el estilo actual como una copia EDITABLE guardada (Canva-style). */
  async function duplicateStyle() {
    if (!selected) return;
    await saveStyle(uniqueCopyName(selected.name));
  }

  async function removeStyle(name: string) {
    setBusy(name);
    await docStyleRemove(name).catch(() => {});
    setBusy("");
    load();
  }

  function buildFacts() {
    const split = (t: string) =>
      t.split("\n").map((l) => l.split("|").map((x) => x.trim())).filter((p) => p[0]);
    const services = split(servicesText).map((p) => ({ title: p[0], desc: "", price: p[1] ?? "", price_note: p[2] ?? "" }));
    const highlights = split(cardsText).map((p) => ({ title: p[0], body: p[1] ?? "" }));
    const benefits = split(benefitsText).map((p) => ({ lead: p[0], body: p[1] ?? "" }));
    const cmp = split(compareText);
    const comparison = cmp.map((p, i) => ({
      label: p[0],
      value: p[1] ?? "",
      pct: Math.max(0, Math.min(100, parseInt(p[2] ?? "50", 10) || 50)),
      tone: i === cmp.length - 1 ? "green" : i === 0 ? "red" : "gold",
    }));
    return {
      kicker: "OFFERTA SERVIZI 2026",
      subtitle,
      client,
      hero_kicker: heroKicker,
      hero_title: heroTitle,
      hero_pitch: heroPitch,
      highlights,
      services,
      recurring_label: "Dal 2° mese (IVA esclusa)",
      recurring_value: recurring,
      benefits,
      comparison,
      validity_days: validity,
      deductible,
    };
  }

  async function generate(format: "pdf" | "html" | "docx") {
    setBusy("gen");
    setNote("");
    try {
      await documentsOfferta(buildFacts(), selected, format);
      setNote(`✓ Oferta generada (${format.toUpperCase()}) con «${selected?.name ?? "estilo por defecto"}».`);
    } catch (e) {
      setNote(`⚠️ ${(e as Error).message}`);
    }
    setBusy("");
  }

  async function preview() {
    setBusy("preview");
    setNote("");
    try {
      const html = await offertaPreviewHtml(buildFacts(), selected);
      const w = window.open("", "_blank");
      if (w) {
        w.document.open();
        w.document.write(html);
        w.document.close();
      }
    } catch (e) {
      setNote(`⚠️ ${(e as Error).message}`);
    }
    setBusy("");
  }

  async function generateSimple(format: "pdf" | "docx") {
    setBusy("simple");
    setNote("");
    try {
      await documentsGenerate({ title: simpleTitle, markdown: simpleMd, format, template: "base", style: selected });
      setNote(`✓ Documento «${simpleTitle}» generado (${format.toUpperCase()}) con «${selected?.name ?? "estilo por defecto"}».`);
    } catch (e) {
      setNote(`⚠️ ${(e as Error).message}`);
    }
    setBusy("");
  }

  return (
    <div className="flex flex-col gap-6">
      {note && (
        <p className="text-xs px-3 py-2 rounded-lg" style={{ background: "var(--surface-1)", color: "var(--text-2)" }}>{note}</p>
      )}

      {/* ── Galería de estilos ── */}
      <section className="card">
        <div className="flex items-center justify-between mb-3">
          <h2 className="t-section" style={{ color: "var(--text-2)" }}>
            Estilos {selected && <span style={{ color: "var(--text-3)" }}>· {selected.name}</span>}
          </h2>
          <label className="btn btn-ghost text-xs cursor-pointer">
            {busy === "extract" ? "Extrayendo…" : "⬆︎ Extraer de un documento"}
            <input type="file" accept=".pdf,image/*" className="hidden"
              onChange={(e) => { const f = e.target.files?.[0]; if (f) onExtract(f); e.currentTarget.value = ""; }} />
          </label>
        </div>
        <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-5 gap-3">
          {entries.map((e) => (
            <button key={e.name + (e.builtin ? "-b" : "")} onClick={() => { setSelected(e.style); setSaveName(e.style.name); }}
              className="text-left rounded-xl p-2 transition-all"
              style={{ background: "var(--surface)", border: `2px solid ${selected?.name === e.name ? "var(--accent)" : "var(--border)"}` }}>
              <Swatch s={e.style} h={64} />
              <div className="flex items-center justify-between mt-2">
                <span className="text-xs font-medium truncate" style={{ color: "var(--text-1)" }}>{e.name}</span>
                {!e.builtin && (
                  <span role="button" onClick={(ev) => { ev.stopPropagation(); removeStyle(e.name); }}
                    className="text-xs opacity-50 hover:opacity-100" style={{ color: "var(--danger)" }} title="Eliminar estilo guardado">✕</span>
                )}
              </div>
              <span className="text-[10px]" style={{ color: "var(--text-3)" }}>{e.builtin ? "preset" : "guardado"}</span>
            </button>
          ))}
        </div>
      </section>

      {/* ── Editor de estilo ── */}
      {selected && (
        <section className="card">
          <h2 className="t-section mb-3" style={{ color: "var(--text-2)" }}>Ajustar el estilo</h2>
          <div className="flex gap-4 items-start flex-wrap">
            <div className="w-40 shrink-0"><Swatch s={selected} h={96} /></div>
            <div className="flex-1 min-w-[260px] grid grid-cols-2 gap-3">
              <Field label="Tinta (oscuro)">
                <input type="color" value={selected.ink} onChange={(e) => patchStyle({ ink: e.target.value, text: e.target.value })} className="w-full h-8 rounded" />
              </Field>
              <Field label="Acento">
                <input type="color" value={selected.accent} onChange={(e) => patchStyle({ accent: e.target.value })} className="w-full h-8 rounded" />
              </Field>
              <Field label="Papel">
                <input type="color" value={selected.paper} onChange={(e) => patchStyle({ paper: e.target.value })} className="w-full h-8 rounded" />
              </Field>
              <Field label={`Esquinas: ${selected.radius}px`}>
                <input type="range" min={0} max={16} value={selected.radius} onChange={(e) => patchStyle({ radius: Number(e.target.value) })} />
              </Field>
              <Field label="Tipografía de títulos">
                <select className="input text-sm" value={selected.font_display}
                  onChange={(e) => patchStyle({ font_display: e.target.value, font: e.target.value })}>
                  {FONTS.map((f) => <option key={f.label} value={f.stack}>{f.label}</option>)}
                </select>
              </Field>
              <label className="text-xs flex items-end gap-2" style={{ color: "var(--text-2)" }}>
                <input type="checkbox" checked={selected.caps_headings} onChange={(e) => patchStyle({ caps_headings: e.target.checked })} />
                Títulos en MAYÚSCULAS
              </label>
            </div>
          </div>
          {palette.length > 0 && (
            <div className="flex gap-1.5 flex-wrap mt-3">
              {palette.map((c) => <span key={c} title={c} style={{ width: 18, height: 18, background: c, borderRadius: 4, border: "1px solid var(--border)" }} />)}
              <span className="text-[10px] ml-2 self-center" style={{ color: "var(--text-3)" }}>fuentes: {fonts.length ? fonts.join(", ") : "—"}</span>
            </div>
          )}
          <div className="flex gap-2 mt-3 items-center">
            <input className="input flex-1 text-sm" value={saveName} onChange={(e) => setSaveName(e.target.value)} placeholder="Nombre para guardar este estilo" />
            <button className="btn btn-ghost text-xs shrink-0" disabled={busy === "save"} onClick={duplicateStyle} title="Crear una copia editable de este estilo">⧉ Duplicar</button>
            <button className="btn btn-gold text-xs shrink-0" disabled={busy === "save"} onClick={() => saveStyle()}>Guardar estilo</button>
          </div>
        </section>
      )}

      {/* ── Generar oferta ── */}
      <section className="card">
        <h2 className="t-section mb-3" style={{ color: "var(--text-2)" }}>Generar oferta</h2>
        <div className="grid sm:grid-cols-2 gap-3">
          <Field label="Cliente"><input className="input" value={client} onChange={(e) => setClient(e.target.value)} placeholder="Avv. Lisa Armenio" /></Field>
          <Field label="Subtítulo"><input className="input" value={subtitle} onChange={(e) => setSubtitle(e.target.value)} /></Field>
          <Field label="Kicker (hero)"><input className="input" value={heroKicker} onChange={(e) => setHeroKicker(e.target.value)} /></Field>
          <Field label="Titular (hero)"><input className="input" value={heroTitle} onChange={(e) => setHeroTitle(e.target.value)} /></Field>
          <div className="sm:col-span-2"><Field label="Frase de valor (admite **negrita**)"><input className="input" value={heroPitch} onChange={(e) => setHeroPitch(e.target.value)} /></Field></div>
          <div className="sm:col-span-2"><Field label="Tarjetas «qué incluimos» — Título | descripción"><textarea className="input font-mono text-xs" rows={3} value={cardsText} onChange={(e) => setCardsText(e.target.value)} /></Field></div>
          <div className="sm:col-span-2"><Field label="Servicios — Título | precio | nota"><textarea className="input font-mono text-xs" rows={2} value={servicesText} onChange={(e) => setServicesText(e.target.value)} /></Field></div>
          <Field label="Canone recurrente"><input className="input" value={recurring} onChange={(e) => setRecurring(e.target.value)} /></Field>
          <Field label="Validez (días)"><input className="input" type="number" value={validity} onChange={(e) => setValidity(Number(e.target.value))} /></Field>
          <div className="sm:col-span-2"><Field label="Propuestas de valor — Lead | texto"><textarea className="input font-mono text-xs" rows={2} value={benefitsText} onChange={(e) => setBenefitsText(e.target.value)} /></Field></div>
          <div className="sm:col-span-2"><Field label="Comparativa — Etiqueta | valor | % (0–100, el último = verde)"><textarea className="input font-mono text-xs" rows={3} value={compareText} onChange={(e) => setCompareText(e.target.value)} /></Field></div>
          <label className="text-xs flex items-center gap-2" style={{ color: "var(--text-2)" }}>
            <input type="checkbox" checked={deductible} onChange={(e) => setDeductible(e.target.checked)} />
            Coste deducible (franja + callout)
          </label>
        </div>
        <div className="flex gap-2 mt-4 items-center flex-wrap">
          <button className="btn btn-gold" disabled={busy === "gen"} onClick={() => generate("pdf")}>{busy === "gen" ? "Generando…" : "⬇︎ Generar PDF"}</button>
          <button className="btn btn-ghost text-xs" disabled={busy === "gen"} onClick={() => generate("docx")}>⬇︎ Word</button>
          <button className="btn btn-ghost text-xs" disabled={busy === "preview"} onClick={preview}>{busy === "preview" ? "…" : "👁 Vista previa"}</button>
          <button className="btn btn-ghost text-xs" disabled={busy === "gen"} onClick={() => generate("html")}>HTML</button>
        </div>
      </section>

      {/* ── Documento simple (markdown + estilo) ── */}
      <section className="card">
        <h2 className="t-section mb-3" style={{ color: "var(--text-2)" }}>
          Documento simple (Markdown → PDF/Word con el estilo elegido)
        </h2>
        <div className="flex flex-col gap-3">
          <Field label="Título">
            <input className="input" value={simpleTitle} onChange={(e) => setSimpleTitle(e.target.value)} />
          </Field>
          <Field label="Contenido (Markdown: ## títulos, **negrita**, tablas, listas)">
            <textarea className="input font-mono text-xs" rows={6} value={simpleMd} onChange={(e) => setSimpleMd(e.target.value)} />
          </Field>
          <div className="flex gap-2">
            <button className="btn btn-gold" disabled={busy === "simple"} onClick={() => generateSimple("pdf")}>
              {busy === "simple" ? "Generando…" : "⬇︎ PDF"}
            </button>
            <button className="btn btn-ghost text-xs" disabled={busy === "simple"} onClick={() => generateSimple("docx")}>⬇︎ Word</button>
          </div>
        </div>
      </section>
    </div>
  );
}

/** Envoltorio modal a pantalla casi completa para abrir el estudio desde Proyectos o el Chat. */
export function DocStudioModal({ open, onClose }: { open: boolean; onClose: () => void }) {
  if (!open) return null;
  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center p-4 sm:p-8 overflow-y-auto"
      style={{ background: "rgba(0,0,0,0.45)" }}
      onClick={onClose}
    >
      <div
        className="w-full max-w-5xl my-2"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-3 mb-4">
          <div className="flex-1">
            <h2 className="font-display text-xl font-bold" style={{ color: "var(--text-1)" }}>Documentos</h2>
            <p className="text-xs" style={{ color: "var(--text-3)" }}>Elige o extrae un estilo, ajústalo, duplícalo o guárdalo, y genera con esa apariencia.</p>
          </div>
          <button
            onClick={onClose}
            className="shrink-0 rounded-full w-8 h-8 flex items-center justify-center"
            style={{ background: "var(--surface-2)", color: "var(--text-2)" }}
            aria-label="Cerrar"
            title="Cerrar"
          >
            ✕
          </button>
        </div>
        <DocStudio />
      </div>
    </div>
  );
}
