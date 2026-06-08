//! CRDT Last-Write-Wins Map: convergente y libre de conflictos por clave.
//! Cada entrada guarda (valor, timestamp lógico); al fusionar gana el timestamp
//! mayor (desempate determinista por valor) → ambos dispositivos convergen.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct LwwMap {
    entries: BTreeMap<String, (String, i64)>, // key -> (value, ts)
}

impl LwwMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fija una clave con un timestamp lógico (mayor = más reciente).
    pub fn set(&mut self, key: &str, value: &str, ts: i64) {
        match self.entries.get(key) {
            Some((v, t)) if (*t, v.as_str()) >= (ts, value) => {} // ya hay algo más nuevo
            _ => {
                self.entries
                    .insert(key.to_string(), (value.to_string(), ts));
            }
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(|(v, _)| v.as_str())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Fusiona otro mapa en este (commutativa, idempotente, asociativa).
    pub fn merge(&mut self, other: &LwwMap) {
        for (k, (v, t)) in &other.entries {
            self.set(k, v, *t);
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_converges_both_directions() {
        let mut a = LwwMap::new();
        a.set("tema", "plasma teal", 10);
        a.set("modo", "oscuro", 10);
        let mut b = LwwMap::new();
        b.set("idioma", "español", 11);
        b.set("modo", "claro", 20); // más reciente → debe ganar

        let mut a2 = a.clone();
        let mut b2 = b.clone();
        a2.merge(&b);
        b2.merge(&a);
        assert_eq!(a2, b2, "convergencia");
        assert_eq!(a2.get("modo"), Some("claro")); // LWW
        assert_eq!(a2.get("tema"), Some("plasma teal"));
        assert_eq!(a2.get("idioma"), Some("español"));
    }

    #[test]
    fn merge_is_idempotent() {
        let mut a = LwwMap::new();
        a.set("x", "1", 1);
        let b = a.clone();
        a.merge(&b);
        a.merge(&b);
        assert_eq!(a.len(), 1);
    }
}
