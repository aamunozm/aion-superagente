// Clientes de AION: control-plane (auth) y puente del núcleo (chat).

export const CONTROL_URL =
  process.env.NEXT_PUBLIC_CONTROL_URL ?? "http://127.0.0.1:8787";
export const BRIDGE_URL =
  process.env.NEXT_PUBLIC_BRIDGE_URL ?? "http://127.0.0.1:8765";

export type AuthResult = {
  id: string;
  email: string;
  token: string;
  recovery_code?: string;
};

async function authCall(path: string, email: string, password: string): Promise<AuthResult> {
  const res = await fetch(`${CONTROL_URL}${path}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ email, password }),
  });
  const data = await res.json();
  if (!res.ok) throw new Error(data.error ?? "error de autenticación");
  return data as AuthResult;
}

export const register = (email: string, password: string) =>
  authCall("/auth/register", email, password);
export const login = (email: string, password: string) =>
  authCall("/auth/login", email, password);

/// Recuperación de contraseña (local-first): email + código de recuperación + nueva.
export const resetPassword = async (email: string, code: string, newPassword: string) => {
  const res = await fetch(`${CONTROL_URL}/auth/reset`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ email, recovery_code: code, new_password: newPassword }),
  });
  const data = await res.json();
  if (!res.ok) throw new Error(data.error ?? "código o email inválido");
  return data as { ok: boolean };
};

export type ChatEvent =
  | { kind: "thinking"; text: string }
  | { kind: "answer"; text: string }
  | { kind: "done"; tokens: number; tps: number }
  | { kind: "error"; text: string };

export type AgentEvent =
  | { kind: "thought"; text: string; agent?: string }
  | { kind: "action"; text: string; agent?: string }
  | { kind: "observation"; text: string; agent?: string }
  | { kind: "answer"; text: string; steps?: number; agent?: string }
  | { kind: "confirm"; id: string; text: string }
  | { kind: "ask"; id: string; text: string }
  | { kind: "done" }
  | { kind: "error"; text: string };

/** Aprueba o rechaza una acción sensible que AION pidió confirmar (HITL). */
export async function confirmDecision(id: string, approved: boolean): Promise<void> {
  await fetch(`${BRIDGE_URL}/api/confirm`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ id, approved }),
  }).catch(() => {});
}

/** Responde (en texto) a una pregunta que el agente hizo a mitad de tarea. */
export async function answerQuestion(id: string, text: string): Promise<void> {
  await fetch(`${BRIDGE_URL}/api/ask`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ id, text }),
  }).catch(() => {});
}

/** Lee un cuerpo SSE y entrega cada evento `data:` parseado a `onEvent`. */
async function readSse<T>(res: Response, onEvent: (e: T) => void): Promise<void> {
  if (!res.body) throw new Error("sin cuerpo de respuesta");
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buf = "";
  for (;;) {
    const { value, done } = await reader.read();
    if (done) break;
    buf += decoder.decode(value, { stream: true });
    const parts = buf.split("\n\n");
    buf = parts.pop() ?? "";
    for (const part of parts) {
      const line = part.trim();
      if (!line.startsWith("data:")) continue;
      try {
        onEvent(JSON.parse(line.slice(5).trim()) as T);
      } catch {
        /* fragmento parcial */
      }
    }
  }
}

/** Chat con streaming de razonamiento + respuesta. */
/** Idioma elegido por el usuario (para que AION responda en él). */
function lang(): string {
  if (typeof window === "undefined") return "es";
  return localStorage.getItem("aion_lang") || "es";
}

export async function chatStream(
  prompt: string,
  think: boolean,
  onEvent: (e: ChatEvent) => void,
  convoId?: string,
  projectId?: string,
): Promise<void> {
  const res = await fetch(`${BRIDGE_URL}/api/chat`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      prompt,
      think,
      lang: lang(),
      convo_id: convoId ?? "default",
      project_id: projectId,
    }),
  });
  await readSse(res, onEvent);
}

// ── Proyectos (workspace estilo NotebookLM) ─────────────────────────────────

export type Project = { id: string; name: string; desc: string; icon: string; created: string; updated: string };
export type ProjectSource = { id: string; title: string; kind: string; content: string; active: boolean; created: string };
export type ProjectOutput = { id: string; kind: string; title: string; content: string; created: string };

async function jpost<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(`${BRIDGE_URL}${path}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  return (await res.json()) as T;
}

export async function projectsList(): Promise<Project[]> {
  const r = await fetch(`${BRIDGE_URL}/api/projects`).then((x) => x.json()).catch(() => ({ projects: [] }));
  return (r.projects ?? []) as Project[];
}
export const projectCreate = (name: string, desc: string, icon: string) =>
  jpost<{ ok: boolean; project?: Project; error?: string }>("/api/projects", { name, desc, icon });
export const projectRemove = (id: string) => jpost<{ ok: boolean }>("/api/projects/remove", { id });
export const projectGet = (id: string) =>
  jpost<{ ok: boolean; project?: Project; sources?: ProjectSource[]; outputs?: ProjectOutput[]; error?: string }>(
    "/api/project/get",
    { id },
  );
export const projectSourceAdd = (project_id: string, title: string, kind: string, content: string) =>
  jpost<{ ok: boolean; source?: ProjectSource; error?: string }>("/api/project/source/add", {
    project_id,
    title,
    kind,
    content,
  });
export const projectSourceToggle = (project_id: string, id: string, active: boolean) =>
  jpost<{ ok: boolean }>("/api/project/source/toggle", { project_id, id, active });
export const projectSourceRemove = (project_id: string, id: string) =>
  jpost<{ ok: boolean }>("/api/project/source/remove", { project_id, id });
export const projectStudioGenerate = (project_id: string, kind: string) =>
  jpost<{ ok: boolean; output?: ProjectOutput; error?: string }>("/api/project/studio/generate", {
    project_id,
    kind,
    lang: lang(),
  });
export const projectStudioRemove = (project_id: string, id: string) =>
  jpost<{ ok: boolean }>("/api/project/studio/remove", { project_id, id });

/** Reinicia el hilo de una conversación en el backend (nuevo chat). */
export async function chatReset(convoId: string): Promise<void> {
  await fetch(`${BRIDGE_URL}/api/chat/new`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ convo_id: convoId }),
  }).catch(() => {});
}

