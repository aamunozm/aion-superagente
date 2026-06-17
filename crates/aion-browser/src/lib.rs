//! # aion-browser
//!
//! Capacidad web del agente. F5 (actual): [`WebClient`] descarga una URL y extrae
//! su texto legible — el primitivo "leer la web". Incluye guardas anti-SSRF
//! (rechaza hosts internos/privados) y límite de tamaño.
//!
//! Evolución futura: navegación autónoma con DOM+visión (browser-use sidecar →
//! chromiumoxide/CDP) detrás de un trait `BrowserDriver`.

mod driver;
mod html;

pub use driver::{BrowserDriver, ChromiumoxideDriver, El, PageView, Snapshot};

use aion_kernel::{AionError, Result};
use std::time::Duration;

const MAX_CHARS: usize = 4000;

/// Cliente web del agente.
pub struct WebClient {
    http: reqwest::Client,
    max_chars: usize,
}

impl Default for WebClient {
    fn default() -> Self {
        Self::new()
    }
}

impl WebClient {
    pub fn new() -> Self {
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            // UA de navegador real: algunos sitios bloquean clientes desconocidos.
            .user_agent(
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 \
                 (KHTML, like Gecko) Version/17.0 Safari/605.1.15",
            );
        // PRIVACIDAD: si Ariel configura un proxy (Tor SOCKS5 o VPN/proxy), TODO el tráfico
        // web de AION sale por ahí → oculta la IP real. Ej.: AION_PROXY=socks5h://127.0.0.1:9050
        // (Tor) o http://user:pass@host:port. Sin esto, sale directo.
        if let Ok(p) = std::env::var("AION_PROXY") {
            if !p.trim().is_empty() {
                if let Ok(proxy) = reqwest::Proxy::all(p.trim()) {
                    builder = builder.proxy(proxy);
                }
            }
        }
        let http = builder.build().unwrap_or_default();
        Self {
            http,
            max_chars: MAX_CHARS,
        }
    }

    /// **Búsqueda web real** (DuckDuckGo HTML, sin API key). Devuelve resultados
    /// con título, URL y fragmento, para que el agente investigue en varias fuentes.
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        // MULTI-FUENTE: consulta varios motores EN PARALELO y FUSIONA (dedup por host),
        // en vez de "uno u otro". Así nunca depende de una sola fuente y diversifica.
        let q = query.trim();
        let (ddg, ia, wiki) = tokio::join!(
            self.search_ddg(q, limit),
            self.search_ddg_instant(q, limit),
            self.search_wikipedia(q, limit),
        );

        let mut out: Vec<SearchResult> = Vec::new();
        let mut seen_host: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut seen_url: std::collections::HashSet<String> = std::collections::HashSet::new();
        // Intercala varias fuentes; Wikipedia rellena al final.
        for r in ddg
            .unwrap_or_default()
            .into_iter()
            .chain(ia.unwrap_or_default())
            .chain(wiki.unwrap_or_default())
        {
            if !seen_url.insert(r.url.clone()) {
                continue; // misma URL exacta ya incluida
            }
            let host = host_of(&r.url);
            // Permite varias entradas pero limita duplicados del MISMO host (diversidad).
            let dup = seen_host.contains(&host) && host != "es.wikipedia.org";
            if dup {
                continue;
            }
            seen_host.insert(host);
            out.push(r);
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    /// DuckDuckGo HTML (POST). Devuelve vacío si bloquea (anomaly/captcha).
    async fn search_ddg(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let resp = self
            .http
            .post("https://html.duckduckgo.com/html/")
            .header("Accept-Language", "es-ES,es;q=0.9,en;q=0.8")
            .form(&[("q", query), ("kl", "wt-wt")])
            .send()
            .await
            .map_err(|e| AionError::Internal(format!("ddg falló: {e}")))?;
        let body = resp
            .text()
            .await
            .map_err(|e| AionError::Internal(format!("ddg cuerpo inválido: {e}")))?;
        Ok(parse_ddg_results(&body, limit))
    }

    /// DuckDuckGo LITE (POST). Endpoint minimalista, más estable y menos bloqueado que el html:
    /// 2ª fuente web para que la cobertura sobreviva si el html limita por carga.
    async fn search_ddg_lite(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let resp = self
            .http
            .post("https://lite.duckduckgo.com/lite/")
            .header("Accept-Language", "es-ES,es;q=0.9,en;q=0.8")
            .form(&[("q", query)])
            .send()
            .await
            .map_err(|e| AionError::Internal(format!("ddg-lite falló: {e}")))?;
        let body = resp
            .text()
            .await
            .map_err(|e| AionError::Internal(format!("ddg-lite cuerpo inválido: {e}")))?;
        Ok(parse_ddg_lite_results(&body, limit))
    }

    /// DuckDuckGo Instant Answer (API JSON). Endpoint distinto, menos propenso a
    /// bloqueo; da un resumen + temas relacionados que ENLAZAN a sitios reales
    /// (no solo Wikipedia). Diversifica las fuentes.
    async fn search_ddg_instant(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let url = format!(
            "https://api.duckduckgo.com/?q={}&format=json&no_html=1&skip_disambig=1",
            urlencode(query)
        );
        let json: serde_json::Value = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| AionError::Internal(format!("ddg-ia falló: {e}")))?
            .json()
            .await
            .map_err(|e| AionError::Internal(format!("ddg-ia json inválido: {e}")))?;
        let mut out = Vec::new();
        // Respuesta directa (Abstract) si la hay.
        let abstract_text = json["AbstractText"].as_str().unwrap_or("");
        let abstract_url = json["AbstractURL"].as_str().unwrap_or("");
        if !abstract_text.is_empty() && !abstract_url.is_empty() {
            out.push(SearchResult {
                title: json["Heading"].as_str().unwrap_or("Resumen").to_string(),
                url: abstract_url.to_string(),
                snippet: abstract_text.to_string(),
                source: "web".into(),
            });
        }
        // Temas relacionados (enlazan a sitios reales).
        if let Some(arr) = json["RelatedTopics"].as_array() {
            for it in arr.iter() {
                if out.len() >= limit {
                    break;
                }
                let (Some(text), Some(u)) = (it["Text"].as_str(), it["FirstURL"].as_str()) else {
                    continue;
                };
                if text.is_empty() || u.is_empty() {
                    continue;
                }
                out.push(SearchResult {
                    title: text.chars().take(80).collect(),
                    url: u.to_string(),
                    snippet: text.to_string(),
                    source: "web".into(),
                });
            }
        }
        Ok(out)
    }

