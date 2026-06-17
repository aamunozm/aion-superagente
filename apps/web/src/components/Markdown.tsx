"use client";

// Renderizador de Markdown PROPIO (cero dependencias) — fiel al ethos local-first y
// minimal de AION (3 deps en total; aquí no sumamos ninguna). El LLM emite Markdown bien
// formado; este parser por BLOQUES + INLINE cubre lo que de verdad produce —encabezados,
// listas (anidadas), tablas GFM, código, citas, enlaces, negritas/cursivas— y lo presenta
// ordenado (estilo Perplexity) con los tokens de diseño del proyecto. Antes el chat volcaba
// la respuesta como texto plano (whitespace-pre-wrap) y el Markdown salía crudo y desordenado.

import React from "react";

// ─────────────────────────── INLINE (negrita, cursiva, código, enlaces) ───────────────────────────

// Devuelve el primer match de cualquier patrón inline, el más a la IZQUIERDA (para respetar
// el orden del texto). `code` es terminal (su contenido es literal, sin re-parsear).
type InlineHit = { start: number; end: number; node: React.ReactNode };

function firstInline(text: string, keyBase: string): InlineHit | null {
  const patterns: { re: RegExp; make: (m: RegExpExecArray, k: string) => React.ReactNode }[] = [
    // Código en línea: contenido LITERAL (no se re-parsea).
    { re: /`([^`]+)`/, make: (m, k) => <code key={k} className="md-code">{m[1]}</code> },
    // Enlace [texto](url): el texto sí admite formato anidado.
    {
      re: /\[([^\]]+)\]\(([^)\s]+)\)/,
      make: (m, k) => (
        <a key={k} href={m[2]} target="_blank" rel="noopener noreferrer" className="md-link">
          {inline(m[1], k + "t")}
        </a>
      ),
    },
    // Negrita **x** o __x__ (antes que cursiva, porque ** contiene *).
    { re: /\*\*([^*]+)\*\*|__([^_]+)__/, make: (m, k) => <strong key={k}>{inline(m[1] ?? m[2], k + "b")}</strong> },
    // Cursiva *x* o _x_.
    { re: /\*([^*\n]+)\*|_([^_\n]+)_/, make: (m, k) => <em key={k}>{inline(m[1] ?? m[2], k + "i")}</em> },
    // ~~tachado~~
    { re: /~~([^~]+)~~/, make: (m, k) => <del key={k}>{inline(m[1], k + "s")}</del> },
  ];
  let best: { idx: number; m: RegExpExecArray; make: (m: RegExpExecArray, k: string) => React.ReactNode } | null = null;
  for (const p of patterns) {
    const m = p.re.exec(text);
    if (m && (best === null || m.index < best.idx)) {
      best = { idx: m.index, m, make: p.make };
    }
  }
  if (!best) return null;
  return {
    start: best.idx,
    end: best.idx + best.m[0].length,
    node: best.make(best.m, `${keyBase}-${best.idx}`),
  };
}

// Convierte texto con formato inline en nodos React.
function inline(text: string, keyBase = "x"): React.ReactNode[] {
  const out: React.ReactNode[] = [];
  let rest = text;
  let guard = 0;
  while (rest.length && guard++ < 500) {
    const hit = firstInline(rest, keyBase + guard);
    if (!hit) {
      out.push(rest);
      break;
    }
    if (hit.start > 0) out.push(rest.slice(0, hit.start));
    out.push(hit.node);
    rest = rest.slice(hit.end);
  }
  return out;
}

// ─────────────────────────── BLOQUES ───────────────────────────

const BULLET = /^(\s*)[-*+]\s+(.*)$/;
const ORDERED = /^(\s*)\d+[.)]\s+(.*)$/;

// Agrupa ítems de lista consecutivos (con su indentación) y los anida por nivel.
function renderList(lines: string[], start: number, key: string): { node: React.ReactNode; next: number } {
  type Item = { indent: number; ordered: boolean; text: string };
  const items: Item[] = [];
  let i = start;
  while (i < lines.length) {
    const b = BULLET.exec(lines[i]);
    const o = ORDERED.exec(lines[i]);
    if (b) items.push({ indent: b[1].length, ordered: false, text: b[2] });
    else if (o) items.push({ indent: o[1].length, ordered: true, text: o[2] });
    else break;
    i++;
  }
  // Construye recursivamente respetando la indentación (anidamiento).
  const build = (idx: number, level: number): { nodes: React.ReactNode[]; consumed: number } => {
    const nodes: React.ReactNode[] = [];
    let j = idx;
    while (j < items.length && items[j].indent >= level) {
      if (items[j].indent > level) {
        // Sub-lista: la cuelga del último ítem ya emitido.
        const sub = build(j, items[j].indent);
        const ordered = items[j].ordered;
        const ListTag = ordered ? "ol" : "ul";
        nodes.push(
          React.createElement(ListTag, { key: `${key}-s${j}`, className: ordered ? "md-ol" : "md-ul" }, sub.nodes),
        );
        j += sub.consumed;
      } else {
        nodes.push(
          <li key={`${key}-li${j}`} className="md-li">
            {inline(items[j].text, `${key}-${j}`)}
          </li>,
        );
        j++;
      }
    }
    return { nodes, consumed: j - idx };
  };
  const top = build(0, items[0].indent);
  const ordered = items[0].ordered;
  const ListTag = ordered ? "ol" : "ul";
  const node = React.createElement(ListTag, { key, className: ordered ? "md-ol" : "md-ul" }, top.nodes);
  return { node, next: i };
}

// Tabla GFM: fila de cabecera + separador |---|:--| + filas.
function renderTable(lines: string[], start: number, key: string): { node: React.ReactNode; next: number } | null {
  const splitRow = (l: string) =>
    l.trim().replace(/^\||\|$/g, "").split("|").map((c) => c.trim());
  const sep = lines[start + 1] ?? "";
  if (!/^\s*\|?[\s:|-]+\|[\s:|-]+\|?\s*$/.test(sep) || !sep.includes("-")) return null;
  const header = splitRow(lines[start]);
  let i = start + 2;
  const rows: string[][] = [];
  while (i < lines.length && lines[i].includes("|") && lines[i].trim() !== "") {
    rows.push(splitRow(lines[i]));
    i++;
  }
  const node = (
    <div key={key} className="md-table-wrap">
      <table className="md-table">
        <thead>
          <tr>{header.map((c, x) => <th key={x}>{inline(c, `${key}h${x}`)}</th>)}</tr>
        </thead>
        <tbody>
          {rows.map((r, y) => (
            <tr key={y}>{header.map((_, x) => <td key={x}>{inline(r[x] ?? "", `${key}r${y}c${x}`)}</td>)}</tr>
          ))}
        </tbody>
      </table>
    </div>
  );
  return { node, next: i };
}

function renderBlocks(src: string): React.ReactNode[] {
  const lines = src.replace(/\r\n/g, "\n").split("\n");
  const out: React.ReactNode[] = [];
  let i = 0;
  let k = 0;
  while (i < lines.length) {
    const line = lines[i];
    const key = `b${k++}`;

    // Línea en blanco → separadora.
    if (line.trim() === "") { i++; continue; }

    // Código cercado ``` ... ```
    const fence = /^\s*```(\w*)\s*$/.exec(line);
    if (fence) {
      const buf: string[] = [];
      i++;
      while (i < lines.length && !/^\s*```\s*$/.test(lines[i])) { buf.push(lines[i]); i++; }
      i++; // cierra ```
      out.push(<pre key={key} className="md-pre"><code>{buf.join("\n")}</code></pre>);
      continue;
    }

    // Encabezados ATX (# .. ######)
    const h = /^(#{1,6})\s+(.*)$/.exec(line);
    if (h) {
      const lvl = h[1].length;
      const Tag = `h${Math.min(lvl + 1, 6)}` as keyof React.JSX.IntrinsicElements; // h1→h2… no robar el h1 de la página
      out.push(React.createElement(Tag, { key, className: `md-h md-h${lvl}` }, inline(h[2], key)));
      i++;
      continue;
    }

    // Regla horizontal
    if (/^\s*([-*_])(\s*\1){2,}\s*$/.test(line)) { out.push(<hr key={key} className="md-hr" />); i++; continue; }

    // Cita >
    if (/^\s*>\s?/.test(line)) {
      const buf: string[] = [];
      while (i < lines.length && /^\s*>\s?/.test(lines[i])) { buf.push(lines[i].replace(/^\s*>\s?/, "")); i++; }
      out.push(<blockquote key={key} className="md-quote">{renderBlocks(buf.join("\n"))}</blockquote>);
      continue;
    }

    // Tabla GFM
    if (line.includes("|") && i + 1 < lines.length) {
      const t = renderTable(lines, i, key);
      if (t) { out.push(t.node); i = t.next; continue; }
    }

    // Listas (con anidamiento)
    if (BULLET.test(line) || ORDERED.test(line)) {
      const l = renderList(lines, i, key);
      out.push(l.node);
      i = l.next;
      continue;
    }

    // Párrafo: junta líneas hasta una en blanco o un nuevo bloque.
    const buf: string[] = [line];
    i++;
    while (
      i < lines.length &&
      lines[i].trim() !== "" &&
      !/^\s*```/.test(lines[i]) &&
      !/^(#{1,6})\s/.test(lines[i]) &&
      !/^\s*>\s?/.test(lines[i]) &&
      !BULLET.test(lines[i]) &&
      !ORDERED.test(lines[i])
    ) {
      buf.push(lines[i]);
      i++;
    }
    out.push(<p key={key} className="md-p">{inline(buf.join(" "), key)}</p>);
  }
  return out;
}

export default function Markdown({ children, className }: { children: string; className?: string }) {
  return <div className={`md${className ? " " + className : ""}`}>{renderBlocks(children ?? "")}</div>;
}
