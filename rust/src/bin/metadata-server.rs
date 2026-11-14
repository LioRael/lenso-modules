//! Acceptance host for optional SDK descriptions and native streaming Actions.
use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{Query, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures_util::TryStreamExt;
use lenso_app::{
    ActionError, CancellationToken, Catalog, Context, ErrorCode, Executor, InputIssue,
    InputIssueCode, require_principal,
};
use lenso_auth_source::{
    application::{
        AuthApplication, auth::random_hex, create_auth, passwords::ScryptHasher,
        sqlite::SqliteAuthStore,
    },
    file_local::LocalBlobs,
    file_sqlite::SqliteFiles,
    files::{self, Files},
};
use serde_json::{Value, json};
use std::{collections::HashMap, path::Path, sync::Arc};
#[derive(Clone)]
struct Host {
    auth: AuthApplication,
    files: Files<SqliteFiles, LocalBlobs>,
    actions: files::Actions,
}
fn failure(error: ActionError) -> Response {
    let status = match error.code() {
        ErrorCode::InvalidInput => 400,
        ErrorCode::Unauthenticated => 401,
        ErrorCode::Forbidden => 403,
        ErrorCode::NotFound => 404,
        ErrorCode::Conflict => 409,
        _ => 500,
    };
    (
        StatusCode::from_u16(status).expect("fixed status"),
        Json(error.public()),
    )
        .into_response()
}
async fn operation(host: &Host, request: Request) -> Result<Response, ActionError> {
    let path = request.uri().path().to_owned();
    let method = request.method().clone();
    let query = Query::<HashMap<String, String>>::try_from_uri(request.uri())
        .map_err(|_| ActionError::new(ErrorCode::InvalidInput, "Invalid query"))?
        .0;
    let token = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let principal = host.auth.resources.principal(token).await?;
    let auth_context = Context {
        call_id: random_hex(16)?,
        resources: host.auth.resources.clone(),
        principal: principal.clone(),
        cancellation: CancellationToken::new(),
    };
    let executor = Executor::new(|_| {});
    if path == "/manifest" {
        require_principal(principal.as_ref())?;
        let mut catalog = Catalog::new();
        catalog.register(
            host.auth
                .actions
                .describe::<SqliteAuthStore, ScryptHasher>("auth"),
        )?;
        catalog.register(host.actions.describe::<SqliteFiles, LocalBlobs>("files"))?;
        return Ok(Json(
            json!({"modules":catalog.describe(),"actions":catalog.describe_actions()}),
        )
        .into_response());
    }
    if path == "/auth/register" || path == "/auth/login" {
        let body = to_bytes(request.into_body(), 4096)
            .await
            .map_err(|_| ActionError::new(ErrorCode::InvalidInput, "Body too large"))?;
        let input: Value = serde_json::from_slice(&body).map_err(|cause| {
            ActionError::new(ErrorCode::InvalidInput, "Invalid JSON")
                .with_source(cause)
                .with_issues(vec![InputIssue {
                    path: vec![],
                    code: InputIssueCode::InvalidFormat,
                }])
        })?;
        let output = if path.ends_with("register") {
            executor
                .call_json(&host.auth.actions.register, &auth_context, input)
                .await?
        } else {
            executor
                .call_json(&host.auth.actions.login, &auth_context, input)
                .await?
        };
        return Ok(Json(output).into_response());
    }
    let context = Context {
        call_id: auth_context.call_id,
        resources: host.files.clone(),
        principal,
        cancellation: CancellationToken::new(),
    };
    if path == "/files" && method == "POST" {
        let stream = request
            .into_body()
            .into_data_stream()
            .map_err(std::io::Error::other);
        let source = Box::new(tokio_util::io::StreamReader::new(stream));
        let input = files::UploadInput::new(query.get("name").cloned().unwrap_or_default(), source);
        return Ok((
            StatusCode::CREATED,
            Json(executor.call(&host.actions.upload, &context, input).await?),
        )
            .into_response());
    }
    if path == "/files" && method == "GET" {
        let limit = query
            .get("limit")
            .map(|v| v.parse::<u32>())
            .transpose()
            .map_err(|_| ActionError::new(ErrorCode::InvalidInput, "Invalid page"))?;
        let input = files::Page {
            after: query.get("after").cloned(),
            limit,
        };
        return Ok(
            Json(json!({"files":executor.call(&host.actions.list,&context,input).await?}))
                .into_response(),
        );
    }
    let id = path.split('/').nth(2).unwrap_or("");
    let input = files::FileInput { id: id.into() };
    if path == format!("/files/{id}/content") && method == "GET" {
        let (_, reader) = executor
            .call(&host.actions.download, &context, input)
            .await?;
        return Ok(Response::new(Body::from_stream(
            tokio_util::io::ReaderStream::new(reader),
        )));
    }
    if path == format!("/files/{id}") && method == "DELETE" {
        executor.call(&host.actions.delete, &context, input).await?;
        return Ok(Json(json!({"deleted":true})).into_response());
    }
    if path == format!("/files/{id}") && method == "GET" {
        return Ok(Json(executor.call(&host.actions.stat, &context, input).await?).into_response());
    }
    Ok(StatusCode::NOT_FOUND.into_response())
}
async fn handle(State(host): State<Arc<Host>>, request: Request) -> Response {
    match operation(&host, request).await {
        Ok(response) => response,
        Err(error) => failure(error),
    }
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::var("LENSO_METADATA_ROOT")?;
    let root = Path::new(&root);
    let auth = create_auth(&root.join("auth.sqlite"), "auth")?;
    auth.store.migrate().await?;
    let store = SqliteFiles::open(&root.join("files.sqlite"), "files")?;
    store.migrate().await?;
    let files = Files::new(store, LocalBlobs::open(root, "files")?, 8)?;
    let host = Arc::new(Host {
        auth,
        files,
        actions: files::Actions::new("files")?,
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    println!("LISTENING {}", listener.local_addr()?.port());
    axum::serve(listener, Router::new().fallback(handle).with_state(host)).await?;
    Ok(())
}
