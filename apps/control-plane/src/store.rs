//! Almacén de usuarios. Trait + implementación en memoria (tests) y en archivo
//! JSONL persistente (producción local, sin Docker/Postgres). El swap a Postgres
//! es directo tras el mismo trait.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub email: String,
    pub password_hash: String,
    pub tier: String,
    /// Hash (Argon2) del código de recuperación de contraseña (local-first, sin email).
    #[serde(default)]
    pub recovery_hash: String,
}

/// Contrato de persistencia de usuarios.
pub trait UserStore: Send + Sync {
    fn create_user(&self, email: &str, password_hash: &str) -> Result<User, String>;
    fn find_by_email(&self, email: &str) -> Option<User>;
    fn find_by_id(&self, id: &str) -> Option<User>;
    /// Cambia el plan del usuario. Se cableará al webhook de Stripe (F1 billing).
    /// Cubierto por tests; el uso en producción llega con el webhook.
    #[allow(dead_code)]
    fn set_tier(&self, id: &str, tier: &str) -> Result<(), String>;
    /// Guarda el hash del código de recuperación (al registrarse).
    fn set_recovery(&self, id: &str, recovery_hash: &str) -> Result<(), String>;
    /// Cambia la contraseña (al recuperar).
    fn update_password(&self, id: &str, password_hash: &str) -> Result<(), String>;
}

/// Implementación en memoria (solo tests; no persiste entre reinicios).
#[derive(Default)]
#[allow(dead_code)]
pub struct InMemoryStore {
    users: Mutex<HashMap<String, User>>, // id -> user
}

impl UserStore for InMemoryStore {
    fn create_user(&self, email: &str, password_hash: &str) -> Result<User, String> {
        let mut users = self.users.lock().unwrap_or_else(|e| e.into_inner());
        if users.values().any(|u| u.email == email) {
            return Err("el email ya está registrado".into());
        }
        let user = User {
            id: uuid::Uuid::new_v4().to_string(),
            email: email.to_string(),
            password_hash: password_hash.to_string(),
            tier: "free".to_string(),
            recovery_hash: String::new(),
        };
        users.insert(user.id.clone(), user.clone());
        Ok(user)
    }

    fn find_by_email(&self, email: &str) -> Option<User> {
        self.users
            .lock()
            .unwrap()
            .values()
            .find(|u| u.email == email)
            .cloned()
    }

    fn find_by_id(&self, id: &str) -> Option<User> {
        self.users
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned()
    }

    #[allow(dead_code)]
    fn set_tier(&self, id: &str, tier: &str) -> Result<(), String> {
        let mut users = self.users.lock().unwrap_or_else(|e| e.into_inner());
        let u = users.get_mut(id).ok_or("usuario no encontrado")?;
        u.tier = tier.to_string();
        Ok(())
    }

    fn set_recovery(&self, id: &str, recovery_hash: &str) -> Result<(), String> {
        let mut users = self.users.lock().unwrap_or_else(|e| e.into_inner());
        let u = users.get_mut(id).ok_or("usuario no encontrado")?;
        u.recovery_hash = recovery_hash.to_string();
        Ok(())
    }

    fn update_password(&self, id: &str, password_hash: &str) -> Result<(), String> {
        let mut users = self.users.lock().unwrap_or_else(|e| e.into_inner());
        let u = users.get_mut(id).ok_or("usuario no encontrado")?;
        u.password_hash = password_hash.to_string();
        Ok(())
    }
}

/// Almacén PERSISTENTE en archivo JSONL (un usuario por línea). Las cuentas
/// sobreviven a reinicios — sin Docker ni base de datos externa.
pub struct FileStore {
    path: PathBuf,
    users: Mutex<HashMap<String, User>>, // id -> user
}

