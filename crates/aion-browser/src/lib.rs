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

pub use driver::{
    bing_search, debug_html, google_search, web_search, BrowserDriver, ChromiumoxideDriver, El,
    PageView, Snapshot,
};

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

    /// **Búsqueda web real** multi-fuente, sin API key. Fuente PRINCIPAL: la **web real** vía el
    /// navegador propio de AION (buscadores permisivos con rotación), respaldada por OpenAlex
    /// (académico), DuckDuckGo y Wikipedia. Devuelve título, URL y fragmento para investigar.
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        // BUSCADOR PROPIO MULTI-FUENTE (todo gratis y SIN clave, nada de APIs de pago):
        //  · WEB REAL vía navegador propio de AION: prueba buscadores permisivos en orden
        //    (Brave→Startpage→DuckDuckGo→Mojeek→Ecosia) y rota si alguno pide captcha. Datos web
        //    de verdad, no Wikipedia. (Google/Bing quedan para uso logueado: bloquean al bot.)
        //  · SearXNG self-hosted (si Ariel lo corre) · OpenAlex (~250M papers académicos)
        //  · DuckDuckGo (html+instant) · Wikipedia ES/EN (respaldo enciclopédico)
        // Se consultan EN PARALELO y se FUSIONAN (dedup por host), priorizando la web real.
        use futures_util::stream::{FuturesUnordered, StreamExt};
        let q = query.trim();
        // Fan-out RESILIENTE. Cada fuente corre en paralelo con techo de 8s y devuelve su
        // PRIORIDAD de mezcla (0 = primero). Recolectamos según llegan; una fuente caída no
        // arrastra la búsqueda. Pero ESPERAMOS a la web real (la principal, prio 0) hasta ~12s
        // antes del corte anticipado: arranca más lento (Chrome + rotación) pero es la mejor.
        let dur = Duration::from_secs(8);
        // La web real (prio 0) puede tardar más: lanza/usa Chrome y puede rotar varios motores.
        let web_dur = Duration::from_secs(15);
        let flat = |r: std::result::Result<
            Result<Vec<SearchResult>>,
            tokio::time::error::Elapsed,
        >| { r.ok().and_then(|x| x.ok()).unwrap_or_default() };
        type SrcFut<'a> = std::pin::Pin<
            Box<dyn std::future::Future<Output = (u8, Vec<SearchResult>)> + Send + 'a>,
        >;
        let mut futs: FuturesUnordered<SrcFut> = FuturesUnordered::new();
        // Prio 0: WEB REAL por el navegador propio (rota buscadores permisivos). Es la mejor
        // fuente; por eso esperamos a que llegue (primary) antes de conformarnos con el resto.
        futs.push(Box::pin(async {
            (
                0u8,
                flat(tokio::time::timeout(web_dur, driver::web_search(q, limit)).await),
            )
        }));
        futs.push(Box::pin(async {
            (
                1u8,
                flat(tokio::time::timeout(dur, self.search_searxng(q, limit)).await),
            )
        }));
        futs.push(Box::pin(async {
            (
                2u8,
                flat(tokio::time::timeout(dur, self.search_openalex(q, limit)).await),
            )
        }));
        futs.push(Box::pin(async {
            (
                3u8,
                flat(tokio::time::timeout(dur, self.search_ddg(q, limit)).await),
            )
        }));
        futs.push(Box::pin(async {
            (
                4u8,
                flat(tokio::time::timeout(dur, self.search_ddg_instant(q, limit)).await),
            )
        }));
        futs.push(Box::pin(async {
            (
                5u8,
                flat(
                    tokio::time::timeout(dur, self.search_wiki("en.wikipedia.org", q, limit)).await,
                ),
            )
        }));
        futs.push(Box::pin(async {
            (
                6u8,
                flat(
                    tokio::time::timeout(dur, self.search_wiki("es.wikipedia.org", q, limit)).await,
                ),
            )
        }));

        let total = futs.len();
        let mut groups: Vec<(u8, Vec<SearchResult>)> = Vec::new();
        let start = tokio::time::Instant::now();
        let mut deadline = start + web_dur; // techo absoluto (la web real puede tardar)
        let mut primary_done = false; // ¿llegó ya la web real (prio 0)?
        loop {
            let sleep = tokio::time::sleep_until(deadline);
            tokio::pin!(sleep);
            tokio::select! {
                biased;
                maybe = futs.next() => match maybe {
                    Some((prio, v)) => {
                        if prio == 0 {
                            primary_done = true;
                        }
                        groups.push((prio, v));
                        if groups.len() == total {
                            break;
                        }
                        // Corte anticipado solo si ya tenemos la web real (o ya esperamos >12s por
                        // ella, p. ej. rotando motores) y hay ≥3 fuentes: +1s a las rezagadas y cierra.
                        let may_cut = primary_done || start.elapsed() >= Duration::from_secs(12);
                        if may_cut && groups.len() >= 3 {
                            let soon = tokio::time::Instant::now() + Duration::from_millis(1000);
                            if soon < deadline {
                                deadline = soon;
                            }
                        }
                    }
                    None => break,
                },
                _ = &mut sleep => break,
            }
        }
        // Ordena por prioridad de mezcla y concatena: las fuentes que no llegaron a tiempo
        // simplemente no aportan.
        groups.sort_by_key(|(p, _)| *p);

        let mut out: Vec<SearchResult> = Vec::new();
        let mut seen_host: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut seen_url: std::collections::HashSet<String> = std::collections::HashSet::new();
        // Diversifica por host; Wikipedia puede repetir host (es/en).
        for r in groups.into_iter().flat_map(|(_, v)| v) {
            if !seen_url.insert(r.url.clone()) {
                continue; // misma URL exacta ya incluida
            }
            let host = host_of(&r.url);
            // Permite varias entradas pero limita duplicados del MISMO host (diversidad).
            let dup = seen_host.contains(&host) && !host.ends_with("wikipedia.org");
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
                });
            }
        }
        Ok(out)
    }

    /// Búsqueda vía API de Wikipedia en cualquier idioma (`host` = es/en.wikipedia.org). Fuente
    /// fiable: devuelve artículos reales con extracto y URL navegable. La API `list=search`
    /// busca por CONTENIDO (no solo por título), así que acierta con consultas largas.
    async fn search_wiki(
        &self,
        host: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        let q = urlencode(query.trim());
        let url = format!(
            "https://{host}/w/api.php?action=query&list=search&srsearch={q}\
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
                    url: format!("https://{host}/wiki/{}", urlencode(&page)),
                    title,
                    snippet,
                });
            }
        }
        Ok(out)
    }

    /// **OpenAlex** (académico, SIN API key): ~250M trabajos científicos arbitrados. Devuelve
    /// título, año, abstract reconstruido y URL navegable (DOI o landing). Fuente de nivel
    /// PROFESIONAL para investigación — gratis, propia, sin pagar. Lo que faltaba para que AION
    /// citara papers de verdad cuando Ariel pide "solo fuentes académicas".
    async fn search_openalex(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        // `mailto` mete la petición en el "polite pool" de OpenAlex: mucho más rápido y fiable
        // que el pool común (que llega a tardar >15s y dar timeout). Es gratis y sin API key.
        let url = format!(
            "https://api.openalex.org/works?search={}&per_page={}&mailto=info@prontoclick.it",
            urlencode(query.trim()),
            limit.clamp(1, 10)
        );
        let json: serde_json::Value = self
            .http
            .get(&url)
            .header("User-Agent", "AION/1.0 (agente local-first)")
            .send()
            .await
            .map_err(|e| AionError::Internal(format!("openalex falló: {e}")))?
            .json()
            .await
            .map_err(|e| AionError::Internal(format!("openalex json inválido: {e}")))?;
        let mut out = Vec::new();
        if let Some(arr) = json["results"].as_array() {
            for w in arr.iter().take(limit) {
                let title = w["display_name"].as_str().unwrap_or("").to_string();
                if title.is_empty() {
                    continue;
                }
                // URL navegable: DOI si existe, si no la landing de OpenAlex.
                let url = w["doi"]
                    .as_str()
                    .map(str::to_string)
                    .or_else(|| w["id"].as_str().map(str::to_string))
                    .unwrap_or_default();
                if url.is_empty() {
                    continue;
                }
                let year = w["publication_year"]
                    .as_i64()
                    .map(|y| y.to_string())
                    .unwrap_or_default();
                let abs = reconstruct_abstract(&w["abstract_inverted_index"]);
                let snippet = if abs.is_empty() {
                    format!("[Académico {year}] {title}")
                } else {
                    let a: String = abs.chars().take(280).collect();
                    format!("[Académico {year}] {a}")
                };
                out.push(SearchResult {
                    title,
                    url,
                    snippet,
                });
            }
        }
        Ok(out)
    }

    /// **SearXNG** self-hosted (opcional): metabuscador PROPIO, gratis y sin clave, que agrega
    /// Google/Bing/Brave/etc. respetando la privacidad — el "patrón oro" libre para agentes. Se
    /// activa con `AION_SEARXNG_URL` (p. ej. http://127.0.0.1:8888). Sin esa variable, no hace
    /// nada (silencioso). Da búsqueda web de alta calidad sin depender de una API de pago.
    async fn search_searxng(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let Ok(base) = std::env::var("AION_SEARXNG_URL") else {
            return Ok(vec![]);
        };
        let base = base.trim().trim_end_matches('/');
        if base.is_empty() {
            return Ok(vec![]);
        }
        let url = format!("{base}/search?q={}&format=json", urlencode(query.trim()));
        let json: serde_json::Value = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| AionError::Internal(format!("searxng falló: {e}")))?
            .json()
            .await
            .map_err(|e| AionError::Internal(format!("searxng json inválido: {e}")))?;
        let mut out = Vec::new();
        if let Some(arr) = json["results"].as_array() {
            for r in arr.iter().take(limit) {
                let (Some(title), Some(u)) = (r["title"].as_str(), r["url"].as_str()) else {
                    continue;
                };
                out.push(SearchResult {
                    title: title.to_string(),
                    url: u.to_string(),
                    snippet: r["content"].as_str().unwrap_or("").to_string(),
                });
            }
        }
        Ok(out)
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
        if text.len() > self.max_chars {
            text.truncate(self.max_chars);
            text.push_str(" …[truncado]");
        }
        Ok(text)
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
}