/** Agente ReAct con herramientas: emite pasos (thought/action/observation) + answer. */
export async function agentStream(
  task: string,
  onEvent: (e: AgentEvent) => void,
): Promise<void> {
  const res = await fetch(`${BRIDGE_URL}/api/agent`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ task, lang: lang() }),
  });
  await readSse(res, onEvent);
}

/** Equipo multiagente: orquestador + especialistas. Emite la actividad por rol. */
export async function crewStream(
  task: string,
  onEvent: (e: AgentEvent) => void,
): Promise<void> {
  const res = await fetch(`${BRIDGE_URL}/api/crew`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ task, lang: lang() }),
  });
  await readSse(res, onEvent);
}

// ── Adjuntos: documentos (biblioteca) y fotos (visión) ──────────────────

/** Sube un documento (.pdf/.txt/.md) a la biblioteca, bajo un dominio. */
export async function libraryUpload(
  domain: string,
  filename: string,
  contentB64: string,
): Promise<{ ok: boolean; passages: number; source: string; total_chunks: number }> {
  const res = await fetch(`${BRIDGE_URL}/api/library/upload`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ domain, filename, content_b64: contentB64 }),
  });
  const data = await res.json();
  if (data.error) throw new Error(data.error);
  return data;
}

export type LibraryDoc = { domain: string; source: string; chunks: number };

/** Lista los documentos de la biblioteca (agrupados por dominio/fuente). */
export async function libraryList(): Promise<{ total_chunks: number; documents: LibraryDoc[] }> {
  return jsonCall(`/api/library`);
}

/** Encola un libro para ingesta en segundo plano (no bloquea). */
export async function libraryEnqueue(
  domain: string,
  filename: string,
  contentB64: string,
): Promise<{ ok: boolean; id: string; queued: string }> {
  return jsonCall(`/api/library/enqueue`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ domain, filename, content_b64: contentB64 }),
  });
}

export type QueueStatus = {
  pending: number;
  processing: number;
  done: number;
  error: number;
  current: string | null;
};

/** Estado de la cola de ingesta. */
export async function libraryQueue(): Promise<QueueStatus> {
  return jsonCall(`/api/library/queue`);
}

/** Limpia de la cola los trabajos terminados. */
export async function libraryQueueClear(): Promise<{ cleared: number }> {
  return jsonCall(`/api/library/queue/clear`, { method: "POST" });
}

/** Elimina un documento de la biblioteca. */
export async function libraryRemove(domain: string, source: string): Promise<{ removed: number }> {
  return jsonCall(`/api/library/remove`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ domain, source }),
  });
}

/** Pregunta a la biblioteca: respuesta fundamentada con fuentes. */
export async function libraryAsk(
  query: string,
  domain?: string,
): Promise<{ answer: string; sources: { n: number; domain: string; source: string; idx: number; score: number }[] }> {
  return jsonCall(`/api/library/ask`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ query, domain }),
  });
}

