//! Almacén de usuarios. Trait + implementación en memoria (dev/test).
//! En producción se sustituye por `PostgresStore` (sqlx) — mismo trait.

use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct User {
    pub id: String,
    pub email: String,
    pub password_hash: String,
    pub tier: String,
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
}

/// Implementación en memoria (no persiste entre reinicios).
#[derive(Default)]
pub struct InMemoryStore {
    users: Mutex<HashMap<String, User>>, // id -> user
}

impl UserStore for InMemoryStore {
    fn create_user(&self, email: &str, password_hash: &str) -> Result<User, String> {
        let mut users = self.users.lock().unwrap();
        if users.values().any(|u| u.email == email) {
            return Err("el email ya está registrado".into());
        }
        let user = User {
            id: uuid::Uuid::new_v4().to_string(),
            email: email.to_string(),
            password_hash: password_hash.to_string(),
            tier: "free".to_string(),
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
        self.users.lock().unwrap().get(id).cloned()
    }

    #[allow(dead_code)]
    fn set_tier(&self, id: &str, tier: &str) -> Result<(), String> {
        let mut users = self.users.lock().unwrap();
        let u = users.get_mut(id).ok_or("usuario no encontrado")?;
        u.tier = tier.to_string();
        Ok(())
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
}
