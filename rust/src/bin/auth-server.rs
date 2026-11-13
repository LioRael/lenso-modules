use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use lenso_app::{ActionError, CallRecord, CancellationToken, Context, ErrorCode, Executor};
use lenso_auth_source::application::{AuthApplication, auth::random_hex, create_auth};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};
#[derive(Clone)]
struct Host {
    app: AuthApplication,
    permits: Arc<tokio::sync::Semaphore>,
}
struct HttpFailure(StatusCode, Value);
impl From<(StatusCode, Json<Value>)> for HttpFailure {
    fn from((status, Json(body)): (StatusCode, Json<Value>)) -> Self {
        Self(status, body)
    }
}
impl IntoResponse for HttpFailure {
    fn into_response(self) -> Response {
        (self.0, Json(self.1)).into_response()
    }
}
fn failure(error: ActionError) -> HttpFailure {
    let status = match error.code() {
        ErrorCode::InvalidInput => StatusCode::BAD_REQUEST,
        ErrorCode::Unauthenticated => StatusCode::UNAUTHORIZED,
        ErrorCode::Forbidden => StatusCode::FORBIDDEN,
        ErrorCode::Conflict => StatusCode::CONFLICT,
        ErrorCode::NotFound => StatusCode::NOT_FOUND,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    let message = if error.code() == ErrorCode::Internal {
        "Authentication operation failed"
    } else {
        error.message()
    };
    (
        status,
        Json(json!({"code":error.code().as_str(),"message":message})),
    )
        .into()
}
async fn operation(host: Host, request: Request) -> Result<Value, HttpFailure> {
    let route = format!("{} {}", request.method(), request.uri().path());
    if !matches!(
        route.as_str(),
        "POST /auth/register" | "POST /auth/login" | "POST /auth/logout" | "GET /auth/session"
    ) {
        return Err((StatusCode::NOT_FOUND, Json(json!({"code":"not_found"}))).into());
    }
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_owned);
    let resources = host.app.resources;
    let context = Context {
        call_id: random_hex(16).map_err(failure)?,
        principal: resources
            .principal(token.as_deref())
            .await
            .map_err(failure)?,
        resources,
        cancellation: CancellationToken::new(),
    };
    let input = if matches!(route.as_str(), "POST /auth/register" | "POST /auth/login") {
        if request
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            != Some("application/json")
        {
            return Err((
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                Json(json!({"code":"invalid_input"})),
            )
                .into());
        }
        let body = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            to_bytes(request.into_body(), 4096),
        )
        .await
        .map_err(|_| -> HttpFailure {
            (
                StatusCode::REQUEST_TIMEOUT,
                Json(json!({"code":"invalid_input"})),
            )
                .into()
        })?
        .map_err(|_| -> HttpFailure {
            (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(json!({"code":"invalid_input"})),
            )
                .into()
        })?;
        serde_json::from_slice(&body).map_err(|_| -> HttpFailure {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"code":"invalid_input"})),
            )
                .into()
        })?
    } else {
        Value::Null
    };
    let executor = Executor::new(|_: CallRecord| {});
    let a = host.app.actions;
    let result = match route.as_str() {
        "POST /auth/register" => executor.call_json(&a.register, &context, input).await,
        "POST /auth/login" => executor.call_json(&a.login, &context, input).await,
        "POST /auth/logout" => {
            executor
                .call_json(
                    &a.logout,
                    &context,
                    json!({"token":token.unwrap_or_default()}),
                )
                .await
        }
        _ => executor.call_json(&a.session, &context, Value::Null).await,
    };
    result.map_err(failure)
}
async fn handle(State(host): State<Host>, request: Request) -> Response {
    let Ok(_permit) = host.permits.clone().try_acquire_owned() else {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"code":"capacity"})),
        )
            .into_response();
    };
    let mut response = match operation(host, request).await {
        Ok(value) => Json(value).into_response(),
        Err(response) => response.into_response(),
    };
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let filename = std::env::var("LENSO_AUTH_DB").unwrap_or_else(|_| "auth.sqlite".into());
    let app = create_auth(Path::new(&filename), "auth")?;
    if std::env::args().any(|v| v == "--migrate") {
        app.store.migrate().await?;
    }
    app.store.check_schema().await?;
    let listener = tokio::net::TcpListener::bind((
        std::net::Ipv4Addr::LOCALHOST,
        std::env::var("PORT")
            .unwrap_or_else(|_| "3001".into())
            .parse::<u16>()?,
    ))
    .await?;
    println!("LISTENING {}", listener.local_addr()?.port());
    axum::serve(
        listener,
        Router::new().fallback(handle).with_state(Host {
            app,
            permits: Arc::new(tokio::sync::Semaphore::new(4)),
        }),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    Ok(())
}
