"use client";

import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

/**
 * MOLÉCULA: Markdown — renderiza las respuestas de AION (estilo Perplexity/Claude):
 * encabezados, listas, tablas GFM, código, citas y enlaces.
 *
 * Todo el tipografiado vive en `.md` (globals.css). Las tablas y el código se
 * envuelven en un contenedor con scroll horizontal propio para que NUNCA
 * desborden la burbuja del chat (ver `.md-scroll`).
 *
 * Citas estilo NotebookLM (opcional): los tokens `«Título»` se convierten en
 * botones que saltan a la fuente. Se activa pasando `onCitation`; `isCitation`
 * decide qué títulos son fuentes válidas (los demás quedan como texto normal).
 */
type Props = {
  children: string;
  onCitation?: (title: string) => void;
  isCitation?: (title: string) => boolean;
};

const CITE_TOKEN = /«([^»]+)»/g;
const CITE_HREF = "#aion-cite:";

export default function Markdown({ children, onCitation, isCitation }: Props) {
  // Si hay manejador de citas, transformamos «Título» en un enlace interno que
  // el override de `a` convierte en botón. Encodeamos el título en el href para
  // no romper la sintaxis Markdown con caracteres especiales del título.
  const source = onCitation
    ? children.replace(
        CITE_TOKEN,
        (full, title) => `[${full}](${CITE_HREF}${encodeURIComponent(title)})`,
      )
    : children;

  return (
    <div className="md">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          table: ({ node, ...props }) => (
            <div className="md-scroll">
              <table {...props} />
            </div>
          ),
          pre: ({ node, ...props }) => (
            <div className="md-scroll">
              <pre {...props} />
            </div>
          ),
          a: ({ node, href, children: linkChildren, ...props }) => {
            if (href && href.startsWith(CITE_HREF)) {
              const title = decodeURIComponent(href.slice(CITE_HREF.length));
              if (!isCitation || isCitation(title)) {
                return (
                  <button
                    type="button"
                    className="md-cite"
                    title="Ver fuente"
                    onClick={() => onCitation?.(title)}
                  >
                    {linkChildren}
                  </button>
                );
              }
              // No es una fuente válida: mostrar el texto tal cual, sin enlace.
              return <span>{linkChildren}</span>;
            }
            return (
              <a {...props} href={href} target="_blank" rel="noreferrer noopener">
                {linkChildren}
              </a>
            );
          },
        }}
      >
        {source}
      </ReactMarkdown>
    </div>
  );
}