    /// Búsqueda vía API de Wikipedia (es). Fuente fiable de respaldo: devuelve
    /// artículos reales con extracto y URL navegable.
    async fn search_wikipedia(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let q = urlencode(query.trim());
        let url = format!(
            "https://es.wikipedia.org/w/api.php?action=query&list=search&srsearch={q}\
             &format=json&srlimit={limit}&srprop=snippet"
        );
        let json: serde_json::Value = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| AionError::Internal(format!("búsqueda wiki falló: {e}")))?
            .json()
            .await
            .map_err(|e| AionError::Internal(format!("json wiki inválido: {e}")))?;
        let mut out = Vec::new();
        if let Some(arr) = json["query"]["search"].as_array() {
            for it in arr.iter().take(limit) {
                let title = it["title"].as_str().unwrap_or("").to_string();
                let snippet = strip_html_tags(it["snippet"].as_str().unwrap_or(""));
                let page = title.replace(' ', "_");
                out.push(SearchResult {
                    url: format!("https://es.wikipedia.org/wiki/{}", urlencode(&page)),
                    title,
                    snippet,
                    source: "enciclopedia".into(),
                });
            }
        }
        Ok(out)
    }

    /// **OpenAlex** — papers académicos revisados por pares (~250M works), sin clave.
    /// `mailto` entra al *polite pool* (más estable). Para rigor y estado del arte.
    async fn search_openalex(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let url = format!(
            "https://api.openalex.org/works?search={}&per_page={limit}\
             &mailto=info@prontoclick.it",
            urlencode(query)
        );
        let json = self.fetch_json(&url).await?;
        let mut out = Vec::new();
        if let Some(arr) = json["results"].as_array() {
            for w in arr.iter().take(limit) {
                let title = w["title"].as_str().unwrap_or("").to_string();
                // Prioriza el PDF/URL de ACCESO ABIERTO (legible: fetch_readable extrae PDF) sobre
                // la landing page del editor (suele ser de PAGO y sin texto). Así los papers se
                // leen de verdad en vez de rendir "NADA".
                let u = w["best_oa_location"]["pdf_url"]
                    .as_str()
                    .or_else(|| w["open_access"]["oa_url"].as_str())
                    .or_else(|| w["best_oa_location"]["landing_page_url"].as_str())
                    .or_else(|| w["primary_location"]["landing_page_url"].as_str())
                    .or_else(|| w["doi"].as_str())
                    .or_else(|| w["id"].as_str())
                    .unwrap_or("")
                    .to_string();
                if title.is_empty() || u.is_empty() {
                    continue;
                }
                let year = w["publication_year"]
                    .as_i64()
                    .map(|y| y.to_string())
                    .unwrap_or_default();
                let cites = w["cited_by_count"].as_i64().unwrap_or(0);
                // El ABSTRACT reconstruido va en el snippet: si el PDF no se pudiera leer, el
                // lector usa el abstract como respaldo (el paper contribuye igual).
                let abs = openalex_abstract(w);
                let snippet = if abs.chars().count() > 40 {
                    format!("Paper académico ({year}) · {cites} citas. {abs}")
                } else {
                    format!("Paper académico ({year}) · {cites} citas")
                };
                out.push(SearchResult {
                    title,
                    url: u,
                    snippet,
                    source: "académico".into(),
                });
            }
        }
        Ok(out)
    }

    /// **Crossref** — metadatos de ~150M obras académicas, SIN clave ni cuenta. El parámetro
    /// `mailto` entra al *polite pool* (más caudal). Fuente académica más fiable sin key (OpenAlex
    /// pasó a exigir clave en feb-2026). El abstract (cuando existe) viaja en el snippet → si el
    /// DOI lleva a un muro de pago, el lector usa el abstract igualmente.
    async fn search_crossref(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let url = format!(
            "https://api.crossref.org/works?query={}&rows={limit}&mailto=info@prontoclick.it",
            urlencode(query)
        );
        let json = self.fetch_json(&url).await?;
        let mut out = Vec::new();
        if let Some(arr) = json["message"]["items"].as_array() {
            for w in arr.iter().take(limit) {
                let title = w["title"][0].as_str().unwrap_or("").to_string();
                let doi = w["DOI"].as_str().unwrap_or("");
                if title.is_empty() || doi.is_empty() {
                    continue;
                }
                let year = w["issued"]["date-parts"][0][0]
                    .as_i64()
                    .map(|y| y.to_string())
                    .unwrap_or_default();
                let abs = w["abstract"]
                    .as_str()
                    .map(strip_html_tags)
                    .unwrap_or_default();
                let snippet = if abs.chars().count() > 40 {
                    format!(
                        "Paper académico ({year}). {}",
                        abs.chars().take(500).collect::<String>()
                    )
                } else {
                    format!("Paper académico ({year})")
                };
                out.push(SearchResult {
                    title,
                    url: format!("https://doi.org/{doi}"),
                    snippet,
                    source: "académico".into(),
                });
            }
        }
        Ok(out)
    }

    /// **Europe PMC** — literatura biomédica y de ciencias de la vida (~40M registros), SIN clave;
    /// muchos con full-text abierto. `abstractText` va al snippet como respaldo.
    async fn search_europepmc(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let url = format!(
            "https://www.ebi.ac.uk/europepmc/webservices/rest/search?query={}&format=json\
             &pageSize={limit}&resultType=core",
            urlencode(query)
        );
        let json = self.fetch_json(&url).await?;
        let mut out = Vec::new();
        if let Some(arr) = json["resultList"]["result"].as_array() {
            for r in arr.iter().take(limit) {
                let title = strip_html_tags(r["title"].as_str().unwrap_or(""));
                if title.is_empty() {
                    continue;
                }
                let doi = r["doi"].as_str().unwrap_or("");
                let id = r["id"].as_str().unwrap_or("");
                let src = r["source"].as_str().unwrap_or("MED");
                let url = if !doi.is_empty() {
                    format!("https://doi.org/{doi}")
                } else {
                    format!("https://europepmc.org/article/{src}/{id}")
                };
                let year = r["pubYear"].as_str().unwrap_or("");
                let abs = strip_html_tags(r["abstractText"].as_str().unwrap_or(""));
                let snippet = if abs.chars().count() > 40 {
                    format!(
                        "Paper ({year}). {}",
                        abs.chars().take(500).collect::<String>()
                    )
                } else {
                    format!("Paper ({year})")
                };
                out.push(SearchResult {
                    title,
                    url,
                    snippet,
                    source: "académico".into(),
                });
            }
        }
        Ok(out)
    }

    /// **arXiv** — preprints (CS, física, matemáticas…), SIN clave. Devuelve la URL del PDF (que
    /// `fetch_readable` SÍ extrae) y el abstract como respaldo. Respuesta en Atom XML (parseo simple).
    async fn search_arxiv(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let url = format!(
            "https://export.arxiv.org/api/query?search_query=all:{}&max_results={limit}\
             &sortBy=relevance",
            urlencode(query)
        );
        let body = self.fetch_raw(&url).await?;
        let mut out = Vec::new();
        for entry in body.split("<entry>").skip(1).take(limit) {
            let title = between(entry, "<title>", "</title>")
                .map(strip_html_tags)
                .unwrap_or_default();
            let summary = between(entry, "<summary>", "</summary>")
                .map(strip_html_tags)
                .unwrap_or_default();
            let id = between(entry, "<id>", "</id>").unwrap_or("").trim();
            if title.is_empty() || id.is_empty() {
                continue;
            }
            // id = .../abs/XXXX → el PDF está en .../pdf/XXXX (legible por fetch_readable).
            let pdf = id.replace("/abs/", "/pdf/");
            out.push(SearchResult {
                title,
                url: pdf,
                snippet: format!(
                    "Preprint arXiv. {}",
                    summary.chars().take(500).collect::<String>()
                ),
                source: "académico".into(),
            });
        }
        Ok(out)
    }

    /// **Open Library** (Internet Archive), SIN key: metadatos de libros (título, autor, año).
    /// Aporta diversidad tipo LIBRO/primaria; la página /works suele tener descripción legible.
    async fn search_openlibrary(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let url = format!(
            "https://openlibrary.org/search.json?q={}&limit={limit}\
             &fields=key,title,author_name,first_publish_year",
            urlencode(query)
        );
        let json = self.fetch_json(&url).await?;
        let mut out = Vec::new();
        if let Some(arr) = json["docs"].as_array() {
            for d in arr.iter().take(limit) {
                let title = d["title"].as_str().unwrap_or("").to_string();
                let key = d["key"].as_str().unwrap_or("");
                if title.is_empty() || key.is_empty() {
                    continue;
                }
                let author = d["author_name"][0].as_str().unwrap_or("");
                let year = d["first_publish_year"]
                    .as_i64()
                    .map(|y| y.to_string())
                    .unwrap_or_default();
                out.push(SearchResult {
                    title,
                    url: format!("https://openlibrary.org{key}"),
                    snippet: format!("Libro · {author} · {year}"),
                    source: "libro".into(),
                });
            }
        }
        Ok(out)
    }

    /// **Hacker News** (Algolia) — discusiones de profesionales tech (foros).
    async fn search_hackernews(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let url = format!(
            "https://hn.algolia.com/api/v1/search?query={}&tags=story&hitsPerPage={limit}",
            urlencode(query)
        );
        let json = self.fetch_json(&url).await?;
        let mut out = Vec::new();
        if let Some(arr) = json["hits"].as_array() {
            for h in arr.iter().take(limit) {
                let title = h["title"].as_str().unwrap_or("").to_string();
                if title.is_empty() {
                    continue;
                }
                let id = h["objectID"].as_str().unwrap_or("");
                let u = h["url"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("https://news.ycombinator.com/item?id={id}"));
                let pts = h["points"].as_i64().unwrap_or(0);
                let nc = h["num_comments"].as_i64().unwrap_or(0);
                out.push(SearchResult {
                    title,
                    url: u,
                    snippet: format!("Hacker News · {pts} puntos · {nc} comentarios"),
                    source: "foro".into(),
                });
            }
        }
        Ok(out)
    }

    /// **Stack Exchange** (Stack Overflow) — Q&A técnico. La API responde gzip.
    async fn search_stackexchange(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let url = format!(
            "https://api.stackexchange.com/2.3/search/advanced?q={}&site=stackoverflow\
             &order=desc&sort=relevance&pagesize={limit}",
            urlencode(query)
        );
        let json = self.fetch_json(&url).await?;
        let mut out = Vec::new();
        if let Some(arr) = json["items"].as_array() {
            for it in arr.iter().take(limit) {
                let title = strip_html_tags(it["title"].as_str().unwrap_or(""));
                let u = it["link"].as_str().unwrap_or("").to_string();
                if title.is_empty() || u.is_empty() {
                    continue;
                }
                let score = it["score"].as_i64().unwrap_or(0);
                let answered = it["is_answered"].as_bool().unwrap_or(false);
                out.push(SearchResult {
                    title,
                    url: u,
                    snippet: format!(
                        "Stack Overflow · score {score}{}",
                        if answered { " · resuelto" } else { "" }
                    ),
                    source: "foro".into(),
                });
            }
        }
        Ok(out)
    }

    /// **GitHub** — repos relevantes (proyectos/herramientas reales), por estrellas. Sin clave.
    async fn search_github(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let url = format!(
            "https://api.github.com/search/repositories?q={}&sort=stars&order=desc&per_page={limit}",
            urlencode(query)
        );
        // Token opcional (Ajustes → APIs): sin él, la búsqueda de repos rate-limita (10/min) y
        // a veces vuelve vacía; con él sube el límite y habilita más. Se lee del entorno (lo publica
        // apikeys::init_env), igual que AION_PROXY → sin acoplar este crate a la config de aion-core.
        let mut req = self
            .http
            .get(&url)
            .header("Accept", "application/vnd.github+json");
        if let Ok(tok) = std::env::var("AION_GITHUB_TOKEN") {
            if !tok.trim().is_empty() {
                req = req.header("Authorization", format!("Bearer {}", tok.trim()));
            }
        }
        let json: serde_json::Value = req
            .send()
            .await
            .map_err(|e| AionError::Internal(format!("github falló: {e}")))?
            .json()
            .await
            .map_err(|e| AionError::Internal(format!("github json inválido: {e}")))?;
        let mut out = Vec::new();
        if let Some(arr) = json["items"].as_array() {
            for r in arr.iter().take(limit) {
                let title = r["full_name"].as_str().unwrap_or("").to_string();
                let u = r["html_url"].as_str().unwrap_or("").to_string();
                if title.is_empty() || u.is_empty() {
                    continue;
                }
                let desc = r["description"].as_str().unwrap_or("(sin descripción)");
                let stars = r["stargazers_count"].as_i64().unwrap_or(0);
                out.push(SearchResult {
                    title,
                    url: u,
                    snippet: format!("GitHub ⭐{stars} · {desc}"),
                    source: "código".into(),
                });
            }
        }
        Ok(out)
    }

    /// **GitHub code search** — busca dentro del CÓDIGO de repos públicos (ficheros, no solo el
    /// repo). GitHub lo exige AUTENTICADO desde 2023: solo se activa si hay `AION_GITHUB_TOKEN`
    /// (Ajustes → APIs); sin token devuelve vacío (keyless-safe). Apunta al RAW del fichero, que
    /// `fetch_readable` lee como texto plano.
    async fn search_github_code(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let tok = std::env::var("AION_GITHUB_TOKEN").unwrap_or_default();
        if tok.trim().is_empty() {
            return Ok(Vec::new()); // sin token, GitHub code-search no es viable
        }
        let url = format!(
            "https://api.github.com/search/code?q={}&per_page={limit}",
            urlencode(query)
        );
        let json: serde_json::Value = self
            .http
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .header("Authorization", format!("Bearer {}", tok.trim()))
            .send()
            .await
            .map_err(|e| AionError::Internal(format!("github code falló: {e}")))?
            .json()
            .await
            .map_err(|e| AionError::Internal(format!("github code json inválido: {e}")))?;
        let mut out = Vec::new();
        if let Some(arr) = json["items"].as_array() {
            for it in arr.iter().take(limit) {
                let repo = it["repository"]["full_name"].as_str().unwrap_or("");
                let path = it["path"].as_str().unwrap_or("");
                let html = it["html_url"].as_str().unwrap_or("");
                if repo.is_empty() || path.is_empty() || html.is_empty() {
                    continue;
                }
                // El blob HTML es JS-pesado → apunta al RAW (texto plano legible):
                // github.com/{repo}/blob/{ref}/{path} → raw.githubusercontent.com/{repo}/{ref}/{path}
                let raw = html
                    .replacen(
                        "https://github.com/",
                        "https://raw.githubusercontent.com/",
                        1,
                    )
                    .replacen("/blob/", "/", 1);
                out.push(SearchResult {
                    title: format!("{repo}/{path}"),
                    url: raw,
                    snippet: format!("Código GitHub · {repo} · {path}"),
                    source: "código".into(),
                });
            }
        }
        Ok(out)
    }

    /// **Búsqueda en GitHub para el AGENTE**: repositorios (por estrellas) + ficheros de CÓDIGO
    /// (estos solo si hay `AION_GITHUB_TOKEN`). Combina ambas. Pública para que la herramienta
    /// `github_search` del agente la use cuando el usuario pida "buscar en GitHub".
    pub async fn github(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        let (repos, code) = tokio::join!(
            self.search_github(query, limit),
            self.search_github_code(query, limit),
        );
        let mut out = repos.unwrap_or_default();
        out.extend(code.unwrap_or_default());
        out
    }

    /// Búsqueda acotada a un DOMINIO vía DDG (`site:`), para fuentes que bloquean su API
    /// directa (Reddit, YouTube). Reusa el parser de DDG; `label` marca la familia.
    async fn search_site(
        &self,
        query: &str,
        site: &str,
        label: &str,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        let mut rs = self
            .search_ddg(&format!("{query} site:{site}"), limit)
            .await?;
        for r in rs.iter_mut() {
            r.source = label.to_string();
        }
        Ok(rs)
    }

    /// **Búsqueda PROFUNDA multi-fuente** (investigación profesional). Consulta MUCHAS fuentes
    /// diversas y creíbles EN PARALELO —web general, académico (OpenAlex), foros (Hacker News,
    /// Stack Exchange, Reddit), código (GitHub), vídeo (YouTube)— SIN depender de Wikipedia, y
    /// devuelve hasta `limit` URLs únicas, diversificadas por dominio (máx. 3/host) y etiquetadas
    /// por familia. Fail-soft: una fuente caída/lenta no vacía el resultado.
    pub async fn search_deep(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        let per = 6usize;
        // Las 4 APIs independientes (cada una en su propio host) + DDG-LITE (lite.duckduckgo.com)
        // van EN PARALELO sin problema. PERO la familia DDG-HTML —búsqueda web + site:reddit +
        // site:youtube— golpea el MISMO host (html.duckduckgo.com); lanzarlas a la vez dispara el
        // anomaly-block de DDG (la misma fragilidad por la que el caller ya ESCALONA los ángulos)
        // y, cuando bloquea, perdemos web+foro+vídeo de golpe. Por eso ese trío va EN SERIE, a la
        // vez que el resto. La búsqueda no es el cuello de botella (lo es la LECTURA posterior).
        let html_ddg = async {
            let ddg = self.search_ddg(query, per * 2).await;
            let rd = self.search_site(query, "reddit.com", "foro", per).await;
            let yt = self.search_site(query, "youtube.com", "vídeo", per).await;
            (ddg, rd, yt)
        };
        let others = async {
            tokio::join!(
                self.search_ddg_lite(query, per * 2),
                self.search_crossref(query, per),
                self.search_arxiv(query, per),
                self.search_europepmc(query, per),
                self.search_openlibrary(query, per),
                self.search_openalex(query, per),
                self.search_hackernews(query, per),
                self.search_stackexchange(query, per),
                self.search_github(query, per),
                self.search_github_code(query, per),
            )
        };
        let ((ddg, rd, yt), (lite, cr, ax, ep, ol, oa, hn, se, gh, ghc)) =
            tokio::join!(html_ddg, others);
        let mut out: Vec<SearchResult> = Vec::new();
        let mut seen_url: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut host_count: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        // ROUND-ROBIN entre fuentes: toma 1 de cada una por turno (web, académico, foro, código,
        // vídeo…) en vez de encadenar (que dejaba que DDG llenara el cupo antes de llegar a las
        // demás). Así el resultado es DIVERSO de verdad, no dominado por un solo motor. DDG html y
        // lite van como dos fuentes web: si una se limita por carga, la otra mantiene la cobertura.
        let pools: Vec<Vec<SearchResult>> = vec![
            ddg.unwrap_or_default(),
            cr.unwrap_or_default(),
            hn.unwrap_or_default(),
            ax.unwrap_or_default(),
            se.unwrap_or_default(),
            ep.unwrap_or_default(),
            ol.unwrap_or_default(),
            gh.unwrap_or_default(),
            ghc.unwrap_or_default(),
            rd.unwrap_or_default(),
            oa.unwrap_or_default(),
            yt.unwrap_or_default(),
            lite.unwrap_or_default(),
        ];
        let depth = pools.iter().map(|p| p.len()).max().unwrap_or(0);
        'fill: for col in 0..depth {
            for pool in &pools {
                let Some(r) = pool.get(col) else {
                    continue;
                };
                if r.url.is_empty() || !r.url.starts_with("http") {
                    continue;
                }
                if !seen_url.insert(r.url.clone()) {
                    continue;
                }
                let host = host_of(&r.url);
                // Agregadores académicos (doi.org, arXiv, Europe PMC…): cada URL es un paper
                // distinto aunque compartan host → exentos del tope de 3/dominio.
                let exempt = matches!(
                    host.as_str(),
                    "doi.org"
                        | "arxiv.org"
                        | "www.arxiv.org"
                        | "export.arxiv.org"
                        | "europepmc.org"
                        | "www.ncbi.nlm.nih.gov"
                        | "openalex.org"
                        | "raw.githubusercontent.com"
                );
                let c = host_count.entry(host).or_insert(0);
                if !exempt && *c >= 3 {
                    continue; // máx 3 por dominio: evita que un sitio domine
                }
                *c += 1;
                out.push(r.clone());
                if out.len() >= limit {
                    break 'fill;
                }
            }
        }
        out
    }

    /// **Clima EN TIEMPO REAL de un lugar** vía Open-Meteo (sin API key): geocodifica
    /// la ciudad y devuelve la observación actual (temperatura, sensación, cielo,
    /// humedad, viento). Es la fuente correcta para "¿qué temperatura hace?": la
    /// búsqueda web general solo devuelve artículos, nunca el dato del momento.
    pub async fn weather(&self, place: &str) -> Result<String> {
        let q = urlencode(place.trim());
        let geo: serde_json::Value = self
            .http
            .get(format!(
                "https://geocoding-api.open-meteo.com/v1/search?name={q}&count=1\
                 &language=es&format=json"
            ))
            .send()
            .await
            .map_err(|e| AionError::Internal(format!("geocoding falló: {e}")))?
            .json()
            .await
            .map_err(|e| AionError::Internal(format!("geocoding json inválido: {e}")))?;
        let Some(hit) = geo["results"].as_array().and_then(|a| a.first()) else {
            return Err(AionError::Internal(format!(
                "no encontré el lugar «{}» en el mapa",
                place.trim()
            )));
        };
        let (Some(lat), Some(lon)) = (hit["latitude"].as_f64(), hit["longitude"].as_f64()) else {
            return Err(AionError::Internal("lugar sin coordenadas".into()));
        };
        let name = hit["name"].as_str().unwrap_or(place.trim());
        let country = hit["country"].as_str().unwrap_or("");
        let label = if country.is_empty() {
            name.to_string()
        } else {
            format!("{name} ({country})")
        };
        self.forecast_at(lat, lon, &label).await
    }

    /// **Clima SIN ciudad — autonomía**: geolocaliza el equipo por su IP pública y
    /// consulta el clima ahí. Permite responder «¿qué temperatura hace?» sin pedirle
    /// la ciudad al usuario.
    pub async fn weather_auto(&self) -> Result<String> {
        let (lat, lon, label) = self.geolocate().await?;
        let r = self.forecast_at(lat, lon, &label).await?;
        Ok(format!("{r} [ubicación estimada por la IP del equipo]"))
    }

    /// **Clima en coordenadas EXACTAS** (las que el usuario fijó en «Conciencia de
    /// entorno»). Más preciso que `weather_auto` —que estima por IP— y CORRECTO detrás
    /// de un proxy/VPN, donde la IP apunta al nodo de salida y no a ti.
    pub async fn weather_at(&self, lat: f64, lon: f64, label: &str) -> Result<String> {
        self.forecast_at(lat, lon, label).await
    }

    /// Ubicación aproximada del equipo por su IP pública → (lat, lon, etiqueta
    /// «Ciudad (País)»). Precisión a nivel de ciudad. Dos proveedores HTTPS sin API
    /// key: ipwho.is (principal) e ipinfo.io (respaldo) — ipapi.co quedó descartado
    /// por rate-limit agresivo. OJO: detrás de AION_PROXY (Tor/VPN) la IP es la del
    /// nodo de salida — la ubicación será la del túnel, no la real del equipo.
    pub async fn geolocate(&self) -> Result<(f64, f64, String)> {
        if let Ok(json) = self.fetch_json("https://ipwho.is/").await {
            if json["success"].as_bool().unwrap_or(false) {
                if let (Some(lat), Some(lon)) =
                    (json["latitude"].as_f64(), json["longitude"].as_f64())
                {
                    let city = json["city"].as_str().unwrap_or("");
                    let country = json["country"].as_str().unwrap_or("");
                    return Ok((lat, lon, place_label(city, country)));
                }
            }
        }
        // Respaldo: ipinfo.io entrega las coordenadas como "lat,lon" en `loc`.
        let json = self.fetch_json("https://ipinfo.io/json").await?;
        let mut parts = json["loc"].as_str().unwrap_or("").split(',');
        let (Some(lat), Some(lon)) = (
            parts.next().and_then(|s| s.trim().parse::<f64>().ok()),
            parts.next().and_then(|s| s.trim().parse::<f64>().ok()),
        ) else {
            return Err(AionError::Internal(
                "la geolocalización por IP no devolvió coordenadas".into(),
            ));
        };
        let city = json["city"].as_str().unwrap_or("");
        let country = json["country"].as_str().unwrap_or("");
        Ok((lat, lon, place_label(city, country)))
    }

    /// GET → JSON con errores legibles (para las APIs públicas sin key).
    async fn fetch_json(&self, url: &str) -> Result<serde_json::Value> {
        self.http
            .get(url)
            .send()
            .await
            .map_err(|e| AionError::Internal(format!("{url} falló: {e}")))?
            .json()
            .await
            .map_err(|e| AionError::Internal(format!("{url} json inválido: {e}")))
    }

    /// Observación actual de Open-Meteo en unas coordenadas, formateada en español.
    async fn forecast_at(&self, lat: f64, lon: f64, label: &str) -> Result<String> {
        let wx: serde_json::Value = self
            .http
            .get(format!(
                "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}\
                 &current=temperature_2m,apparent_temperature,relative_humidity_2m,\
                 weather_code,wind_speed_10m&timezone=auto"
            ))
            .send()
            .await
            .map_err(|e| AionError::Internal(format!("open-meteo falló: {e}")))?
            .json()
            .await
            .map_err(|e| AionError::Internal(format!("open-meteo json inválido: {e}")))?;
        let cur = &wx["current"];
        let Some(temp) = cur["temperature_2m"].as_f64() else {
            return Err(AionError::Internal(
                "open-meteo no devolvió temperatura".into(),
            ));
        };
        let feels = cur["apparent_temperature"].as_f64().unwrap_or(temp);
        let hum = cur["relative_humidity_2m"].as_f64().unwrap_or(0.0);
        let wind = cur["wind_speed_10m"].as_f64().unwrap_or(0.0);
        let desc = weather_desc(cur["weather_code"].as_u64().unwrap_or(u64::MAX));
        let when = cur["time"].as_str().unwrap_or("");
        Ok(format!(
            "Ahora en {label}: {temp:.0} °C (sensación {feels:.0} °C), {desc}, \
             humedad {hum:.0}%, viento {wind:.0} km/h{}.",
            if when.is_empty() {
                String::new()
            } else {
                format!(" — medido a las {}", &when[when.len().saturating_sub(5)..])
            }
        ))
    }

    /// **Búsqueda de LUGARES/NEGOCIOS por dirección** vía OpenStreetMap (Nominatim,
    /// sin API key). Ideal para "¿qué negocio hay en tal dirección?", coordenadas,
    /// tipo de local (restaurante, tienda…). Más fiable que la búsqueda web general
    /// para direcciones. Devuelve nombre, categoría y dirección completa.
    pub async fn search_place(&self, query: &str, limit: usize) -> Result<Vec<PlaceResult>> {
        let q = urlencode(query.trim());
        let url = format!(
            "https://nominatim.openstreetmap.org/search?q={q}&format=jsonv2\
             &addressdetails=1&extratags=1&namedetails=1&limit={limit}"
        );
        let json: serde_json::Value = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| AionError::Internal(format!("búsqueda de lugar falló: {e}")))?
            .json()
            .await
            .map_err(|e| AionError::Internal(format!("json de lugar inválido: {e}")))?;
        let mut out = Vec::new();
        if let Some(arr) = json.as_array() {
            for it in arr.iter().take(limit) {
                let name = it["name"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .or_else(|| it["namedetails"]["name"].as_str())
                    .unwrap_or("")
                    .to_string();
                // Categoría legible: tipo concreto (restaurant, supermarket…) +
                // clase (amenity/shop/office) o las extratags relevantes.
                let et = &it["extratags"];
                let kind = it["type"]
                    .as_str()
                    .filter(|s| !s.is_empty() && *s != "yes")
                    .or_else(|| et["shop"].as_str())
                    .or_else(|| et["amenity"].as_str())
                    .or_else(|| et["office"].as_str())
                    .or_else(|| it["category"].as_str())
                    .unwrap_or("lugar")
                    .to_string();
                let address = it["display_name"].as_str().unwrap_or("").to_string();
                out.push(PlaceResult {
                    name,
                    kind,
                    address,
                });
            }
        }
        Ok(out)
    }

    /// Descarga una URL y devuelve su texto legible (HTML→texto), truncado.
    pub async fn fetch_text(&self, url: &str) -> Result<String> {
        let url = url.trim();
        guard_url(url)?;
        let resp = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| AionError::Internal(format!("fetch falló: {e}")))?;
        if !resp.status().is_success() {
            return Err(AionError::Internal(format!("HTTP {}", resp.status())));
        }
        let body = resp
            .text()
            .await
            .map_err(|e| AionError::Internal(format!("cuerpo inválido: {e}")))?;
        let mut text = html::to_text(&body);
        // Truncar por CARACTERES, no por bytes: `String::truncate(n)` ENTRA EN PÁNICO si el byte
        // n cae en mitad de un carácter UTF-8 (acentos, emojis…), y el texto web está lleno de
        // ellos → web_fetch crasheaba en buena parte de las páginas. take(n) sobre chars es seguro.
        if text.chars().count() > self.max_chars {
            text = text.chars().take(self.max_chars).collect();
            text.push_str(" …[truncado]");
        }
        Ok(text)
    }

    /// **Lectura PROFUNDA de una fuente** (para investigación): como `fetch_text` pero con un
    /// presupuesto de caracteres configurable (más alto) y SOPORTE DE PDF — muchas fuentes
    /// académicas son PDFs y `fetch_text` solo veía basura binaria. Detecta PDF por content-type
    /// o por la cabecera mágica `%PDF` y extrae su texto. Recorta por CARACTERES (no bytes: evita
    /// pánico en mitad de un UTF-8). Mantiene guard anti-SSRF + proxy.
    pub async fn fetch_readable(&self, url: &str, max_chars: usize) -> Result<String> {
        let url = url.trim();
        guard_url(url)?;
        let resp = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| AionError::Internal(format!("fetch falló: {e}")))?;
        if !resp.status().is_success() {
            return Err(AionError::Internal(format!("HTTP {}", resp.status())));
        }
        let ctype = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();
        let looks_pdf = ctype.contains("application/pdf") || url.to_lowercase().ends_with(".pdf");
        let mut text = if looks_pdf {
            let bytes = resp
                .bytes()
                .await
                .map_err(|e| AionError::Internal(format!("cuerpo inválido: {e}")))?;
            extract_pdf(bytes.to_vec()).await.unwrap_or_default()
        } else {
            let body = resp
                .text()
                .await
                .map_err(|e| AionError::Internal(format!("cuerpo inválido: {e}")))?;
            // Content-type a veces miente: si el cuerpo empieza por %PDF, trátalo como PDF.
            if body.as_bytes().starts_with(b"%PDF") {
                extract_pdf(body.into_bytes()).await.unwrap_or_default()
            } else {
                html::to_text(&body)
            }
        };
        if text.chars().count() > max_chars {
            text = text.chars().take(max_chars).collect();
            text.push_str(" …[truncado]");
        }
        Ok(text)
    }

    /// **Lectura RENDERIZADA** con el navegador headless propio (Chromium vía CDP): para páginas
    /// JS/SPA o con muro de verificación que el fetch estático no puede leer (Reddit, YouTube…).
    /// Devuelve el texto VISIBLE (innerText) tras renderizar. Más cara (~1-3 s + memoria) y
    /// SERIALIZADA por el navegador singleton, por eso el caller la reserva para hosts de bajo
    /// rendimiento estático. Con TIMEOUT para no colgar el pipeline. Requiere Chrome instalado (o
    /// `AION_CHROME`); si no, error → el caller cae al snippet. Mantiene el guard anti-SSRF.
    pub async fn fetch_rendered(&self, url: &str, max_chars: usize) -> Result<String> {
        guard_url(url)?;
        let driver = ChromiumoxideDriver;
        let view = tokio::time::timeout(Duration::from_secs(20), driver.open(url))
            .await
            .map_err(|_| AionError::Internal("render headless: timeout".into()))??;
        let mut text = view.text;
        if text.chars().count() > max_chars {
            text = text.chars().take(max_chars).collect();
            text.push_str(" …[truncado]");
        }
        Ok(text)
    }

    /// **Lectura desde Wayback (Internet Archive), SIN key**: rescata páginas que fallan en
    /// directo (paywall, 403/404, retiradas, anti-bot) leyendo su copia archivada más reciente.
    /// Coherente con el modo local-first/privado (no requiere cuenta). Error si no hay snapshot.
    pub async fn fetch_archived(&self, url: &str, max_chars: usize) -> Result<String> {
        let api = format!(
            "https://archive.org/wayback/available?url={}",
            urlencode(url)
        );
        let json = self.fetch_json(&api).await?;
        let snap = json["archived_snapshots"]["closest"]["url"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| AionError::Internal("sin snapshot en Wayback".into()))?
            .to_string();
        // El snapshot es HTML normal (con la barra de Wayback, que to_text descarta como chrome);
        // reusa el extractor estándar de lectura.
        self.fetch_readable(&snap, max_chars).await
    }

    /// Cuerpo CRUDO (sin pasar por el extractor de texto): para APIs JSON. Mantiene
    /// el guard anti-SSRF y el proxy (`AION_PROXY`) como el resto del cliente.
    pub async fn fetch_raw(&self, url: &str) -> Result<String> {
        let url = url.trim();
        guard_url(url)?;
        let resp = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| AionError::Internal(format!("fetch falló: {e}")))?;
        if !resp.status().is_success() {
            return Err(AionError::Internal(format!("HTTP {}", resp.status())));
        }
        resp.text()
            .await
            .map_err(|e| AionError::Internal(format!("cuerpo inválido: {e}")))
    }
}

