"use client";

// **Editor visual de flujos (React Flow / @xyflow, MIT)** — el lienzo nativo que reemplaza la idea
// de embeber n8n. Edita un grafo de nodos (trigger/acción/condición), lo guarda en /api/flows y lo
// ejecuta mostrando el resultado por nodo. 100% local, on-brand.

import "@xyflow/react/dist/style.css";
import { useCallback, useEffect, useMemo, useState } from "react";
import {
  ReactFlow,
  ReactFlowProvider,
  Background,
  Controls,
  Handle,
  Position,
  applyNodeChanges,
  applyEdgeChanges,
  addEdge,
  type Node as RfNode,
  type Edge as RfEdge,
  type Connection,
  type NodeProps,
} from "@xyflow/react";
import Icon from "@/components/Icon";
import {
  flowsList,
  flowsSet,
  flowsRemove,
  flowsRun,
  flowsMigrate,
  type Flow,
  type FlowNode,
  type FlowRun,
} from "@/lib/api";

// Herramientas ejecutables (deben coincidir con flow_registry del backend: set seguro + entregables).
const TOOLS: { tool: string; label: string }[] = [
  { tool: "seo_audit", label: "Auditar SEO de una web" },
  { tool: "generate_document", label: "Generar documento (PDF/Word)" },
  { tool: "make_note", label: "Crear nota" },
  { tool: "web_search", label: "Buscar en internet" },
  { tool: "web_fetch", label: "Leer una URL" },
  { tool: "library_search", label: "Buscar en biblioteca" },
  { tool: "graph_search", label: "Buscar en el grafo" },
  { tool: "memory_search", label: "Buscar en memoria" },
  { tool: "remember", label: "Guardar recuerdo" },
  { tool: "calculator", label: "Calcular (aritmética)" },
  { tool: "files_list", label: "Listar archivos" },
  { tool: "file_read", label: "Leer un archivo" },
  { tool: "calendar_list", label: "Mirar la agenda" },
  { tool: "contacts_search", label: "Buscar contacto" },
];
const toolLabel = (t: string) => TOOLS.find((x) => x.tool === t)?.label ?? t;
const WHENS = ["", "ok", "err", "true", "false"];

let SEQ = 0;
const uid = (p: string) => `${p}${Date.now().toString(36)}${(SEQ++).toString(36)}`;

// ── Nodos personalizados (on-brand) ─────────────────────────────────────────
type NData = { label: string; sub?: string; tone: string };
function Box({ data, tone, top, bottom }: { data: NData; tone: string; top: boolean; bottom: boolean }) {
  return (
    <div
      className="rounded-xl px-3 py-2 text-[12px] shadow-sm"
      style={{ background: "var(--surface-1)", border: `1.5px solid ${tone}`, minWidth: 150 }}
    >
      {top && <Handle type="target" position={Position.Top} style={{ background: tone }} />}
      <div className="font-semibold" style={{ color: "var(--text-1)" }}>
        {data.label}
      </div>
      {data.sub && (
        <div className="text-[10px] mt-0.5 truncate" style={{ color: "var(--text-3)", maxWidth: 180 }}>
          {data.sub}
        </div>
      )}
      {bottom && <Handle type="source" position={Position.Bottom} style={{ background: tone }} />}
    </div>
  );
}
const C = { trigger: "#2f9e6f", action: "#2563eb", condition: "#b45309" };
const TriggerNode = (p: NodeProps) => <Box data={p.data as NData} tone={C.trigger} top={false} bottom />;
const ActionNode = (p: NodeProps) => <Box data={p.data as NData} tone={C.action} top bottom />;
const ConditionNode = (p: NodeProps) => <Box data={p.data as NData} tone={C.condition} top bottom />;

