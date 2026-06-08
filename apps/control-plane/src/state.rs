//! Estado compartido de la aplicación.

use crate::license::LicenseIssuer;
use crate::store::UserStore;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<dyn UserStore>,
    pub jwt_secret: Arc<Vec<u8>>,
    pub issuer: Arc<LicenseIssuer>,
    pub stripe_configured: bool,
}