/// Guarda anti-SSRF: solo http(s) y rechaza hosts internos/privados.
pub(crate) fn guard_url(url: &str) -> Result<()> {
    let lower = url.to_lowercase();
    if !lower.starts_with("http://") && !lower.starts_with("https://") {
        return Err(AionError::PolicyDenied(
            "solo se permiten URLs http(s)".into(),
        ));
    }
    let host = lower
        .split("://")
        .nth(1)
        .unwrap_or("")
        .split(['/', ':', '?', '#'])
        .next()
        .unwrap_or("");
    let blocked_prefix = ["127.", "10.", "192.168.", "169.254."];
    if host.is_empty()
        || host == "localhost"
        || host == "0.0.0.0"
        || host == "::1"
        || blocked_prefix.iter().any(|b| host.starts_with(b))
        || is_private_172(host)
    {
        return Err(AionError::PolicyDenied(format!(
            "host bloqueado por política anti-SSRF: {host}"
        )));
    }
    Ok(())
}

fn is_private_172(host: &str) -> bool {
    if let Some(rest) = host.strip_prefix("172.") {
        if let Some(octet) = rest.split('.').next() {
            if let Ok(n) = octet.parse::<u8>() {
                return (16..=31).contains(&n);
            }
        }
    }
    false
}

/// Un resultado de búsqueda web.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    /// Familia de fuente de la que vino ("web", "académico", "foro", "código", "vídeo"…).
    /// Permite al informe profesional citar y ponderar por tipo de fuente.
    pub source: String,
}

