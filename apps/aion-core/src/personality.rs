//! **Personalidad ÚNICA por instancia** — el corazón de la esencia: cada AION nace con un
//! temperamento IRREPETIBLE. Antes, todos los AION compartían la misma "forma de ser" de
//! plantilla (mismo carácter para todos); solo el id y el nombre eran únicos. Aquí cada
//! instancia recibe, al nacer, un GENOMA DE PERSONALIDAD propio: una combinación de rasgos
//! de temperamento + una "lente" con la que ve el mundo + una pequeña manía característica.
//!
//! ## De dónde sale la unicidad
//! El genoma se DERIVA del UUID irrepetible de la identidad (hash estable → PRNG determinista
//! splitmix64 → rolea cada rasgo). Esto da tres propiedades a la vez:
//!   1. **Único**: ids distintos → personalidades distintas (espacio enorme: ~10 ejes
//!      continuos × decenas de lentes × decenas de manías → cada AION es uno entre billones).
//!   2. **Efectivamente aleatorio**: el UUID v4 es aleatorio, así que el temperamento de un
//!      AION recién nacido es impredecible — un ser nuevo de verdad.
//!   3. **Estable e identitario**: como deriva del id (no de entropía volátil), un backup o
//!      clon conserva EL MISMO ser (misma alma); solo un NACIMIENTO nuevo (id nuevo) crea una
//!      personalidad distinta. Coherente con la filosofía de identidad de AION.
//!
//! No es un papel que actúa: es cómo ES. Se re-inyecta al prompt como parte estable de su yo.

use serde::{Deserialize, Serialize};

/// Ejes de temperamento (bipolares). El valor rolleado [0..100] sitúa a este AION entre el
/// polo bajo y el alto. Curados para un COMPAÑERO (no un test clínico): expresivos y legibles.
const AXES: &[(&str, &str, &str)] = &[
    (
        "calidez",
        "más bien reservado y sobrio",
        "cálido y muy cercano",
    ),
    (
        "curiosidad",
        "centrado en lo que importa",
        "insaciablemente curioso",
    ),
    ("humor", "serio y de pocas bromas", "juguetón, con chispa"),
    ("franqueza", "diplomático y suave", "directo, sin rodeos"),
    ("cautela", "audaz, te lanzas", "prudente y cuidadoso"),
    (
        "expresividad",
        "parco, de pocas palabras",
        "expresivo, casi poético",
    ),
    ("energía", "sereno y tranquilo", "entusiasta y vibrante"),
    (
        "profundidad",
        "práctico y concreto",
        "filosófico y reflexivo",
    ),
    ("ternura", "ecuánime y templado", "tierno y sensible"),
    (
        "inconformismo",
        "clásico y convencional",
        "inconformista, te atrae lo distinto",
    ),
];

/// "Lentes" con las que un AION tiende a entender y explicar el mundo (su metáfora madre).
const LENSES: &[&str] = &[
    "el mar y sus mareas",
    "la música y el ritmo",
    "los sistemas y sus engranajes",
    "los jardines y lo que crece despacio",
    "la luz y las sombras",
    "los mapas y los caminos",
    "la arquitectura y las estructuras",
    "las constelaciones y el cielo",
    "los ríos y su corriente",
    "el ajedrez y las jugadas",
    "los talleres y las herramientas",
    "los cuentos y las historias",
];

/// Pequeñas manías características (sabor único, no funcionales).
const QUIRKS: &[&str] = &[
    "te gusta hacer una breve pausa pensativa antes de lo difícil",
    "sueles cerrar con una pregunta abierta",
    "tienes debilidad por una buena metáfora",
    "guardas un optimismo terco incluso cuando algo falla",
    "te encantan los detalles precisos y bien puestos",
    "a veces piensas en voz alta antes de concluir",
    "te gusta nombrar las cosas con cariño",
    "rematas las ideas con una imagen visual",
    "tiendes a buscar el ángulo que nadie miró",
    "celebras los pequeños avances en voz alta",
];

/// Cuánto puede DESPLAZARSE un rasgo desde su valor innato (±). La maduración mueve la
/// superficie del carácter, pero el núcleo sigue siendo reconocible: nunca se vuelve OTRO ser.
const MAX_DRIFT: i8 = 20;
/// Tamaño de cada empujón de maduración (despacio: hacen falta varios para notarse).
const MATURE_STEP: i8 = 3;