// ── Conversión AION ↔ React Flow ─────────────────────────────────────────────
function rfFromFlow(flow: Flow): { nodes: RfNode[]; edges: RfEdge[] } {
  const nodes: RfNode[] = flow.nodes.map((n) => {
    let label = n.title || "";
    let sub = "";
    if (n.kind === "trigger") {
      label = label || "Inicio";
      sub = n.trigger.type === "interval" ? `cada ${n.trigger.minutes} min` : n.trigger.type;
    } else if (n.kind === "action") {
      label = label || toolLabel(n.tool);
      sub = n.input;
    } else {
      label = label || "Condición";
      sub = n.test;
    }
    return {
      id: n.id,
      type: n.kind,
      position: { x: n.x, y: n.y },
      data: { label, sub, tone: n.kind },
    };
  });
  const edges: RfEdge[] = flow.edges.map((e) => ({
    id: e.id,
    source: e.from,
    target: e.to,
    label: e.when || undefined,
    data: { when: e.when },
    animated: e.when === "err",
  }));
  return { nodes, edges };
}

function flowFromRf(base: Flow, nodes: RfNode[], edges: RfEdge[]): Flow {
  const byId = new Map(base.nodes.map((n) => [n.id, n]));
  const outNodes: FlowNode[] = nodes.map((rn) => {
    const orig = byId.get(rn.id);
    const x = rn.position.x;
    const y = rn.position.y;
    const title = (rn.data?.label as string) ?? "";
    if (rn.type === "trigger") {
      const trigger = orig && orig.kind === "trigger" ? orig.trigger : { type: "manual" as const };
      return { id: rn.id, title, x, y, kind: "trigger", trigger };
    }
    if (rn.type === "condition") {
      const test = orig && orig.kind === "condition" ? orig.test : "nonempty";
      return { id: rn.id, title, x, y, kind: "condition", test };
    }
    const tool = orig && orig.kind === "action" ? orig.tool : "calculator";
    const input = orig && orig.kind === "action" ? orig.input : "";
    return { id: rn.id, title, x, y, kind: "action", tool, input };
  });
  const outEdges = edges.map((re) => ({
    id: re.id,
    from: re.source,
    to: re.target,
    when: ((re.data?.when as string) ?? (typeof re.label === "string" ? re.label : "")) || "",
  }));
  return { ...base, nodes: outNodes, edges: outEdges };
}

const nodeTypes = { trigger: TriggerNode, action: ActionNode, condition: ConditionNode };

function emptyFlow(): Flow {
  return {
    id: uid("f"),
    name: "Flujo nuevo",
    description: "",
    enabled: true,
    nodes: [{ id: "trigger", title: "Inicio", x: 80, y: 40, kind: "trigger", trigger: { type: "manual" } }],
    edges: [],
  };
}