/// Un lugar/negocio encontrado por dirección (OpenStreetMap).
#[derive(Debug, Clone)]
pub struct PlaceResult {
    pub name: String,
    pub kind: String,
    pub address: String,
}

/// Parsea los resultados del HTML de DuckDuckGo (clases result__a / result__snippet).
/// La URL puede venir DIRECTA (`href="https://..."`, formato 2024+) o como redirección
/// `...uddg=<url codificada>` (formato antiguo); se soportan ambos.
fn parse_ddg_results(html: &str, limit: usize) -> Vec<SearchResult> {
    let mut out = Vec::new();
    for block in html.split("result__a").skip(1) {
        // El href de DDG cambió de formato (2024+): antes era un REDIRECT
        // `//duckduckgo.com/l/?uddg=ENCODED&rut=...`; ahora suele ser la URL DIRECTA
        // (`href="https://sitio.com/..."`). Soportamos AMBOS: tomamos el href y, si lleva
        // `uddg=`, lo decodificamos; si no, es ya la URL real. (Sin esto, el parser buscaba
        // `uddg=`, no lo encontraba y descartaba TODOS los resultados → búsqueda web vacía.)
        let href = block
            .find("href=\"")
            .map(|i| &block[i + 6..])
            .and_then(|s| s.split('"').next())
            .unwrap_or("");
        let url = if let Some(i) = href.find("uddg=") {
            percent_decode(href[i + 5..].split('&').next().unwrap_or(""))
        } else if let Some(rest) = href.strip_prefix("//") {
            // Protocolo-relativo (//host/...): antepón https para que pase el filtro `http`.
            format!("https://{rest}")
        } else {
            // href directo: ya viene con https://.
            href.to_string()
        };
        // título: texto entre el primer '>' y '</a>'
        let title = block
            .find('>')
            .map(|i| &block[i + 1..])
            .and_then(|s| s.split("</a>").next())
            .map(strip_html_tags)
            .unwrap_or_default();
        // fragmento: tras result__snippet
        let snippet = block
            .find("result__snippet")
            .map(|i| &block[i..])
            .and_then(|s| s.find('>').map(|j| &s[j + 1..]))
            .and_then(|s| s.split("</a>").next())
            .map(strip_html_tags)
            .unwrap_or_default();
        // Excluye enlaces internos de DuckDuckGo (anuncios, ajustes, redirecciones de tracking):
        // no son resultados reales y desperdiciarían un hueco de lectura. Coherente con DDG Lite.
        if !url.is_empty() && url.starts_with("http") && !url.contains("duckduckgo.com") {
            out.push(SearchResult {
                title: title.trim().to_string(),
                url,
                snippet: snippet.trim().to_string(),
                source: "web".into(),
            });
            if out.len() >= limit {
                break;
            }
        }
    }
    out
}