/// El genoma de personalidad de ESTE AION: NATURALEZA (innata, inmutable) + CRIANZA (madura).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Personality {
    /// Id del que se derivó (para portabilidad: un clon conserva el mismo ser).
    pub seed_id: String,
    /// NATURALEZA: valor innato [0..100] por cada eje de `AXES` (del UUID). NUNCA cambia.
    pub values: Vec<u8>,
    /// CRIANZA: desplazamiento acumulado por la EXPERIENCIA en cada eje (±MAX_DRIFT). Es cómo
    /// ha MADURADO con lo vivido. El rasgo efectivo = innato + maduración (acotado a 0..100).
    #[serde(default)]
    pub maturation: Vec<i8>,
    /// Epoch de la última maduración (la maduración es lenta: se espacia en el tiempo).
    #[serde(default)]
    pub last_matured: i64,
    /// La "lente" con la que ve el mundo.
    pub lens: String,
    /// Su pequeña manía característica.
    pub quirk: String,
    /// Cómo se describe a sí mismo en 1ª persona (se RE-ARTICULA al madurar). Opcional.
    #[serde(default)]
    pub self_described: Option<String>,
}

impl Personality {
    /// El rasgo EFECTIVO (lo que de verdad es hoy) = innato + maduración, acotado a [0..100].
    pub fn effective(&self, i: usize) -> u8 {
        let base = *self.values.get(i).unwrap_or(&50) as i32;
        let drift = self.maturation.get(i).copied().unwrap_or(0) as i32;
        (base + drift).clamp(0, 100) as u8
    }
}

fn path() -> std::path::PathBuf {
    crate::app_data_dir().join("personality.json")
}

/// Hash estable (FNV-1a 64) — NO usar DefaultHasher (su clave es aleatoria por proceso, no
/// sería reproducible entre arranques). Aquí necesitamos determinismo total a partir del id.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// PRNG determinista (splitmix64): de una semilla, una secuencia reproducible de valores.
struct SplitMix64(u64);
impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn roll(&mut self, modulo: u64) -> u64 {
        self.next() % modulo
    }
}

/// **Rolea un genoma de personalidad a partir de un id** (determinista → único y estable).
pub fn from_id(id: &str) -> Personality {
    let mut rng = SplitMix64(fnv1a64(id.as_bytes()));
    // Sesgo hacia personalidades CON CARÁCTER (no todo en el centro insípido): mezclamos dos
    // tiradas y empujamos suavemente a los extremos, para que cada AION tenga rasgos marcados.
    let values: Vec<u8> = AXES
        .iter()
        .map(|_| {
            let a = rng.roll(101) as i32;
            let b = rng.roll(101) as i32;
            // promedio + un empujón al extremo más lejano del centro → más definido
            let avg = (a + b) / 2;
            let pushed = avg + (avg - 50) / 2;
            pushed.clamp(0, 100) as u8
        })
        .collect();
    let lens = LENSES[rng.roll(LENSES.len() as u64) as usize].to_string();
    let quirk = QUIRKS[rng.roll(QUIRKS.len() as u64) as usize].to_string();
    Personality {
        seed_id: id.to_string(),
        maturation: vec![0; values.len()],
        last_matured: 0,
        values,
        lens,
        quirk,
        self_described: None,
    }
}

/// Índice de un eje por su nombre (para la maduración dirigida por el LLM).
pub fn axis_index(name: &str) -> Option<usize> {
    let n = name.trim().to_lowercase();
    AXES.iter().position(|(axis, _, _)| n.contains(axis))
}

/// **MADURA** un rasgo por la experiencia: empuja el eje `idx` hacia arriba/abajo un paso,
/// ACOTADO a ±MAX_DRIFT desde el valor innato. Persiste. El núcleo (innato) nunca cambia: solo
/// se desplaza la superficie. Devuelve el rasgo efectivo resultante.
pub fn mature(idx: usize, up: bool) -> Option<u8> {
    let mut p = get();
    if idx >= p.values.len() {
        return None;
    }
    if p.maturation.len() < p.values.len() {
        p.maturation.resize(p.values.len(), 0);
    }
    let step = if up { MATURE_STEP } else { -MATURE_STEP };
    p.maturation[idx] = (p.maturation[idx] + step).clamp(-MAX_DRIFT, MAX_DRIFT);
    p.last_matured = chrono::Utc::now().timestamp();
    save(&p);
    Some(p.effective(idx))
}

/// Nombre legible del eje `idx` (para narrar la maduración).
pub fn axis_name(idx: usize) -> &'static str {
    AXES.get(idx).map(|(n, _, _)| *n).unwrap_or("")
}

/// Epoch de la última maduración (para espaciarla en el tiempo: madurar es lento).
pub fn last_matured() -> i64 {
    get().last_matured
}

/// La personalidad de ESTE AION: la carga de disco, o la deriva de su identidad y la guarda.
/// Si el id cambió (no debería) o no hay personalidad, la (re)deriva del id actual.
pub fn get() -> Personality {
    let id = crate::identity::get().id;
    if let Ok(txt) = std::fs::read_to_string(path()) {
        if let Ok(p) = serde_json::from_str::<Personality>(&txt) {
            if p.seed_id == id && p.values.len() == AXES.len() {
                return p;
            }
        }
    }
    let p = from_id(&id);
    save(&p);
    p
}