/** Analiza una imagen adjunta con visión (gemma multimodal, local). */
export async function visionAsk(prompt: string, imageB64: string): Promise<string> {
  const res = await fetch(`${BRIDGE_URL}/api/vision`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ prompt, image_b64: imageB64 }),
  });
  const data = await res.json();
  if (data.error) throw new Error(data.error);
  return data.answer as string;
}

// ── Bóveda de credenciales (Llavero; la contraseña nunca se devuelve) ────

export type CredMeta = { host: string; user: string };

export async function credentialsList(): Promise<CredMeta[]> {
  const r = await jsonCall<{ credentials: CredMeta[] }>(`/api/credentials`);
  return r.credentials;
}

export async function credentialSet(host: string, user: string, pass: string): Promise<void> {
  await jsonCall(`/api/credentials`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ host, user, pass }),
  });
}

export async function credentialRemove(host: string): Promise<void> {
  await jsonCall(`/api/credentials/remove`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ host }),
  });
}

// ── Memoria de largo plazo ──────────────────────────────────────────────

export type MemoryStats = { count: number; path: string };
export type SleepReport = { before: number; merged: number; pruned: number; after: number };

async function jsonCall<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${BRIDGE_URL}${path}`, init);
  const data = await res.json();
  if (data.error) throw new Error(data.error);
  return data as T;
}

export const memoryStats = () => jsonCall<MemoryStats>("/api/memory");

export const memoryRemember = (text: string) =>
  jsonCall<{ ok: boolean; count: number }>("/api/memory/remember", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ text }),
  });

export const memorySleep = () =>
  jsonCall<SleepReport>("/api/memory/sleep", { method: "POST" });

/// Descarga la memoria como archivo JSONL (para llevarla a otro PC/Mac).
export const memoryExport = () => fetch(`${BRIDGE_URL}/api/memory/export`);

/// Importa memoria desde un archivo JSONL (fusiona, omite duplicados).
export const memoryImport = (jsonl: string) =>
  jsonCall<{ ok: boolean; added: number; count: number }>("/api/memory/import", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonl }),
  });

// ── Bandeja de AION (mensajes proactivos del agente hacia ti) ───────────────

export type InboxMessage = {
  id: string;
  at: string;
  kind: string;
  text: string;
  read: boolean;
};

// ── Onboarding: escaneo de hardware, modelos y proveedor ────────────────────

export type SystemScan = {
  os: string;
  arch: string;
  cpu_cores: number;
  ram_gb: number;
  disk_free_gb: number;
  gpu: string;
  tier: string;
  tier_reason: string;
};
export type ModelOption = {
  id: string;
  name: string;
  ollama_name: string;
  size_gb: number;
  tier: string;
  note: string;
  recommended: boolean;
};

export const systemScan = () =>
  jsonCall<{ scan: SystemScan; catalog: ModelOption[] }>("/api/system/scan");

export const providerSet = (cfg: {
  kind: string;
  model: string;
  base_url?: string;
  api_key?: string;
}) =>
  jsonCall<{ ok?: boolean; error?: string }>("/api/provider", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ base_url: "", api_key: "", ...cfg }),
  });

export const governanceSetup = (posture: string) =>
  jsonCall<{ ok: boolean }>("/api/governance/setup", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ posture }),
  });

/// Descarga un modelo local con progreso (SSE).
export type InstalledModel = { name: string; size_gb: number };

/** Modelos locales ya instalados en Ollama. */
export async function modelsInstalled(): Promise<InstalledModel[]> {
  const r = await jsonCall<{ installed: InstalledModel[] }>(`/api/models/installed`);
  return r.installed;
}

/** Elimina un modelo local (no permite borrar el que está en uso). */
export async function modelsRemove(model: string): Promise<void> {
  await jsonCall(`/api/models/remove`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ model }),
  });
}

export async function modelsPull(
  model: string,
  onEvent: (e: { kind: string; status?: string; percent?: number; text?: string }) => void,
): Promise<void> {
  const res = await fetch(`${BRIDGE_URL}/api/models/pull`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ model }),
  });
  await readSse(res, onEvent);
}

/// Estado de preparación del motor/modelo (para mostrar "descargando…" en 1er arranque).
export const status = () =>
  jsonCall<{ engine_up: boolean; model_ready: boolean; engine: string }>("/api/status");

export const inboxList = () =>
  jsonCall<{ unread: InboxMessage[]; unread_count: number; all: InboxMessage[] }>("/api/inbox");

export const inboxRead = (id?: string) =>
  jsonCall<{ ok: boolean }>("/api/inbox/read", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(id ? { id } : {}),
  });