fn strip_html_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    // Decodifica las entidades HTML más comunes en títulos/snippets de DDG y Wikipedia. Antes solo
    // se cubrían &amp; &#x27; &quot;, así que un literal «&#39;» (apóstrofo decimal) o «&lt;» se
    // colaba en las notas que lee el LLM y en la bibliografía del informe. &amp; se procesa AL
    // FINAL para no re-expandir un «&amp;lt;» en «<».
    out.replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", "\u{a0}")
        .replace("&amp;", "&")
        .trim()
        .to_string()
}

/// Etiqueta legible «Ciudad (País)» para la geolocalización (best-effort).
fn place_label(city: &str, country: &str) -> String {
    match (city.is_empty(), country.is_empty()) {
        (false, false) => format!("{city} ({country})"),
        (false, true) => city.to_string(),
        _ => "tu zona".to_string(),
    }
}

/// Descripción en español de un código de tiempo WMO (los que devuelve Open-Meteo).
fn weather_desc(code: u64) -> &'static str {
    match code {
        0 => "despejado",
        1 | 2 => "parcialmente nublado",
        3 => "nublado",
        45 | 48 => "niebla",
        51..=57 => "llovizna",
        61..=67 => "lluvia",
        71..=77 => "nieve",
        80..=82 => "chubascos",
        85 | 86 => "chubascos de nieve",
        95..=99 => "tormenta",
        _ => "condiciones variables",
    }
}

