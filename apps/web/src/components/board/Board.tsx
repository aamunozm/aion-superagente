"use client";

// **Tablero Kanban del proyecto** (modal a pantalla completa). Columnas por etapa con límite WIP,
// barra de progreso, drag&drop nativo entre columnas, side-sheet de detalle (checklist,
// entregables, comentarios) y «Sembrar plan» con tempística. Habla con /api/project/board/*.

import { useCallback, useEffect, useState } from "react";
import Icon from "@/components/Icon";
import {
  boardGet,
  boardSeed,
  boardCardCreate,
  boardCardUpdate,
  boardCardMove,
  boardCardComment,
  boardCardChecklist,
  boardCardDelete,
  type BoardSnapshot,
  type BoardCard,
  type BoardStatus,
  type BoardChecklistItem,
} from "@/lib/api";

const PRIORITY = ["—", "Baja", "Media", "Alta", "Urgente"];
const PRIORITY_COLOR = ["var(--text-3)", "#6b7280", "#2563eb", "#b45309", "#c0594e"];

export function BoardModal({
  projectId,
  projectName,
  open,
  onClose,
}: {
  projectId: string;
  projectName: string;
  open: boolean;
  onClose: () => void;
}) {
  const [snap, setSnap] = useState<BoardSnapshot | null>(null);
  const [loading, setLoading] = useState(false);
  const [active, setActive] = useState<BoardCard | null>(null); // tarjeta abierta en el side-sheet
  const [dragId, setDragId] = useState<string | null>(null);
  const [overCol, setOverCol] = useState<string | null>(null);
  const [showActivity, setShowActivity] = useState(false);
  const [seedOpen, setSeedOpen] = useState(false);

  const reload = useCallback(async () => {
    if (!projectId) return;
    const s = await boardGet(projectId).catch(() => null);
    if (s?.ok) setSnap(s);
  }, [projectId]);

  useEffect(() => {
    if (open) {
      setLoading(true);
      reload().finally(() => setLoading(false));
    }
  }, [open, reload]);

  // Mantén la tarjeta abierta sincronizada con el snapshot recién cargado.
  useEffect(() => {
    if (active && snap) {
      const fresh = snap.cards.find((c) => c.id === active.id);
      if (fresh) setActive(fresh);
    }
  }, [snap]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!open) return null;

  const statuses = (snap?.statuses ?? []).slice().sort((a, b) => a.pos - b.pos);
  const cardsOf = (sid: string) =>
    (snap?.cards ?? []).filter((c) => c.status_id === sid).sort((a, b) => a.pos - b.pos);
  const wipOf = (sid: string) => snap?.wip.find((w) => w.status_id === sid);

  async function addCard(status: BoardStatus) {
    const title = window.prompt(`Nueva tarjeta en «${status.name}»`);
    if (!title?.trim()) return;
    await boardCardCreate(projectId, title.trim(), status.id);
    await reload();
  }

  async function onDrop(status: BoardStatus) {
    setOverCol(null);
    const id = dragId;
    setDragId(null);
    if (!id) return;
    const card = snap?.cards.find((c) => c.id === id);
    if (!card || card.status_id === status.id) return;
    await boardCardMove(projectId, id, status.id);
    await reload();
  }

  async function seed(template: string, playbook: boolean) {
    setSeedOpen(false);
    setLoading(true);
    await boardSeed(projectId, template, playbook).catch(() => null);
    await reload();
    setLoading(false);
  }

  const progress = snap?.progress ?? { done: 0, total: 0, pct: 0 };

  return (
    <div className="fixed inset-0 z-50 flex flex-col" style={{ background: "var(--surface-0)" }}>
      {/* Cabecera */}
      <header
        className="flex items-center gap-3 px-5 h-14 shrink-0"
        style={{ borderBottom: "1px solid var(--border)", background: "var(--surface-1)" }}
      >
        <span className="icon-chip" style={{ width: 32, height: 32, background: "var(--pastel-gold)", color: "var(--on-gold)" }}>
          <Icon name="target" size={16} />
        </span>
        <div className="min-w-0">
          <h2 className="text-sm font-semibold truncate" style={{ color: "var(--text-1)" }}>
            Tablero · {projectName}
          </h2>
          <div className="flex items-center gap-2 mt-0.5">
            <div className="h-1.5 w-40 rounded-full overflow-hidden" style={{ background: "var(--surface-3)" }}>
              <div className="h-full rounded-full" style={{ width: `${progress.pct}%`, background: "var(--accent)" }} />
            </div>
            <span className="text-[11px]" style={{ color: "var(--text-3)" }}>
              {progress.done}/{progress.total} ({progress.pct}%)
            </span>
          </div>
        </div>

        <div className="ml-auto flex items-center gap-2">
          <div className="relative">
            <button className="btn btn-ghost text-xs" onClick={() => setSeedOpen((v) => !v)}>
              <Icon name="sparkle" size={14} /> Sembrar plan
            </button>
            {seedOpen && (
              <div
                className="absolute right-0 mt-1 w-60 rounded-xl p-1.5 z-10 shadow-lg"
                style={{ background: "var(--surface-1)", border: "1px solid var(--border)" }}
              >
                <button className="seed-item" onClick={() => seed("web-seo", true)}>
                  <strong>Proyecto web + SEO</strong>
                  <span>7 etapas · 8 tareas con tempística y checklist</span>
                </button>
                <button className="seed-item" onClick={() => seed("contenido", true)}>
                  <strong>Producción de contenidos</strong>
                  <span>5 etapas · flujo editorial</span>
                </button>
                <button className="seed-item" onClick={() => seed("generico", false)}>
                  <strong>Tablero genérico</strong>
                  <span>Backlog · Por hacer · En curso · Revisión · Hecho</span>
                </button>
              </div>
            )}
          </div>
          <button
            className="btn btn-ghost text-xs"
            onClick={() => setShowActivity((v) => !v)}
            title="Historial de actividad (quién hizo qué)"
          >
            <Icon name="clock" size={14} /> Actividad
          </button>
          <button className="rounded-md p-1.5 hover:opacity-100 opacity-70" onClick={onClose} title="Cerrar tablero">
            <Icon name="stop" size={18} />
          </button>
        </div>
      </header>

      {/* Cuerpo: columnas + (opcional) panel de actividad */}
      <div className="flex-1 flex min-h-0">
        <div className="flex-1 overflow-x-auto overflow-y-hidden">
          {loading && !snap ? (
            <div className="h-full grid place-items-center text-sm" style={{ color: "var(--text-3)" }}>
              Cargando tablero…
            </div>
          ) : statuses.length === 0 ? (
            <EmptyBoard onSeed={() => setSeedOpen(true)} />
          ) : (
            <div className="h-full flex gap-3 p-4 items-start min-w-max">
              {statuses.map((st) => {
                const wip = wipOf(st.id);
                const over = wip?.over ?? false;
                return (
                  <div
                    key={st.id}
                    className="w-72 shrink-0 flex flex-col max-h-full rounded-2xl"
                    style={{
                      background: "var(--surface-1)",
                      border: `1px solid ${overCol === st.id ? "var(--accent)" : "var(--border)"}`,
                    }}
                    onDragOver={(e) => {
                      e.preventDefault();
                      setOverCol(st.id);
                    }}
                    onDragLeave={() => setOverCol((c) => (c === st.id ? null : c))}
                    onDrop={() => onDrop(st)}
                  >
                    <div className="flex items-center gap-2 px-3 py-2.5 shrink-0">
                      <span className="h-2.5 w-2.5 rounded-full shrink-0" style={{ background: st.color }} />
                      <span className="text-xs font-semibold truncate" style={{ color: "var(--text-1)" }}>
                        {st.name}
                      </span>
                      <span
                        className="ml-auto text-[11px] px-1.5 py-0.5 rounded-md shrink-0"
                        style={{
                          background: over ? "var(--danger-subtle, #fbe9e7)" : "var(--surface-3)",
                          color: over ? "#c0594e" : "var(--text-3)",
                          fontWeight: over ? 700 : 500,
                        }}
                        title={st.wip ? `Límite WIP: ${st.wip}` : "Sin límite WIP"}
                      >
                        {wip?.count ?? 0}
                        {st.wip ? `/${st.wip}` : ""}
                      </span>
                    </div>

                    <div className="flex-1 overflow-y-auto px-2 pb-2 flex flex-col gap-2">
                      {cardsOf(st.id).map((c) => (
                        <CardChip
                          key={c.id}
                          card={c}
                          onOpen={() => setActive(c)}
                          onDragStart={() => setDragId(c.id)}
                          onDragEnd={() => setDragId(null)}
                        />
                      ))}
                      <button
                        className="text-[12px] text-left px-2 py-1.5 rounded-lg opacity-60 hover:opacity-100"
                        style={{ color: "var(--text-2)" }}
                        onClick={() => addCard(st)}
                      >
                        <Icon name="plus" size={12} /> Añadir tarjeta
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>

        {showActivity && (
          <aside className="w-72 shrink-0 overflow-y-auto p-3" style={{ borderLeft: "1px solid var(--border)" }}>
            <p className="text-[11px] font-semibold uppercase tracking-[0.12em] mb-2" style={{ color: "var(--text-3)" }}>
              Actividad
            </p>
            <div className="flex flex-col gap-2">
              {(snap?.activity ?? []).map((a) => (
                <div key={a.id} className="text-[12px] leading-snug" style={{ color: "var(--text-2)" }}>
                  <span
                    className="inline-block px-1.5 rounded text-[10px] font-semibold mr-1"
                    style={{
                      background: a.actor === "aion" ? "var(--accent-subtle)" : "var(--surface-3)",
                      color: a.actor === "aion" ? "var(--gold-deep)" : "var(--text-2)",
                    }}
                  >
                    {a.actor}
                  </span>
                  {a.action} {a.detail}
                  <div className="text-[10px] mt-0.5" style={{ color: "var(--text-3)" }}>
                    {fmtTime(a.at)}
                  </div>
                </div>
              ))}
              {(snap?.activity ?? []).length === 0 && (
                <p className="text-[12px]" style={{ color: "var(--text-3)" }}>
                  Sin actividad todavía.
                </p>
              )}
            </div>
          </aside>
        )}
      </div>

      {active && (
        <CardSheet
          projectId={projectId}
          card={active}
          statuses={statuses}
          onClose={() => setActive(null)}
          onChanged={reload}
        />
      )}

      <style jsx>{`
        .seed-item {
          display: flex;
          flex-direction: column;
          align-items: flex-start;
          width: 100%;
          padding: 8px 10px;
          border-radius: 10px;
          text-align: left;
        }
        .seed-item:hover {
          background: var(--surface-2);
        }
        .seed-item strong {
          font-size: 12.5px;
          color: var(--text-1);
        }
        .seed-item span {
          font-size: 11px;
          color: var(--text-3);
        }
      `}</style>
    </div>
  );
}

function CardChip({
  card,
  onOpen,
  onDragStart,
  onDragEnd,
}: {
  card: BoardCard;
  onOpen: () => void;
  onDragStart: () => void;
  onDragEnd: () => void;
}) {
  const doneCount = card.checklist.filter((i) => i.done).length;
  return (
    <div
      draggable
      onDragStart={onDragStart}
      onDragEnd={onDragEnd}
      onClick={onOpen}
      className="rounded-xl p-2.5 cursor-pointer transition-shadow hover:shadow-sm"
      style={{ background: "var(--surface-0)", border: "1px solid var(--border)" }}
    >
      <p className="text-[13px] leading-snug" style={{ color: "var(--text-1)" }}>
        {card.title}
      </p>
      <div className="flex items-center gap-2 mt-2 flex-wrap">
        {card.priority > 0 && (
          <span className="inline-flex items-center gap-1 text-[10px]" style={{ color: PRIORITY_COLOR[card.priority] }}>
            <span className="h-2 w-2 rounded-full" style={{ background: PRIORITY_COLOR[card.priority] }} />
            {PRIORITY[card.priority]}
          </span>
        )}
        {typeof card.estimate_days === "number" && (
          <span className="text-[10px] inline-flex items-center gap-0.5" style={{ color: "var(--text-3)" }}>
            <Icon name="clock" size={10} /> {card.estimate_days}d
          </span>
        )}
        {card.checklist.length > 0 && (
          <span className="text-[10px] inline-flex items-center gap-0.5" style={{ color: doneCount === card.checklist.length ? "#2f9e6f" : "var(--text-3)" }}>
            <Icon name="check" size={10} /> {doneCount}/{card.checklist.length}
          </span>
        )}
        {card.deliverables.length > 0 && (
          <span className="text-[10px] inline-flex items-center gap-0.5" style={{ color: "var(--gold-deep)" }}>
            <Icon name="paperclip" size={10} /> {card.deliverables.length}
          </span>
        )}
        {card.assignee && (
          <span className="ml-auto text-[10px] px-1.5 rounded" style={{ background: "var(--surface-3)", color: "var(--text-2)" }}>
            {card.assignee}
          </span>
        )}
      </div>
    </div>
  );
}

function CardSheet({
  projectId,
  card,
  statuses,
  onClose,
  onChanged,
}: {
  projectId: string;
  card: BoardCard;
  statuses: BoardStatus[];
  onClose: () => void;
  onChanged: () => Promise<void>;
}) {
  const [title, setTitle] = useState(card.title);
  const [desc, setDesc] = useState(card.desc);
  const [comment, setComment] = useState("");
  const [newItem, setNewItem] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    setTitle(card.title);
    setDesc(card.desc);
  }, [card.id]); // eslint-disable-line react-hooks/exhaustive-deps

  const col = statuses.find((s) => s.id === card.status_id);

  async function patch(p: Parameters<typeof boardCardUpdate>[2]) {
    setBusy(true);
    await boardCardUpdate(projectId, card.id, p).catch(() => null);
    await onChanged();
    setBusy(false);
  }
  async function toggleItem(idx: number) {
    const items: BoardChecklistItem[] = card.checklist.map((it, i) => (i === idx ? { ...it, done: !it.done } : it));
    await boardCardChecklist(projectId, card.id, items);
    await onChanged();
  }
  async function addItem() {
    if (!newItem.trim()) return;
    const items = [...card.checklist, { text: newItem.trim(), done: false }];
    setNewItem("");
    await boardCardChecklist(projectId, card.id, items);
    await onChanged();
  }
  async function sendComment() {
    if (!comment.trim()) return;
    await boardCardComment(projectId, card.id, comment.trim());
    setComment("");
    await onChanged();
  }
  async function del() {
    if (!window.confirm("¿Borrar esta tarjeta?")) return;
    await boardCardDelete(projectId, card.id);
    onClose();
    await onChanged();
  }

  return (
    <div className="fixed inset-0 z-[60] flex justify-end" onClick={onClose}>
      <div className="absolute inset-0" style={{ background: "rgba(0,0,0,0.25)" }} />
      <aside
        className="relative w-[420px] max-w-[90vw] h-full overflow-y-auto p-5 flex flex-col gap-4"
        style={{ background: "var(--surface-1)", borderLeft: "1px solid var(--border)" }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-2">
          {col && (
            <span className="inline-flex items-center gap-1.5 text-[11px] px-2 py-1 rounded-md" style={{ background: "var(--surface-3)", color: "var(--text-2)" }}>
              <span className="h-2 w-2 rounded-full" style={{ background: col.color }} />
              {col.name}
            </span>
          )}
          <button className="ml-auto rounded-md p-1 opacity-70 hover:opacity-100" onClick={onClose}>
            <Icon name="stop" size={16} />
          </button>
        </div>

        <input
          className="input text-sm font-semibold"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          onBlur={() => title.trim() && title !== card.title && patch({ title: title.trim() })}
        />

        <textarea
          className="input text-sm min-h-[70px]"
          placeholder="Descripción…"
          value={desc}
          onChange={(e) => setDesc(e.target.value)}
          onBlur={() => desc !== card.desc && patch({ desc })}
        />

        <div className="grid grid-cols-2 gap-3">
          <label className="text-[11px]" style={{ color: "var(--text-3)" }}>
            Prioridad
            <select
              className="input text-sm mt-1"
              value={card.priority}
              onChange={(e) => patch({ priority: Number(e.target.value) })}
            >
              {PRIORITY.map((p, i) => (
                <option key={i} value={i}>
                  {p}
                </option>
              ))}
            </select>
          </label>
          <label className="text-[11px]" style={{ color: "var(--text-3)" }}>
            Estimación (días)
            <input
              type="number"
              min={0}
              step={0.5}
              className="input text-sm mt-1"
              defaultValue={card.estimate_days ?? ""}
              onBlur={(e) => patch({ estimate_days: Number(e.target.value) || 0 })}
            />
          </label>
          <label className="text-[11px]" style={{ color: "var(--text-3)" }}>
            Fecha límite
            <input
              type="date"
              className="input text-sm mt-1"
              defaultValue={card.due ?? ""}
              onBlur={(e) => patch({ due: e.target.value })}
            />
          </label>
          <label className="text-[11px]" style={{ color: "var(--text-3)" }}>
            Responsable
            <input
              className="input text-sm mt-1"
              placeholder="Ariel · AION…"
              defaultValue={card.assignee}
              onBlur={(e) => patch({ assignee: e.target.value })}
            />
          </label>
        </div>

        {/* Checklist */}
        <div>
          <p className="text-[11px] font-semibold uppercase tracking-[0.1em] mb-1.5" style={{ color: "var(--text-3)" }}>
            Checklist
          </p>
          <div className="flex flex-col gap-1">
            {card.checklist.map((it, i) => (
              <label key={i} className="flex items-center gap-2 text-[13px] cursor-pointer" style={{ color: "var(--text-1)" }}>
                <input type="checkbox" checked={it.done} onChange={() => toggleItem(i)} />
                <span style={{ textDecoration: it.done ? "line-through" : "none", opacity: it.done ? 0.6 : 1 }}>{it.text}</span>
              </label>
            ))}
          </div>
          <div className="flex gap-2 mt-2">
            <input
              className="input text-sm flex-1"
              placeholder="Nuevo ítem…"
              value={newItem}
              onChange={(e) => setNewItem(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && addItem()}
            />
            <button className="btn btn-ghost text-xs shrink-0" onClick={addItem}>
              <Icon name="plus" size={12} />
            </button>
          </div>
        </div>

        {/* Entregables */}
        {card.deliverables.length > 0 && (
          <div>
            <p className="text-[11px] font-semibold uppercase tracking-[0.1em] mb-1.5" style={{ color: "var(--text-3)" }}>
              Entregables
            </p>
            <div className="flex flex-col gap-1.5">
              {card.deliverables.map((d, i) => (
                <div key={i} className="flex items-center gap-2 text-[12px] px-2 py-1.5 rounded-lg" style={{ background: "var(--surface-2)" }}>
                  <Icon name="paperclip" size={12} />
                  <span className="px-1.5 rounded text-[10px]" style={{ background: "var(--accent-subtle)", color: "var(--gold-deep)" }}>
                    {d.kind}
                  </span>
                  <span className="truncate" style={{ color: "var(--text-1)" }}>
                    {d.title || d.reference}
                  </span>
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Comentario (queda en el log con tu nombre) */}
        <div className="mt-auto">
          <div className="flex gap-2">
            <input
              className="input text-sm flex-1"
              placeholder="Comentar…"
              value={comment}
              onChange={(e) => setComment(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && sendComment()}
            />
            <button className="btn text-xs shrink-0" onClick={sendComment} disabled={!comment.trim()}>
              <Icon name="send" size={12} />
            </button>
          </div>
          <button className="text-[11px] mt-3 inline-flex items-center gap-1 opacity-70 hover:opacity-100" style={{ color: "#c0594e" }} onClick={del} disabled={busy}>
            <Icon name="trash" size={12} /> Borrar tarjeta
          </button>
        </div>
      </aside>
    </div>
  );
}

function EmptyBoard({ onSeed }: { onSeed: () => void }) {
  return (
    <div className="h-full grid place-items-center">
      <div className="text-center max-w-sm" style={{ color: "var(--text-3)" }}>
        <span className="icon-chip mb-3 mx-auto" style={{ width: 48, height: 48, background: "var(--pastel-gold)", color: "var(--on-gold)" }}>
          <Icon name="target" size={24} />
        </span>
        <p className="text-sm mb-3">
          Tablero por etapas para avanzar el proyecto. Empieza sembrando un plan con tempística y buenas prácticas, o crea
          tus propias columnas.
        </p>
        <button className="btn btn-gold text-sm" onClick={onSeed}>
          <Icon name="sparkle" size={14} /> Sembrar un plan
        </button>
      </div>
    </div>
  );
}

function fmtTime(iso: string): string {
  try {
    return new Date(iso).toLocaleString(undefined, { day: "2-digit", month: "short", hour: "2-digit", minute: "2-digit" });
  } catch {
    return iso;
  }
}
