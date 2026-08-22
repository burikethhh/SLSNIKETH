mod crypto;
mod models;
mod routes;

use axum::{
    routing::{get, post},
    Router,
};
use crypto::LicenseSigner;
use parking_lot::RwLock;
use routes::AppState;
use std::{collections::{HashMap, HashSet}, net::SocketAddr, sync::Arc};
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let signer = match std::env::var("RSA_PRIVATE_KEY_PEM") {
        Ok(pem) => {
            tracing::info!("Loaded RSA private key from environment");
            LicenseSigner::from_pem(&pem)?
        }
        Err(_) => {
            tracing::warn!("No RSA_PRIVATE_KEY_PEM provided. Generating ephemeral RSA-2048 keypair...");
            LicenseSigner::generate_ephemeral()?
        }
    };

    let state = Arc::new(AppState {
        signer,
        gyms: Arc::new(RwLock::new(HashMap::new())),
        disabled_gyms: Arc::new(RwLock::new(HashSet::new())),
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let dashboard_dir = if std::path::Path::new("cloud/dashboard").exists() {
        "cloud/dashboard"
    } else {
        "dashboard"
    };
    let dashboard_service = ServeDir::new(dashboard_dir)
        .append_index_html_on_directories(true);

    let app = Router::new()
        .route("/health", get(routes::health_check))
        .route("/api/v1/health", get(routes::health_check))
        .route("/api/v1/licenses/public-key", get(routes::get_public_key))
        .route("/api/v1/licenses/generate", post(routes::generate_license))
        .route("/api/v1/licenses/verify", post(routes::verify_license))
        .route("/api/v1/gyms/register", post(routes::register_gym))
        .route("/api/v1/gyms", get(routes::list_gyms))
        .route("/api/v1/sync/push", post(routes::sync_push))
        .route("/api/v1/sync/vectors", post(routes::sync_vectors))
        .route("/api/v1/remote/disable", post(routes::remote_disable))
        .fallback_service(dashboard_service)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("GymPOS Cloud Backend listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