impl FileStore {
    /// Abre (o crea) el almacén en `path`, cargando los usuarios existentes.
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let mut users = HashMap::new();
        if let Ok(content) = std::fs::read_to_string(&path) {
            for line in content.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(u) = serde_json::from_str::<User>(line) {
                    users.insert(u.id.clone(), u);
                }
            }
        }
        Self {
            path,
            users: Mutex::new(users),
        }
    }

    pub fn len(&self) -> usize {
        self.users.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Reescribe todo el archivo desde el mapa en memoria.
    fn flush(&self, users: &HashMap<String, User>) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut buf = String::new();
        for u in users.values() {
            buf.push_str(&serde_json::to_string(u).map_err(|e| e.to_string())?);
            buf.push('\n');
        }
        let tmp = self.path.with_extension("jsonl.tmp");
        std::fs::write(&tmp, buf).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())?; // atómico
        Ok(())
    }
}

impl UserStore for FileStore {
    fn create_user(&self, email: &str, password_hash: &str) -> Result<User, String> {
        let mut users = self.users.lock().unwrap_or_else(|e| e.into_inner());
        if users.values().any(|u| u.email.eq_ignore_ascii_case(email)) {
            return Err("el email ya está registrado".into());
        }
        let user = User {
            id: uuid::Uuid::new_v4().to_string(),
            email: email.to_string(),
            password_hash: password_hash.to_string(),
            tier: "free".to_string(),
            recovery_hash: String::new(),
        };
        // Append rápido + el flush completo garantiza consistencia.
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            if let Ok(line) = serde_json::to_string(&user) {
                let _ = writeln!(f, "{line}");
            }
        }
        users.insert(user.id.clone(), user.clone());
        Ok(user)
    }

    fn find_by_email(&self, email: &str) -> Option<User> {
        self.users
            .lock()
            .unwrap()
            .values()
            .find(|u| u.email.eq_ignore_ascii_case(email))
            .cloned()
    }

    fn find_by_id(&self, id: &str) -> Option<User> {
        self.users
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned()
    }

    fn set_tier(&self, id: &str, tier: &str) -> Result<(), String> {
        let mut users = self.users.lock().unwrap_or_else(|e| e.into_inner());
        let u = users.get_mut(id).ok_or("usuario no encontrado")?;
        u.tier = tier.to_string();
        let snapshot = users.clone();
        drop(users);
        self.flush(&snapshot)
    }

    fn set_recovery(&self, id: &str, recovery_hash: &str) -> Result<(), String> {
        let mut users = self.users.lock().unwrap_or_else(|e| e.into_inner());
        let u = users.get_mut(id).ok_or("usuario no encontrado")?;
        u.recovery_hash = recovery_hash.to_string();
        let snapshot = users.clone();
        drop(users);
        self.flush(&snapshot)
    }

    fn update_password(&self, id: &str, password_hash: &str) -> Result<(), String> {
        let mut users = self.users.lock().unwrap_or_else(|e| e.into_inner());
        let u = users.get_mut(id).ok_or("usuario no encontrado")?;
        u.password_hash = password_hash.to_string();
        let snapshot = users.clone();
        drop(users);
        self.flush(&snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_find() {
        let store = InMemoryStore::default();
        let u = store.create_user("a@b.com", "hash").unwrap();
        assert_eq!(store.find_by_email("a@b.com").unwrap().id, u.id);
        assert!(store.create_user("a@b.com", "hash2").is_err()); // duplicado
    }

    #[test]
    fn set_tier_updates_plan() {
        let store = InMemoryStore::default();
        let u = store.create_user("c@d.com", "hash").unwrap();
        assert_eq!(store.find_by_id(&u.id).unwrap().tier, "free");
        store.set_tier(&u.id, "pro").unwrap();
        assert_eq!(store.find_by_id(&u.id).unwrap().tier, "pro");
        assert!(store.set_tier("inexistente", "pro").is_err());
    }

    #[test]
    fn filestore_persists_across_reopen() {
        let dir = std::env::temp_dir().join(format!("aion_users_{}", std::process::id()));
        let path = dir.join("users.jsonl");
        let _ = std::fs::remove_dir_all(&dir);
        {
            let s = FileStore::open(&path);
            s.create_user("p@e.com", "hash").unwrap();
            assert!(s.create_user("p@e.com", "x").is_err()); // duplicado
        }
        // Reabrir: el usuario debe seguir ahí (persistencia entre reinicios).
        let s2 = FileStore::open(&path);
        assert!(
            s2.find_by_email("P@E.COM").is_some(),
            "case-insensitive + persistente"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
