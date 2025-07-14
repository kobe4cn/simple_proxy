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
    #[serde(skip_serializing, skip_deserializing)]
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
        let password_hash = hash_password(&self.inner.argon2, password)?;

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
            let password_hash = hash_password(&self.inner.argon2, password)?;
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
fn hash_password(argon2: &Argon2<'static>, password: String) -> Result<String, BoxError> {
    let salt = SaltString::generate(&mut OsRng);
    let password_hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| e.to_string())?
        .to_string();
    Ok(password_hash)
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
            "/users/{id}",
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

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3001")
        .await
        .unwrap();
    tracing::info!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

async fn get_users(State(state): State<AppState>) -> impl IntoResponse {
    let users = state.get_all_users();
    tracing::info!("get_users: {:?}", users);
    Json(users)
}

async fn get_user(
    Path(id): Path<u64>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, StatusCode> {
    let user = state.get_user(id).ok_or(StatusCode::NOT_FOUND)?;
    tracing::info!("get_user: {:?}", user);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_state_new() {
        let state = AppState::new();
        assert_eq!(state.get_all_users().len(), 0);
        assert!(state.health());
    }

    #[test]
    fn test_create_user() {
        let state = AppState::new();

        let user = state
            .create_user(
                "Alice".to_string(),
                "alice@example.com".to_string(),
                "password123".to_string(),
            )
            .expect("Failed to create user");

        assert_eq!(user.id, 1);
        assert_eq!(user.name, "Alice");
        assert_eq!(user.email, "alice@example.com");
        assert!(!user.password.is_empty());
        assert!(user.password.starts_with("$argon2"));
        assert_eq!(state.get_all_users().len(), 1);
    }

    #[test]
    fn test_create_multiple_users() {
        let state = AppState::new();

        let user1 = state
            .create_user(
                "Alice".to_string(),
                "alice@example.com".to_string(),
                "password123".to_string(),
            )
            .expect("Failed to create user1");

        let user2 = state
            .create_user(
                "Bob".to_string(),
                "bob@example.com".to_string(),
                "password456".to_string(),
            )
            .expect("Failed to create user2");

        assert_eq!(user1.id, 1);
        assert_eq!(user2.id, 2);
        assert_eq!(state.get_all_users().len(), 2);
    }

    #[test]
    fn test_get_user() {
        let state = AppState::new();

        let created_user = state
            .create_user(
                "Alice".to_string(),
                "alice@example.com".to_string(),
                "password123".to_string(),
            )
            .expect("Failed to create user");

        let retrieved_user = state.get_user(created_user.id).expect("User not found");

        assert_eq!(retrieved_user.id, created_user.id);
        assert_eq!(retrieved_user.name, created_user.name);
        assert_eq!(retrieved_user.email, created_user.email);
        assert_eq!(retrieved_user.password, created_user.password);
    }

    #[test]
    fn test_get_nonexistent_user() {
        let state = AppState::new();
        assert!(state.get_user(999).is_none());
    }

    #[test]
    fn test_update_user() {
        let state = AppState::new();

        let user = state
            .create_user(
                "Alice".to_string(),
                "alice@example.com".to_string(),
                "password123".to_string(),
            )
            .expect("Failed to create user");

        let update = UpdateUser {
            name: Some("Alice Updated".to_string()),
            email: Some("alice.updated@example.com".to_string()),
            password: Some("newpassword123".to_string()),
        };

        let updated_user = state
            .update_user(user.id, update)
            .expect("Failed to update user");

        assert_eq!(updated_user.id, user.id);
        assert_eq!(updated_user.name, "Alice Updated");
        assert_eq!(updated_user.email, "alice.updated@example.com");
        assert!(!updated_user.password.is_empty());
        assert!(updated_user.password.starts_with("$argon2"));
        assert!(updated_user.updated_at > user.updated_at);
    }

    #[test]
    fn test_update_user_partial() {
        let state = AppState::new();

        let user = state
            .create_user(
                "Alice".to_string(),
                "alice@example.com".to_string(),
                "password123".to_string(),
            )
            .expect("Failed to create user");

        let original_password = user.password.clone();
        let original_updated_at = user.updated_at;

        let update = UpdateUser {
            name: Some("Alice Updated".to_string()),
            email: None,
            password: None,
        };

        let updated_user = state
            .update_user(user.id, update)
            .expect("Failed to update user");

        assert_eq!(updated_user.id, user.id);
        assert_eq!(updated_user.name, "Alice Updated");
        assert_eq!(updated_user.email, user.email); // 未改变
        assert_eq!(updated_user.password, original_password); // 未改变
        assert!(updated_user.updated_at > original_updated_at);
    }

    #[test]
    fn test_update_nonexistent_user() {
        let state = AppState::new();

        let update = UpdateUser {
            name: Some("Alice".to_string()),
            email: None,
            password: None,
        };

        let result = state.update_user(999, update);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "User not found");
    }

    #[test]
    fn test_delete_user() {
        let state = AppState::new();

        let user = state
            .create_user(
                "Alice".to_string(),
                "alice@example.com".to_string(),
                "password123".to_string(),
            )
            .expect("Failed to create user");

        assert_eq!(state.get_all_users().len(), 1);

        let deleted = state.delete_user(user.id);
        assert!(deleted);
        assert_eq!(state.get_all_users().len(), 0);
        assert!(state.get_user(user.id).is_none());
    }

    #[test]
    fn test_delete_nonexistent_user() {
        let state = AppState::new();
        let deleted = state.delete_user(999);
        assert!(!deleted);
    }

    #[test]
    fn test_get_all_users() {
        let state = AppState::new();

        assert_eq!(state.get_all_users().len(), 0);

        let user1 = state
            .create_user(
                "Alice".to_string(),
                "alice@example.com".to_string(),
                "password123".to_string(),
            )
            .expect("Failed to create user1");

        let user2 = state
            .create_user(
                "Bob".to_string(),
                "bob@example.com".to_string(),
                "password456".to_string(),
            )
            .expect("Failed to create user2");

        let all_users = state.get_all_users();
        assert_eq!(all_users.len(), 2);

        let ids: Vec<u64> = all_users.iter().map(|u| u.id).collect();
        assert!(ids.contains(&user1.id));
        assert!(ids.contains(&user2.id));
    }

    #[test]
    fn test_password_hashing() {
        let state = AppState::new();

        let user1 = state
            .create_user(
                "Alice".to_string(),
                "alice@example.com".to_string(),
                "password123".to_string(),
            )
            .expect("Failed to create user1");

        let user2 = state
            .create_user(
                "Bob".to_string(),
                "bob@example.com".to_string(),
                "password123".to_string(),
            )
            .expect("Failed to create user2");

        // 相同密码应该产生不同的哈希值（因为不同的盐）
        assert_ne!(user1.password, user2.password);

        // 密码哈希应该以 $argon2 开头
        assert!(user1.password.starts_with("$argon2"));
        assert!(user2.password.starts_with("$argon2"));
    }

    #[test]
    fn test_user_serialization() {
        let state = AppState::new();

        let user = state
            .create_user(
                "Alice".to_string(),
                "alice@example.com".to_string(),
                "password123".to_string(),
            )
            .expect("Failed to create user");

        // 测试序列化（密码应该被跳过）
        let json = serde_json::to_string(&user).expect("Failed to serialize user");
        assert!(!json.contains("password123"));
        assert!(json.contains("Alice"));
        assert!(json.contains("alice@example.com"));

        // 测试反序列化
        let deserialized_user: User =
            serde_json::from_str(&json).expect("Failed to deserialize user");
        assert_eq!(deserialized_user.name, user.name);
        assert_eq!(deserialized_user.email, user.email);
        assert_eq!(deserialized_user.password, ""); // 密码字段为空
    }

    #[test]
    fn test_concurrent_user_creation() {
        use std::sync::Arc;
        use std::thread;

        let state = Arc::new(AppState::new());
        let mut handles = vec![];

        // 创建多个线程同时创建用户
        for i in 0..10 {
            let state_clone = Arc::clone(&state);
            let handle = thread::spawn(move || {
                state_clone
                    .create_user(
                        format!("User{}", i),
                        format!("user{}@example.com", i),
                        "password123".to_string(),
                    )
                    .expect("Failed to create user")
            });
            handles.push(handle);
        }

        // 等待所有线程完成
        let users: Vec<User> = handles
            .into_iter()
            .map(|h| h.join().expect("Thread failed"))
            .collect();

        // 验证所有用户都被创建
        assert_eq!(users.len(), 10);
        assert_eq!(state.get_all_users().len(), 10);

        // 验证ID是唯一的
        let mut ids: Vec<u64> = users.iter().map(|u| u.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 10);
    }
}