/// Un lugar/negocio encontrado por dirección (OpenStreetMap).
#[derive(Debug, Clone)]
pub struct PlaceResult {
    pub name: String,
    pub kind: String,
    pub address: String,
}

/// Parsea los resultados del HTML de DuckDuckGo (clases result__a / result__snippet).
/// Las URLs vienen como redirección `...uddg=<url codificada>`; se decodifican.
fn parse_ddg_results(html: &str, limit: usize) -> Vec<SearchResult> {
    let mut out = Vec::new();
    for block in html.split("result__a").skip(1) {
        // href="...uddg=ENCODED&rut=..."
        let url = block
            .find("uddg=")
            .map(|i| &block[i + 5..])
            .and_then(|s| s.split(['&', '"']).next())
            .map(percent_decode)
            .unwrap_or_default();
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
        if !url.is_empty() && url.starts_with("http") {
            out.push(SearchResult {
                title: title.trim().to_string(),
                url,
                snippet: snippet.trim().to_string(),
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
    out.replace("&amp;", "&")
        .replace("&#x27;", "'")
        .replace("&quot;", "\"")
        .trim()
        .to_string()
}

/// Reconstruye el abstract de un trabajo de OpenAlex desde su `abstract_inverted_index`
/// (mapa palabra → posiciones), que es como OpenAlex entrega los resúmenes. Ordena las palabras
/// por posición y las une. Vacío si no hay índice. Esto le da a AION un extracto real del paper
/// para razonar y citar, no solo el título.
fn reconstruct_abstract(idx: &serde_json::Value) -> String {
    let Some(obj) = idx.as_object() else {
        return String::new();
    };
    let mut words: Vec<(u64, &str)> = Vec::new();
    for (w, positions) in obj {
        if let Some(arr) = positions.as_array() {
            for p in arr {
                if let Some(pos) = p.as_u64() {
                    words.push((pos, w.as_str()));
                }
            }
        }
    }
    words.sort_by_key(|(p, _)| *p);
    words
        .into_iter()
        .map(|(_, w)| w)
        .collect::<Vec<_>>()
        .join(" ")
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

/// Codifica una cadena para usarla en una query string (percent-encoding).
/// Extrae el host de una URL (para deduplicar por dominio). Best-effort, sin deps.
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
