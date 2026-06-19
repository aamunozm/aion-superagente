"use client";

import { useState } from "react";
import ReactMarkdown, { defaultUrlTransform } from "react-markdown";
import remarkGfm from "remark-gfm";
import { useLightbox } from "@/lib/lightbox";

// react-markdown sanea las URLs y por defecto DESCARTA los data: (anti-XSS), lo que
// rompía las fotos del agente (cara reconocida, screenshots) que llegan como
// `data:image/jpeg;base64,…`. Permitimos data-URLs de imagen RASTER (sin SVG, que sí
// puede llevar script) y dejamos el saneo por defecto para todo lo demás.
const RASTER_DATA = /^data:image\/(png|jpe?g|gif|webp);/i;
const safeUrl = (url: string) => (RASTER_DATA.test(url) ? url : defaultUrlTransform(url));

/**
 * MOLÉCULA: Markdown — renderiza las respuestas de AION (estilo Perplexity/Claude):
 * encabezados, listas, tablas GFM, código, citas, enlaces e imágenes.
 *
 * Todo el tipografiado vive en `.md` (globals.css). Las tablas y el código se
 * envuelven en un contenedor con scroll horizontal propio para que NUNCA
 * desborden la burbuja del chat (ver `.md-scroll`). Los bloques de código llevan
 * botón de copiar; las imágenes se amplían a pantalla completa (lightbox).
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

// Bloque de código con botón "copiar": extrae el texto crudo del <pre> y lo copia.
function CodeBlock({ children }: { children: React.ReactNode }) {
  const [copied, setCopied] = useState(false);
  async function copy(e: React.MouseEvent<HTMLButtonElement>) {
    const pre = e.currentTarget.parentElement?.querySelector("pre");
    const text = pre?.textContent ?? "";
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* sin permiso de portapapeles */
    }
  }
  return (
    <div className="md-codewrap" style={{ position: "relative" }}>
      <button
        type="button"
        onClick={copy}
        className="md-copy"
        title="Copiar código"
        aria-label="Copiar código"
      >
        {copied ? "✓ copiado" : "copiar"}
      </button>
      <div className="md-scroll">
        <pre>{children}</pre>
      </div>
    </div>
  );
}

export default function Markdown({ children, onCitation, isCitation }: Props) {
  const lightbox = useLightbox();
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
        urlTransform={safeUrl}
        components={{
          table: ({ node, ...props }) => (
            <div className="md-scroll">
              <table {...props} />
            </div>
          ),
          // El <pre> va dentro de CodeBlock (con botón copiar). react-markdown pasa el
          // <code> interno como children del <pre>; lo reusamos tal cual.
          pre: ({ node, children: preChildren }) => <CodeBlock>{preChildren}</CodeBlock>,
          img: ({ node, src, alt, ...props }) => (
            // eslint-disable-next-line @next/next/no-img-element
            <img
              {...props}
              src={typeof src === "string" ? src : ""}
              alt={alt ?? "imagen"}
              loading="lazy"
              style={{ cursor: "zoom-in" }}
              onClick={() => typeof src === "string" && src && lightbox.open(src, alt)}
              title="Ampliar"
            />
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