function Canvas() {
  const [flows, setFlows] = useState<Flow[]>([]);
  const [flow, setFlow] = useState<Flow>(() => emptyFlow());
  const [nodes, setNodes] = useState<RfNode[]>([]);
  const [edges, setEdges] = useState<RfEdge[]>([]);
  const [selNode, setSelNode] = useState<string | null>(null);
  const [selEdge, setSelEdge] = useState<string | null>(null);
  const [run, setRun] = useState<FlowRun | null>(null);
  const [busy, setBusy] = useState(false);
  const [savedAt, setSavedAt] = useState<string>("");

  const reloadList = useCallback(async () => {
    const r = await flowsList().catch(() => null);
    if (r?.flows) setFlows(r.flows);
  }, []);

  useEffect(() => {
    reloadList();
  }, [reloadList]);

  const loadFlow = useCallback((f: Flow) => {
    setFlow(f);
    const { nodes, edges } = rfFromFlow(f);
    setNodes(nodes);
    setEdges(edges);
    setSelNode(null);
    setSelEdge(null);
    setRun(null);
  }, []);

  // Carga el primer flujo existente al entrar (si hay).
  useEffect(() => {
    if (flows.length && flow.nodes.length === 1 && !savedAt) loadFlow(flows[0]);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [flows]);

  useEffect(() => {
    // Pinta el flujo en blanco recién creado.
    if (nodes.length === 0) {
      const { nodes: n, edges: e } = rfFromFlow(flow);
      setNodes(n);
      setEdges(e);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const onConnect = useCallback((c: Connection) => setEdges((eds) => addEdge({ ...c, data: { when: "" } }, eds)), []);

  const addNode = (kind: "action" | "condition") => {
    const id = uid("n");
    const nn: RfNode = {
      id,
      type: kind,
      position: { x: 120 + Math.random() * 120, y: 160 + Math.random() * 120 },
      data:
        kind === "action"
          ? { label: toolLabel("seo_audit"), sub: "", tone: "action" }
          : { label: "Condición", sub: "nonempty", tone: "condition" },
    };
    // Guarda los campos por tipo en el modelo base para no perderlos al serializar.
    setFlow((f) => ({
      ...f,
      nodes: [
        ...f.nodes,
        kind === "action"
          ? { id, title: "", x: nn.position.x, y: nn.position.y, kind: "action", tool: "seo_audit", input: "" }
          : { id, title: "", x: nn.position.x, y: nn.position.y, kind: "condition", test: "nonempty" },
      ],
    }));
    setNodes((ns) => [...ns, nn]);
    setSelNode(id);
  };

  // Edición del nodo/edge seleccionado: actualiza el modelo base (flow) y refresca el lienzo.
  const patchNode = (id: string, patch: Partial<FlowNode>) => {
    setFlow((f) => {
      const nodes2 = f.nodes.map((n) => (n.id === id ? ({ ...n, ...patch } as FlowNode) : n));
      const f2 = { ...f, nodes: nodes2 };
      const { nodes: rn } = rfFromFlow({ ...f2, edges: [] });
      setNodes((cur) => cur.map((c) => rn.find((x) => x.id === c.id) ? { ...c, data: rn.find((x) => x.id === c.id)!.data } : c));
      return f2;
    });
  };
  const patchEdgeWhen = (id: string, when: string) => {
    setEdges((eds) => eds.map((e) => (e.id === id ? { ...e, label: when || undefined, data: { when }, animated: when === "err" } : e)));
  };

  const currentModel = () => flowFromRf(flow, nodes, edges);

  const save = async () => {
    setBusy(true);
    const m = currentModel();
    const r = await flowsSet(m).catch(() => null);
    setBusy(false);
    if (r?.ok) {
      setFlow(m);
      setSavedAt(new Date().toLocaleTimeString());
      reloadList();
    }
  };
  const doRun = async () => {
    await save();
    setBusy(true);
    const r = await flowsRun(flow.id).catch(() => null);
    setBusy(false);
    if (r) setRun(r);
  };
  const newFlow = () => {
    const f = emptyFlow();
    loadFlow(f);
    setSavedAt("");
  };
  const del = async () => {
    if (!window.confirm("¿Borrar este flujo?")) return;
    await flowsRemove(flow.id);
    await reloadList();
    newFlow();
  };
  const migrate = async () => {
    setBusy(true);
    const r = await flowsMigrate().catch(() => null);
    setBusy(false);
    await reloadList();
    if (r?.ok) window.alert(`Migrados ${r.added ?? 0} flujo(s) lineales al editor.`);
  };

  const selectedNode = useMemo(() => flow.nodes.find((n) => n.id === selNode) ?? null, [flow.nodes, selNode]);
  const selectedEdge = useMemo(() => edges.find((e) => e.id === selEdge) ?? null, [edges, selEdge]);

  return (
    <div className="h-full flex min-h-0">
      {/* Lista de flujos */}
      <aside className="w-56 shrink-0 flex flex-col min-h-0" style={{ borderRight: "1px solid var(--border)" }}>
        <div className="flex items-center gap-2 px-3 h-12 shrink-0" style={{ borderBottom: "1px solid var(--border)" }}>
          <span className="text-xs font-semibold uppercase tracking-[0.12em]" style={{ color: "var(--text-3)" }}>
            Flujos
          </span>
          <button className="ml-auto rounded-md p-1 opacity-70 hover:opacity-100" onClick={newFlow} title="Nuevo flujo">
            <Icon name="plus" size={16} />
          </button>
        </div>
        <div className="flex-1 overflow-y-auto p-2 flex flex-col gap-1">
          {flows.map((f) => (
            <button
              key={f.id}
              onClick={() => loadFlow(f)}
              className="text-left rounded-lg px-2.5 py-2 text-[13px]"
              style={{
                background: f.id === flow.id ? "var(--surface-2)" : "transparent",
                color: "var(--text-1)",
              }}
            >
              <div className="truncate font-medium">{f.name}</div>
              <div className="text-[10px]" style={{ color: "var(--text-3)" }}>
                {f.nodes.length} nodos
              </div>
            </button>
          ))}
          {flows.length === 0 && (
            <button className="btn btn-ghost text-xs mt-1" onClick={migrate}>
              <Icon name="refresh" size={12} /> Migrar flujos lineales
            </button>
          )}
        </div>
      </aside>

      {/* Lienzo */}
      <section className="flex-1 flex flex-col min-w-0 min-h-0">
        <div className="flex items-center gap-2 px-3 h-12 shrink-0" style={{ borderBottom: "1px solid var(--border)" }}>
          <input
            className="input text-sm font-semibold"
            style={{ maxWidth: 240 }}
            value={flow.name}
            onChange={(e) => setFlow((f) => ({ ...f, name: e.target.value }))}
          />
          <button className="btn btn-ghost text-xs" onClick={() => addNode("action")}>
            <Icon name="plus" size={12} /> Acción
          </button>
          <button className="btn btn-ghost text-xs" onClick={() => addNode("condition")}>
            <Icon name="plus" size={12} /> Condición
          </button>
          <div className="ml-auto flex items-center gap-2">
            {savedAt && <span className="text-[10px]" style={{ color: "var(--text-3)" }}>guardado {savedAt}</span>}
            <button className="btn btn-ghost text-xs" onClick={del} title="Borrar flujo">
              <Icon name="trash" size={12} />
            </button>
            <button className="btn text-xs" onClick={save} disabled={busy}>
              <Icon name="check" size={12} /> Guardar
            </button>
            <button className="btn btn-gold text-xs" onClick={doRun} disabled={busy}>
              <Icon name="play" size={12} /> Ejecutar
            </button>
          </div>
        </div>

        <div className="flex-1 min-h-0">
          <ReactFlow
            nodes={nodes}
            edges={edges}
            nodeTypes={nodeTypes}
            onNodesChange={(ch) => setNodes((ns) => applyNodeChanges(ch, ns))}
            onEdgesChange={(ch) => setEdges((es) => applyEdgeChanges(ch, es))}
            onConnect={onConnect}
            onNodeClick={(_, n) => { setSelNode(n.id); setSelEdge(null); }}
            onEdgeClick={(_, e) => { setSelEdge(e.id); setSelNode(null); }}
            onPaneClick={() => { setSelNode(null); setSelEdge(null); }}
            fitView
            proOptions={{ hideAttribution: true }}
          >
            <Background color="var(--border)" gap={18} />
            <Controls showInteractive={false} />
          </ReactFlow>
        </div>

        {run && (
          <div className="shrink-0 max-h-40 overflow-y-auto px-4 py-2" style={{ borderTop: "1px solid var(--border)", background: "var(--surface-1)" }}>
            <div className="text-[11px] font-semibold mb-1" style={{ color: run.ok ? "#2f9e6f" : "#c0594e" }}>
              {run.ok ? "✓ Flujo completado" : run.stopped_for_approval ? "⏸ Detenido: requiere tu aprobación" : "✕ Flujo con errores"}
            </div>
            {run.steps.map((s, i) => (
              <div key={i} className="text-[11px] py-0.5 flex gap-2" style={{ color: "var(--text-2)" }}>
                <span style={{ color: s.ok ? "#2f9e6f" : "#c0594e" }}>{s.ok ? "✓" : "✕"}</span>
                <span className="font-medium" style={{ color: "var(--text-1)" }}>{s.tool}</span>
                <span className="truncate">{s.output}</span>
              </div>
            ))}
          </div>
        )}
      </section>

      {/* Inspector */}
      <aside className="w-72 shrink-0 overflow-y-auto p-4 flex flex-col gap-3" style={{ borderLeft: "1px solid var(--border)" }}>
        {selectedNode ? (
          <>
            <p className="text-[11px] font-semibold uppercase tracking-[0.1em]" style={{ color: "var(--text-3)" }}>
              {selectedNode.kind === "trigger" ? "Disparador" : selectedNode.kind === "action" ? "Acción" : "Condición"}
            </p>
            <label className="text-[11px]" style={{ color: "var(--text-3)" }}>
              Título
              <input
                className="input text-sm mt-1"
                value={selectedNode.title}
                onChange={(e) => patchNode(selectedNode.id, { title: e.target.value })}
              />
            </label>

            {selectedNode.kind === "action" && (
              <>
                <label className="text-[11px]" style={{ color: "var(--text-3)" }}>
                  Herramienta
                  <select
                    className="input text-sm mt-1"
                    value={selectedNode.tool}
                    onChange={(e) => patchNode(selectedNode.id, { tool: e.target.value } as Partial<FlowNode>)}
                  >
                    {TOOLS.map((t) => (
                      <option key={t.tool} value={t.tool}>{t.label}</option>
                    ))}
                  </select>
                </label>
                <label className="text-[11px]" style={{ color: "var(--text-3)" }}>
                  Entrada <span style={{ color: "var(--text-3)" }}>({"{{in}}"} = valor entrante)</span>
                  <textarea
                    className="input text-sm mt-1 min-h-[64px]"
                    value={selectedNode.input}
                    onChange={(e) => patchNode(selectedNode.id, { input: e.target.value } as Partial<FlowNode>)}
                    placeholder="p.ej. https://cliente.it  ·  Título ::: contenido"
                  />
                </label>
              </>
            )}

            {selectedNode.kind === "condition" && (
              <label className="text-[11px]" style={{ color: "var(--text-3)" }}>
                Test (enruta true/false)
                <input
                  className="input text-sm mt-1"
                  value={selectedNode.test}
                  onChange={(e) => patchNode(selectedNode.id, { test: e.target.value } as Partial<FlowNode>)}
                  placeholder="ok · nonempty · contains:TEXTO · equals:TEXTO"
                />
              </label>
            )}

            {selectedNode.kind === "trigger" && (
              <label className="text-[11px]" style={{ color: "var(--text-3)" }}>
                Tipo
                <select
                  className="input text-sm mt-1"
                  value={selectedNode.trigger.type}
                  onChange={(e) => {
                    const t = e.target.value;
                    const trigger = t === "interval" ? { type: "interval" as const, minutes: 60 } : t === "event" ? { type: "event" as const, kind: "" } : { type: "manual" as const };
                    patchNode(selectedNode.id, { trigger } as Partial<FlowNode>);
                  }}
                >
                  <option value="manual">Manual</option>
                  <option value="interval">Cada N minutos</option>
                  <option value="event">Por evento</option>
                </select>
              </label>
            )}
          </>
        ) : selectedEdge ? (
          <>
            <p className="text-[11px] font-semibold uppercase tracking-[0.1em]" style={{ color: "var(--text-3)" }}>
              Conexión
            </p>
            <label className="text-[11px]" style={{ color: "var(--text-3)" }}>
              Cuándo seguirla
              <select
                className="input text-sm mt-1"
                value={(selectedEdge.data?.when as string) ?? ""}
                onChange={(e) => patchEdgeWhen(selectedEdge.id, e.target.value)}
              >
                {WHENS.map((w) => (
                  <option key={w} value={w}>{w === "" ? "Siempre" : w}</option>
                ))}
              </select>
            </label>
            <p className="text-[11px]" style={{ color: "var(--text-3)" }}>
              Tras una acción usa <b>ok/err</b>; tras una condición <b>true/false</b>.
            </p>
          </>
        ) : (
          <p className="text-[12px]" style={{ color: "var(--text-3)" }}>
            Selecciona un nodo o una conexión para editarlo. Arrastra desde el borde inferior de un
            nodo al superior de otro para conectarlos.
          </p>
        )}
      </aside>
    </div>
  );
}

export function FlowEditor() {
  return (
    <ReactFlowProvider>
      <Canvas />
    </ReactFlowProvider>
  );
}
