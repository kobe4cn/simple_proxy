use argon2::{
    Argon2,
    password_hash::{PasswordHasher, SaltString},
};
use axum::{
    Json, Router,
    error_handling::HandleErrorLayer,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use chrono::{DateTime, Utc};
use dashmap::DashMap;

use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tower::{BoxError, ServiceBuilder};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Serialize, Deserialize, Clone)]
struct User {
    id: u64,
    name: String,
    email: String,
    #[serde(skip_serializing)]
    password: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct CreateUser {
    name: String,
    email: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct UpdateUser {
    name: Option<String>,
    email: Option<String>,
    password: Option<String>,
}

#[derive(Debug, Clone)]
struct AppState {
    inner: Arc<AppStateInner>,
}

#[derive(Debug)]
struct AppStateInner {
    next_id: AtomicU64,
    users: DashMap<u64, User>,
    argon2: Argon2<'static>,
}

impl AppState {
    fn new() -> Self {
        Self {
            inner: Arc::new(AppStateInner {
                next_id: AtomicU64::new(1),
                users: DashMap::new(),
                argon2: Argon2::default(),
            }),
        }
    }

    fn get_user(&self, id: u64) -> Option<User> {
        self.inner.users.get(&id).map(|user| user.clone())
    }

    fn create_user(&self, name: String, email: String, password: String) -> Result<User, BoxError> {
        let id = self.inner.next_id.fetch_add(1, Ordering::SeqCst);
        let now = Utc::now();

        // Hash password with Argon2
        let salt = SaltString::generate(&mut OsRng);
        let password_hash = self
            .inner
            .argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| e.to_string())?
            .to_string();

        let user = User {
            id,
            name,
            email,
            password: password_hash,
            created_at: now,
            updated_at: now,
        };

        self.inner.users.insert(id, user.clone());
        Ok(user)
    }

    fn update_user(&self, id: u64, update: UpdateUser) -> Result<User, BoxError> {
        let mut user = self.inner.users.get(&id).ok_or("User not found")?.clone();

        if let Some(name) = update.name {
            user.name = name;
        }

        if let Some(email) = update.email {
            user.email = email;
        }

        if let Some(password) = update.password {
            // Hash new password
            let salt = SaltString::generate(&mut OsRng);
            let password_hash = self
                .inner
                .argon2
                .hash_password(password.as_bytes(), &salt)
                .map_err(|e| e.to_string())?
                .to_string();
            user.password = password_hash;
        }

        user.updated_at = Utc::now();
        self.inner.users.insert(id, user.clone());
        Ok(user)
    }

    fn delete_user(&self, id: u64) -> bool {
        self.inner.users.remove(&id).is_some()
    }

    fn get_all_users(&self) -> Vec<User> {
        self.inner
            .users
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    fn health(&self) -> bool {
        // Simple health check - could be extended with more checks
        true
    }
}

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                format!("{}=debug,tower_http=debug", env!("CARGO_CRATE_NAME")).into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let app_state = AppState::new();

    // Compose the routes
    let app = Router::new()
        .route("/users", get(get_users).post(create_user))
        .route(
            "/users/:id",
            get(get_user).put(update_user).delete(delete_user),
        )
        .route("/health", get(health_check))
        // Add middleware to all routes
        .layer(
            ServiceBuilder::new()
                .layer(HandleErrorLayer::new(|error: BoxError| async move {
                    if error.is::<tower::timeout::error::Elapsed>() {
                        Ok(StatusCode::REQUEST_TIMEOUT)
                    } else {
                        Err((
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!("Unhandled internal error: {error}"),
                        ))
                    }
                }))
                .timeout(std::time::Duration::from_secs(10))
                .layer(TraceLayer::new_for_http())
                .into_inner(),
        )
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    tracing::debug!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

async fn get_users(State(state): State<AppState>) -> impl IntoResponse {
    let users = state.get_all_users();
    Json(users)
}

async fn get_user(
    Path(id): Path<u64>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, StatusCode> {
    let user = state.get_user(id).ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(user))
}

async fn create_user(
    State(state): State<AppState>,
    Json(input): Json<CreateUser>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let user = state
        .create_user(input.name, input.email, input.password)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok((StatusCode::CREATED, Json(user)))
}

async fn update_user(
    Path(id): Path<u64>,
    State(state): State<AppState>,
    Json(input): Json<UpdateUser>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let user = state
        .update_user(id, input)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(user))
}

async fn delete_user(Path(id): Path<u64>, State(state): State<AppState>) -> impl IntoResponse {
    if state.delete_user(id) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

async fn health_check(State(state): State<AppState>) -> impl IntoResponse {
    if state.health() {
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "healthy",
                "timestamp": Utc::now()
            })),
        )
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "status": "unhealthy",
                "timestamp": Utc::now()
            })),
        )
    }
}
