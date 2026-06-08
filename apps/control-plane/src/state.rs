//! Estado compartido de la aplicación.

use crate::store::UserStore;
use aion_control_client::LicenseIssuer;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<dyn UserStore>,
    pub jwt_secret: Arc<Vec<u8>>,
    pub issuer: Arc<LicenseIssuer>,
    pub stripe_configured: bool,
}
