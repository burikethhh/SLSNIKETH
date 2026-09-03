mod crypto;
mod db;
mod models;
mod rate_limit;
mod routes;

use axum::{
    routing::{get, post},
    Router,
};
use crypto::LicenseSigner;
use db::CloudDatabase;
use parking_lot::RwLock;
use routes::AppState;
use std::{net::SocketAddr, sync::Arc};
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

    let signer = if let Ok(pem) = std::env::var("RSA_PRIVATE_KEY_PEM") {
        tracing::info!("Loaded RSA private key from environment");
        LicenseSigner::from_pem(&pem)?
    } else if let Ok(pem) = std::fs::read_to_string("rsa_private_key.pem") {
        tracing::info!("Loaded RSA private key from rsa_private_key.pem");
        LicenseSigner::from_pem(&pem)?
    } else {
        tracing::warn!("Generating in-memory ephemeral signing key");
        LicenseSigner::generate_ephemeral()?
    };

    let admin_key = match std::env::var("ADMIN_SECRET_KEY") {
        Ok(key) => key,
        Err(_) => "gympos_master_ceo_secret_2026".to_string(),
    };
    tracing::info!("Master Admin Authentication active");

    let cloud_db = Arc::new(CloudDatabase::new("gympos_cloud.sqlite")?);
    let loaded_gyms = cloud_db.load_all_gyms().unwrap_or_default();
    let loaded_disabled = cloud_db.load_disabled_gyms().unwrap_or_default();
    let loaded_revoked_licenses = cloud_db.load_revoked_license_ids().unwrap_or_default();
    tracing::info!(
        "Loaded {} gyms, {} disabled gyms, and {} revoked licenses from SQLite",
        loaded_gyms.len(),
        loaded_disabled.len(),
        loaded_revoked_licenses.len()
    );

    let login_limiter = Arc::new(rate_limit::RateLimiter::new());
    let limiter_for_cleanup = login_limiter.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(600)).await;
            limiter_for_cleanup.cleanup(std::time::Duration::from_secs(3600));
        }
    });

    let state = Arc::new(AppState {
        signer,
        db: cloud_db,
        gyms: Arc::new(RwLock::new(loaded_gyms)),
        disabled_gyms: Arc::new(RwLock::new(loaded_disabled)),
        revoked_licenses: Arc::new(RwLock::new(loaded_revoked_licenses)),
        admin_key,
        login_limiter,
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
        .route("/api/v1/auth/admin-login", post(routes::admin_login))
        .route("/api/v1/licenses/public-key", get(routes::get_public_key))
        .route("/api/v1/licenses", get(routes::list_licenses))
        .route("/api/v1/licenses/generate", post(routes::generate_license))
        .route("/api/v1/licenses/verify", post(routes::verify_license))
        .route("/api/v1/licenses/revoke", post(routes::revoke_license_endpoint))
        .route("/api/v1/admin/owners", get(routes::admin_list_owners_hierarchy))
        .route("/api/v1/admin/owners/:email/branches", post(routes::admin_create_branch_for_owner))
        .route("/api/v1/admin/branches/:gym_id/issue-key", post(routes::admin_issue_branch_key))
        .route("/api/v1/gyms/register", post(routes::register_gym))
        .route("/api/v1/gyms", get(routes::list_gyms))
        .route("/api/v1/gyms/update", post(routes::update_gym))
        .route("/api/v1/gyms/:id", axum::routing::delete(routes::delete_gym))
        .route("/api/v1/sync/push", post(routes::sync_push))
        .route("/api/v1/sync/vectors", post(routes::sync_vectors))
        .route("/api/v1/remote/disable", post(routes::remote_disable))
        .route("/api/v1/analytics/fleet", get(routes::analytics_fleet))
        // Owner Multi-Branch Portal & Remote Catalog Bridge
        .route("/portal", get(serve_portal_html))
        .route("/portal.html", get(serve_portal_html))
        .route("/api/v1/owner/auth/register", post(routes::owner_register))
        .route("/api/v1/owner/auth/login", post(routes::owner_login))
        .route("/api/v1/owner/exists", get(routes::owner_check_exists))
        .route("/api/v1/owner/gyms", post(routes::owner_create_gym))
        .route("/api/v1/owner/branches", get(routes::owner_get_branches))
        .route("/api/v1/owner/analytics", get(routes::owner_get_analytics))
        .route("/api/v1/owner/catalog", get(routes::owner_get_catalog))
        .route("/api/v1/owner/catalog/products", post(routes::owner_save_products))
        .route("/api/v1/owner/catalog/plans", post(routes::owner_save_plans))
        .route("/api/v1/owner/catalog/promos", post(routes::owner_save_promos))
        .route("/api/v1/owner/catalog/override", post(routes::owner_save_branch_override))
        .route("/api/v1/owner/staff", get(routes::owner_list_staff).post(routes::owner_create_staff))
        .route("/api/v1/owner/staff/:id", axum::routing::put(routes::owner_update_staff).delete(routes::owner_delete_staff))
        // Scalable Auto-Updater & Release Controller
        .route("/api/v1/updates/check", get(routes::check_for_updates))
        .route("/api/v1/updates/publish", post(routes::publish_release_endpoint))
        .route("/api/v1/updates/releases", get(routes::list_releases_endpoint))
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
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}

async fn serve_portal_html() -> impl axum::response::IntoResponse {
    let path = if std::path::Path::new("cloud/dashboard/portal.html").exists() {
        "cloud/dashboard/portal.html"
    } else {
        "dashboard/portal.html"
    };
    match tokio::fs::read_to_string(path).await {
        Ok(html) => (
            axum::http::StatusCode::OK,
            [("content-type", "text/html; charset=utf-8")],
            html,
        ),
        Err(_) => (
            axum::http::StatusCode::NOT_FOUND,
            [("content-type", "text/plain")],
            "Portal not found".to_string(),
        ),
    }
}
