# Example Server

请使用 axum 实现以下的服务器：

1. GET /users/{id} 返回用户信息
2. GET /users 返回所有用户信息
3. POST /users 创建一个用户
4. PUT /users/{id} 更新一个用户
5. DELETE /users/{id} 删除一个用户
6. GET /health 返回健康状态

User schema:

```rust
struct User {
    id: u64,
    name: String,
    email: String,
    //argon2 hash password. should not be response to client.
    password: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}
```

App state:
使用 dashmap 存储用户信息

```rust
struct AppState {
    inner: Arc<AppStateInner>,
}

struct AppStateInner {
    next_id: AtomicU64,
    users: DashMap<u64, User>,
    argon2: Argon2<'static>,
}
```

App state 需要支持以下方法:

- `new` 创建一个 AppState
- `get_user` 获取一个用户
- `create_user` 创建一个用户
- `update_user` 更新一个用户
- `delete_user` 删除一个用户
- `health` 返回健康状态


## Dependencies

项目将使用以下的依赖:

- axum
- dashmap
- argon2
- chrono
- serde
- serde_json
- tokio, features: full



### Argon2

example:

```rust
use argon2::{
    password_hash::{
        rand_core::OsRng,
        PasswordHash, PasswordHasher, PasswordVerifier, SaltString
    },
    Argon2
};

let password = b"hunter42"; // Bad password; don't actually use!
let salt = SaltString::generate(&mut OsRng);

// Argon2 with default params (Argon2id v19)
let argon2 = Argon2::default();

// Hash password to PHC string ($argon2id$v=19$...)
let password_hash = argon2.hash_password(password, &salt)?.to_string();

// Verify password against PHC string.
//
// NOTE: hash params from `parsed_hash` are used instead of what is configured in the
// `Argon2` instance.
let parsed_hash = PasswordHash::new(&password_hash)?;
assert!(Argon2::default().verify_password(password, &parsed_hash).is_ok());
```

### axum

example:

```rust
use axum::{
    error_handling::HandleErrorLayer,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, patch},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::Duration,
};
use tower::{BoxError, ServiceBuilder};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                format!("{}=debug,tower_http=debug", env!("CARGO_CRATE_NAME")).into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let db = Db::default();

    // Compose the routes
    let app = Router::new()
        .route("/todos", get(todos_index).post(todos_create))
        .route("/todos/{id}", patch(todos_update).delete(todos_delete))
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
                .timeout(Duration::from_secs(10))
                .layer(TraceLayer::new_for_http())
                .into_inner(),
        )
        .with_state(db);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    tracing::debug!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

// The query parameters for todos index
#[derive(Debug, Deserialize, Default)]
pub struct Pagination {
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

async fn todos_index(pagination: Query<Pagination>, State(db): State<Db>) -> impl IntoResponse {
    let todos = db.read().unwrap();

    let todos = todos
        .values()
        .skip(pagination.offset.unwrap_or(0))
        .take(pagination.limit.unwrap_or(usize::MAX))
        .cloned()
        .collect::<Vec<_>>();

    Json(todos)
}

#[derive(Debug, Deserialize)]
struct CreateTodo {
    text: String,
}

async fn todos_create(State(db): State<Db>, Json(input): Json<CreateTodo>) -> impl IntoResponse {
    let todo = Todo {
        id: Uuid::new_v4(),
        text: input.text,
        completed: false,
    };

    db.write().unwrap().insert(todo.id, todo.clone());

    (StatusCode::CREATED, Json(todo))
}

#[derive(Debug, Deserialize)]
struct UpdateTodo {
    text: Option<String>,
    completed: Option<bool>,
}

async fn todos_update(
    Path(id): Path<Uuid>,
    State(db): State<Db>,
    Json(input): Json<UpdateTodo>,
) -> Result<impl IntoResponse, StatusCode> {
    let mut todo = db
        .read()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or(StatusCode::NOT_FOUND)?;

    if let Some(text) = input.text {
        todo.text = text;
    }

    if let Some(completed) = input.completed {
        todo.completed = completed;
    }

    db.write().unwrap().insert(todo.id, todo.clone());

    Ok(Json(todo))
}

async fn todos_delete(Path(id): Path<Uuid>, State(db): State<Db>) -> impl IntoResponse {
    if db.write().unwrap().remove(&id).is_some() {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

type Db = Arc<RwLock<HashMap<Uuid, Todo>>>;

#[derive(Debug, Serialize, Clone)]
struct Todo {
    id: Uuid,
    text: String,
    completed: bool,
}
```
