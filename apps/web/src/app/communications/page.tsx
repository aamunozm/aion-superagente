"use client";

import { useEffect, useState } from "react";
import {
  AppShell,
  Icon,
  IconChip,
  Badge,
  Button,
  Input,
  Avatar,
  StatusDot,
  type IconName,
} from "@/components";
import { commsGet, commsSet, type CommContact, type CommsPolicy } from "@/lib/api";

// Canales de MENSAJERÍA que se filtran por contacto (calendario/contactos son del
// propio Ariel y solo dependen del interruptor maestro).
const MSG_CHANNELS: { key: string; label: string; icon: IconName }[] = [
  { key: "imessage", label: "Mensajes", icon: "message" },
  { key: "whatsapp", label: "WhatsApp", icon: "whatsapp" },
];

function slug(name: string): string {
  return (
    name
      .toLowerCase()
      .normalize("NFD")
      .replace(/[̀-ͯ]/g, "")
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-|-$/g, "") || "contacto"
  );
}

export default function CommunicationsPage() {
  const [pol, setPol] = useState<CommsPolicy | null>(null);
  const [saving, setSaving] = useState(false);
  const [msg, setMsg] = useState("");
  const [err, setErr] = useState(false);
  const [draft, setDraft] = useState({ name: "", handle: "" });

  useEffect(() => {
    commsGet()
      .then(setPol)
      .catch(() => setErr(true));
  }, []);

  function patch(p: Partial<CommsPolicy>) {
    setPol((prev) => (prev ? { ...prev, ...p } : prev));
    setMsg("");
  }
  function patchContact(id: string, c: Partial<CommContact>) {
    setPol((prev) =>
      prev
        ? { ...prev, contacts: prev.contacts.map((x) => (x.id === id ? { ...x, ...c } : x)) }
        : prev,
    );
    setMsg("");
  }
  function removeContact(id: string) {
    setPol((prev) => (prev ? { ...prev, contacts: prev.contacts.filter((x) => x.id !== id) } : prev));
  }
  function addContact() {
    const name = draft.name.trim();
    if (!name || !pol) return;
    const id = `${slug(name)}-${pol.contacts.length + 1}`;
    const c: CommContact = {
      id,
      name,
      handle: draft.handle.trim(),
      channels: ["imessage"],
      allow_read: true,
      allow_send: false,
      note: "",
    };
    setPol({ ...pol, contacts: [...pol.contacts, c] });
    setDraft({ name: "", handle: "" });
    setMsg("");
  }
  function toggleChannel(c: CommContact, ch: string) {
    const has = c.channels.includes(ch);
    patchContact(c.id, {
      channels: has ? c.channels.filter((x) => x !== ch) : [...c.channels, ch],
    });
  }

  async function save() {
    if (!pol || saving) return;
    setSaving(true);
    setMsg("");
    try {
      const r = await commsSet({
        enabled: pol.enabled,
        default_allow: pol.default_allow,
        contacts: pol.contacts,
      });
      setMsg(r.ok ? "✅ Guardado · estos son los contactos con los que puedo hablar." : `⚠️ ${r.error ?? "error"}`);
    } catch (e) {
      setMsg(`⚠️ ${e instanceof Error ? e.message : "error"}`);
    } finally {
      setSaving(false);
    }
  }

  return (
    <AppShell title="Comunicaciones">
      <div className="max-w-6xl mx-auto px-3 py-6">
        <p className="text-[15px] mb-6 max-w-2xl" style={{ color: "var(--text-2)" }}>
          Decide <strong>con quién</strong> y <strong>por qué canal</strong> puede comunicarse AION
          (Mensajes, WhatsApp), y deja que mire tu <strong>agenda</strong> y tus{" "}
          <strong>contactos</strong>. Por privacidad, todo está desactivado hasta que lo enciendas.
          Enviar siempre te pedirá confirmación.
        </p>

        {err && (
          <div className="card text-sm" style={{ color: "var(--text-2)" }}>
            No pude leer la configuración. ¿Está AION en marcha (puerto 8765)?
          </div>
        )}

        {pol && (
          <div className="flex flex-col gap-4">
            {/* Interruptor maestro */}
            <div className="card flex items-center justify-between">
              <div className="flex items-center gap-3">
                <IconChip icon="message" tint={pol.enabled ? "mint" : "gold"} />
                <div>
                  <h2 className="t-section" style={{ color: "var(--text-2)" }}>
                    Comunicaciones
                  </h2>
                  <p className="text-xs mt-0.5 flex items-center gap-1.5" style={{ color: "var(--text-3)" }}>
                    <StatusDot color={pol.enabled ? "var(--on-mint)" : "var(--text-3)"} />
                    {pol.enabled ? "Activadas" : "Desactivadas"} · calendario y contactos incluidos
                  </p>
                </div>
              </div>
              <Button variant={pol.enabled ? "subtle" : "primary"} onClick={() => patch({ enabled: !pol.enabled })}>
                {pol.enabled ? "Desactivar" : "Activar"}
              </Button>
            </div>

            {/* Modo abierto (default_allow) */}
            <div className="card flex items-center justify-between" style={{ opacity: pol.enabled ? 1 : 0.5 }}>
              <div className="flex-1 pr-4">
                <h3 className="text-sm font-semibold" style={{ color: "var(--text-1)" }}>
                  Solo mi lista de contactos
                </h3>
                <p className="text-xs mt-0.5" style={{ color: "var(--text-3)" }}>
                  Recomendado. Si lo desactivas, AION podrá escribir a cualquier número/correo (modo
                  abierto) — menos privado.
                </p>
              </div>
              <button
                onClick={() => patch({ default_allow: !pol.default_allow })}
                disabled={!pol.enabled}
                className="shrink-0 rounded-full transition-all"
                style={{
                  width: 46,
                  height: 26,
                  background: !pol.default_allow ? "var(--accent)" : "var(--surface-2)",
                  position: "relative",
                }}
                aria-label="Solo lista de contactos"
              >
                <span
                  className="block rounded-full transition-all"
                  style={{
                    width: 20,
                    height: 20,
                    background: "#fff",
                    position: "absolute",
                    top: 3,
                    left: !pol.default_allow ? 23 : 3,
                    boxShadow: "var(--shadow-soft)",
                  }}
                />
              </button>
            </div>

            {/* Lista de contactos */}
            <div className="card">
              <div className="flex items-center gap-2 mb-1">
                <Icon name="users" size={16} />
                <h2 className="t-section" style={{ color: "var(--text-2)" }}>
                  Contactos permitidos
                </h2>
                <span className="text-xs ml-auto" style={{ color: "var(--text-3)" }}>
                  {pol.contacts.length}
                </span>
              </div>
              <p className="text-xs mb-4" style={{ color: "var(--text-3)" }}>
                Para cada persona: en qué canales puede AION hablarle, y si solo puede leer o también
                enviar.
              </p>

              {pol.contacts.length === 0 && (
                <p className="text-sm py-3 text-center" style={{ color: "var(--text-3)" }}>
                  Aún no hay contactos. Añade el primero abajo.
                </p>
              )}

              <div className="flex flex-col gap-3">
                {pol.contacts.map((c) => (
                  <div
                    key={c.id}
                    className="rounded-xl p-3"
                    style={{ background: "var(--surface-1)", border: "1px solid var(--border)" }}
                  >
                    <div className="flex items-center gap-3 mb-3">
                      <Avatar label={c.name} size={36} />
                      <div className="min-w-0 flex-1">
                        <div className="text-sm font-semibold truncate" style={{ color: "var(--text-1)" }}>
                          {c.name}
                        </div>
                        <input
                          value={c.handle}
                          onChange={(e) => patchContact(c.id, { handle: e.target.value })}
                          placeholder="teléfono (+39…) o email"
                          className="text-xs w-full bg-transparent outline-none"
                          style={{ color: "var(--text-3)" }}
                        />
                      </div>
                      <button
                        onClick={() => removeContact(c.id)}
                        className="shrink-0 opacity-50 hover:opacity-100"
                        style={{ color: "#ef4444" }}
                        aria-label="Quitar contacto"
                      >
                        <Icon name="trash" size={16} />
                      </button>
                    </div>

                    <div className="flex flex-wrap items-center gap-2">
                      {/* Canales */}
                      {MSG_CHANNELS.map((ch) => {
                        const on = c.channels.includes(ch.key);
                        return (
                          <button
                            key={ch.key}
                            onClick={() => toggleChannel(c, ch.key)}
                            className="inline-flex items-center gap-1.5 text-xs px-2.5 py-1 rounded-full transition-all"
                            style={{
                              background: on ? "var(--accent-subtle)" : "var(--surface-2)",
                              color: on ? "var(--gold-deep)" : "var(--text-3)",
                              border: `1px solid ${on ? "var(--accent)" : "transparent"}`,
                            }}
                          >
                            <Icon name={ch.icon} size={13} /> {ch.label}
                          </button>
                        );
                      })}

                      <span className="mx-1" style={{ width: 1, height: 18, background: "var(--border-2)" }} />

                      {/* Permisos leer/enviar */}
                      <button
                        onClick={() => patchContact(c.id, { allow_read: !c.allow_read })}
                        className="inline-flex items-center gap-1.5 text-xs px-2.5 py-1 rounded-full"
                        style={{
                          background: c.allow_read ? "var(--pastel-mint)" : "var(--surface-2)",
                          color: c.allow_read ? "var(--on-mint)" : "var(--text-3)",
                        }}
                      >
                        <Icon name="eye" size={13} /> Leer
                      </button>
                      <button
                        onClick={() => patchContact(c.id, { allow_send: !c.allow_send })}
                        className="inline-flex items-center gap-1.5 text-xs px-2.5 py-1 rounded-full"
                        style={{
                          background: c.allow_send ? "var(--pastel-peach)" : "var(--surface-2)",
                          color: c.allow_send ? "var(--on-peach)" : "var(--text-3)",
                        }}
                      >
                        <Icon name="send" size={13} /> Enviar
                      </button>
                    </div>
                  </div>
                ))}
              </div>

              {/* Añadir contacto */}
              <div className="flex flex-col sm:flex-row gap-2 mt-4 pt-4" style={{ borderTop: "1px solid var(--border)" }}>
                <Input
                  placeholder="Nombre (p. ej. Mamá)"
                  value={draft.name}
                  onChange={(e) => setDraft({ ...draft, name: e.target.value })}
                />
                <Input
                  placeholder="Teléfono o email (opcional)"
                  value={draft.handle}
                  onChange={(e) => setDraft({ ...draft, handle: e.target.value })}
                  onKeyDown={(e) => e.key === "Enter" && addContact()}
                />
                <Button variant="gold" onClick={addContact} className="shrink-0">
                  <span className="inline-flex items-center gap-1.5">
                    <Icon name="plus" size={15} /> Añadir
                  </span>
                </Button>
              </div>
            </div>

            {/* Guardar */}
            <div className="flex items-center gap-3">
              <Button onClick={save} disabled={saving}>
                {saving ? "Guardando…" : "Guardar cambios"}
              </Button>
              {msg && (
                <span className="text-sm" style={{ color: "var(--accent)" }}>
                  {msg}
                </span>
              )}
            </div>

            <p className="text-xs mt-1" style={{ color: "var(--text-3)" }}>
              Honestidad: AION puede leer/escribir Mensajes y abrir WhatsApp Web (necesita sesión por
              QR una vez), mirar tu agenda y tus contactos. No puede hacer llamadas de audio ni
              transcribir notas de voz — eso no es automatizable hoy y no se finge.
            </p>
          </div>
        )}
      </div>
    </AppShell>
  );
}
