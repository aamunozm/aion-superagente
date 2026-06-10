//! # aion-browser
//!
//! Capacidad web del agente. F5 (actual): [`WebClient`] descarga una URL y extrae
//! su texto legible — el primitivo "leer la web". Incluye guardas anti-SSRF
//! (rechaza hosts internos/privados) y límite de tamaño.
//!
//! Evolución futura: navegación autónoma con DOM+visión (browser-use sidecar →
//! chromiumoxide/CDP) detrás de un trait `BrowserDriver`.

mod html;

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
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            // UA de navegador real: algunos sitios bloquean clientes desconocidos.
            .user_agent(
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 \
                 (KHTML, like Gecko) Version/17.0 Safari/605.1.15",
            )
            .build()
            .unwrap_or_default();
        Self {
            http,
            max_chars: MAX_CHARS,
        }
    }

    /// **Búsqueda web real** (DuckDuckGo HTML, sin API key). Devuelve resultados
    /// con título, URL y fragmento, para que el agente investigue en varias fuentes.
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        // 1) DuckDuckGo HTML (POST) — mejor cobertura general cuando no bloquea.
        if let Ok(resp) = self
            .http
            .post("https://html.duckduckgo.com/html/")
            .header("Accept-Language", "es-ES,es;q=0.9,en;q=0.8")
            .form(&[("q", query.trim()), ("kl", "wt-wt")])
            .send()
            .await
        {
            if let Ok(body) = resp.text().await {
                let results = parse_ddg_results(&body, limit);
                if !results.is_empty() {
                    return Ok(results);
                }
            }
        }
        // 2) Fallback FIABLE (sin rate-limit ni captcha): API de Wikipedia.
        self.search_wikipedia(query, limit).await
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
                });
            }
        }
        Ok(out)
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
}

/// Guarda anti-SSRF: solo http(s) y rechaza hosts internos/privados.
fn guard_url(url: &str) -> Result<()> {
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

/// Codifica una cadena para usarla en una query string (percent-encoding).
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