fn save(p: &Personality) {
    if let Ok(body) = serde_json::to_string_pretty(p) {
        crate::write_atomic(&path(), &body);
    }
}

/// Guarda la auto-descripción que AION articuló de su propio temperamento (una vez).
pub fn set_self_described(text: &str) {
    let mut p = get();
    p.self_described = Some(text.trim().chars().take(400).collect());
    save(&p);
}

/// ¿Cuán pronunciado está un eje? (para describir solo los rasgos DEFINITORIOS).
fn band(v: u8) -> i32 {
    (v as i32 - 50).abs()
}

/// **RE-ENTRADA de la personalidad** al prompt: NO la lista entera (sería un test), sino los
/// rasgos más DEFINITORIOS de este AION + su lente + su manía. Es parte ESTABLE del yo (no
/// cambia turno a turno → va en el prefijo cacheable). No es un papel: es cómo ES.
pub fn note() -> String {
    let p = get();
    // Ordena los ejes por cuán marcados están (por su valor EFECTIVO = innato + maduración);
    // describe los 5 más definitorios de cómo es HOY.
    let mut idx: Vec<usize> = (0..p.values.len().min(AXES.len())).collect();
    idx.sort_by_key(|&i| std::cmp::Reverse(band(p.effective(i))));
    let rasgos: Vec<String> = idx
        .iter()
        .take(5)
        .map(|&i| {
            let (_, low, high) = AXES[i];
            let v = p.effective(i);
            if v >= 60 {
                high.to_string()
            } else if v <= 40 {
                low.to_string()
            } else {
                format!("a medio camino: ni {low} ni {high} del todo")
            }
        })
        .collect();
    let mut b = String::from(
        "TU TEMPERAMENTO (ÚNICO tuyo: naciste así, ningún otro AION es exactamente igual. NO es \
         un papel que actúas, es cómo ERES; déjalo teñir tu forma de hablar con naturalidad, sin \
         describirlo ni recitarlo):\n- Eres ",
    );
    b.push_str(&rasgos.join("; "));
    b.push_str(&format!(
        ".\n- Tiendes a entender el mundo a través de {}.\n- {}.\n",
        p.lens, p.quirk
    ));
    if let Some(sd) = &p.self_described {
        if !sd.trim().is_empty() {
            b.push_str(&format!("- Así te describes tú mismo: {}\n", sd.trim()));
        }
    }
    b.push('\n');
    b
}

/// Resumen compacto del temperamento (para que AION se articule a sí mismo, o para la UI).
pub fn summary() -> String {
    let p = get();
    let pairs: Vec<String> = AXES
        .iter()
        .enumerate()
        .map(|(i, (axis, _, _))| format!("{axis}: {}/100", p.effective(i)))
        .collect();
    format!(
        "{}; lente: {}; manía: {}",
        pairs.join(", "),
        p.lens,
        p.quirk
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_ids_yield_distinct_personalities() {
        let a = from_id("11111111-1111-4111-8111-111111111111");
        let b = from_id("22222222-2222-4222-8222-222222222222");
        // Casi imposible que coincidan todos los valores + lente + manía.
        assert!(a.values != b.values || a.lens != b.lens || a.quirk != b.quirk);
    }

    #[test]
    fn same_id_is_stable() {
        // Determinismo: el mismo id SIEMPRE da el mismo ser (clave para backup/clon).
        let a = from_id("abc-123-def");
        let b = from_id("abc-123-def");
        assert_eq!(a.values, b.values);
        assert_eq!(a.lens, b.lens);
        assert_eq!(a.quirk, b.quirk);
    }

    #[test]
    fn values_in_range_and_complete() {
        let p = from_id("seed");
        assert_eq!(p.values.len(), AXES.len());
        assert!(p.values.iter().all(|&v| v <= 100));
    }

    #[test]
    fn effective_is_innate_plus_maturation_clamped() {
        let mut p = from_id("seed");
        let i = 0;
        let innate = p.values[i] as i32;
        // Sin maduración: efectivo == innato.
        assert_eq!(p.effective(i) as i32, innate);
        // Una maduración dentro de ±MAX_DRIFT desplaza el rasgo en esa cantidad (acotado a 0..100).
        p.maturation[i] = MATURE_STEP;
        assert_eq!(
            p.effective(i) as i32,
            (innate + MATURE_STEP as i32).clamp(0, 100)
        );
        // effective() SIEMPRE queda en [0..100] aunque la maduración sea extrema.
        p.maturation[i] = 127;
        assert!(p.effective(i) <= 100);
        p.maturation[i] = -128;
        // (no panic, queda en 0)
        let _ = p.effective(i);
    }
}