/// Reconstruye el abstract de un work de OpenAlex desde su `abstract_inverted_index`
/// ({palabra: [posiciones]}). Vacío si no lo trae. Sirve de respaldo cuando el PDF no se lee.
fn openalex_abstract(w: &serde_json::Value) -> String {
    let Some(inv) = w["abstract_inverted_index"].as_object() else {
        return String::new();
    };
    let mut words: Vec<(usize, &str)> = Vec::new();
    for (word, poss) in inv {
        if let Some(arr) = poss.as_array() {
            for p in arr {
                if let Some(i) = p.as_u64() {
                    words.push((i as usize, word.as_str()));
                }
            }
        }
    }
    words.sort_by_key(|(i, _)| *i);
    words
        .into_iter()
        .map(|(_, w)| w)
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(600)
        .collect()
}

/// Parsea los resultados de DuckDuckGo LITE (lite.duckduckgo.com): tabla simple con anclas
/// `result-link`. Más estable y menos bloqueada que el endpoint html — buena 2ª fuente web.
fn parse_ddg_lite_results(html: &str, limit: usize) -> Vec<SearchResult> {
    let mut out = Vec::new();
    for block in html.split("result-link").skip(1) {
        let href = block
            .find("href=\"")
            .map(|i| &block[i + 6..])
            .and_then(|s| s.split('"').next())
            .unwrap_or("");
        let url = if let Some(i) = href.find("uddg=") {
            percent_decode(href[i + 5..].split('&').next().unwrap_or(""))
        } else if let Some(rest) = href.strip_prefix("//") {
            format!("https://{rest}")
        } else {
            href.to_string()
        };
        let title = block
            .find('>')
            .map(|i| &block[i + 1..])
            .and_then(|s| s.split("</a>").next())
            .map(strip_html_tags)
            .unwrap_or_default();
        if url.starts_with("http") && !url.contains("duckduckgo.com") {
            out.push(SearchResult {
                title: title.trim().to_string(),
                url,
                snippet: String::new(),
                source: "web".into(),
            });
            if out.len() >= limit {
                break;
            }
        }
    }
    out
}

/// Extrae el texto de un PDF (bytes en memoria). En hilo BLOQUEANTE (parseo CPU-bound) y bajo
/// `catch_unwind`: `pdf-extract` puede entrar en pánico con PDFs malformados, y eso JAMÁS debe
/// tumbar al agente. `None` si no se pudo extraer (la investigación sigue con las demás fuentes).
async fn extract_pdf(bytes: Vec<u8>) -> Option<String> {
    tokio::task::spawn_blocking(move || {
        std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(&bytes).ok())
            .ok()
            .flatten()
            .filter(|t| t.trim().chars().count() >= 40)
    })
    .await
    .ok()
    .flatten()
}

/// Codifica una cadena para usarla en una query string (percent-encoding).
/// Extrae el host de una URL (para deduplicar por dominio). Best-effort, sin deps.
/// Devuelve el texto entre `start` y `end` (primera ocurrencia). Para parsear XML/HTML simple.
fn between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let i = s.find(start)? + start.len();
    let j = s[i..].find(end)? + i;
    Some(&s[i..j])
}

fn host_of(url: &str) -> String {
    let after = url.split("://").nth(1).unwrap_or(url);
    after
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .to_lowercase()
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Decodifica percent-encoding (%XX y '+').
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                if let (Some(h), Some(l)) = (hi, lo) {
                    out.push((h * 16 + l) as u8);
                    i += 3;
                    continue;
                }
                out.push(b'%');
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ddg_direct_href_format() {
        // Formato NUEVO de DDG (2024+): href DIRECTO, sin redirect uddg=. Antes el parser
        // buscaba uddg= y devolvía VACÍO → búsqueda web rota para casi toda consulta técnica.
        let html = r#"<a class="result__a" href="https://vectorize.io/articles/best-ai-agent-memory-systems">Best AI Agent Memory Systems</a>
            <a class="result__snippet" href="x">An overview of memory systems for agents.</a>"#;
        let r = parse_ddg_results(html, 5);
        assert_eq!(r.len(), 1);
        assert_eq!(
            r[0].url,
            "https://vectorize.io/articles/best-ai-agent-memory-systems"
        );
        assert!(r[0].title.contains("Memory"));
    }

    #[test]
    fn parses_ddg_legacy_uddg_redirect() {
        // Formato ANTIGUO (redirect): debe seguir decodificando uddg= por retrocompatibilidad.
        let html = r#"<a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fx&rut=ab">Ejemplo</a>"#;
        let r = parse_ddg_results(html, 5);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].url, "https://example.com/x");
    }

    #[test]
    fn decodes_common_html_entities_in_text() {
        // Títulos y snippets de DDG/Wikipedia llegan con entidades HTML. Si no se decodifican,
        // el texto literal «&#39;» / «&lt;» se cuela en las notas que lee el LLM y en la
        // bibliografía del informe. Antes solo se decodificaban &amp; &#x27; &quot;.
        let s = strip_html_tags("Rust&#39;s &quot;safety&quot; &amp; speed &lt;3&nbsp;ftw");
        assert_eq!(s, "Rust's \"safety\" & speed <3\u{a0}ftw");
    }

    #[test]
    fn blocks_non_http_and_internal_hosts() {
        assert!(guard_url("file:///etc/passwd").is_err());
        assert!(guard_url("http://localhost:8787/").is_err());
        assert!(guard_url("http://127.0.0.1/").is_err());
        assert!(guard_url("http://192.168.1.1/").is_err());
        assert!(guard_url("http://172.16.0.1/").is_err());
        assert!(guard_url("https://example.com/page").is_ok());
        assert!(guard_url("http://172.32.0.1/").is_ok()); // fuera del rango privado
    }
}
