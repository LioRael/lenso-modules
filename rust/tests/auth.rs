use lenso_app::{CallRecord, CancellationToken, Context, ErrorCode, Executor, Principal};
use lenso_auth_source::application::{
    AuthApplication, AuthResources,
    auth::{Auth, PasswordHasher, token_hash},
    create_auth,
    passwords::ScryptHasher,
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicI64, Ordering},
};
async fn fixture(path: &std::path::Path, realm: &str) -> (AuthApplication, Arc<AtomicI64>) {
    let mut app = create_auth(path, realm).unwrap();
    app.store.migrate().await.unwrap();
    let clock = Arc::new(AtomicI64::new(1_700_000_000_000));
    let now = clock.clone();
    app.resources = Auth::with_clock(
        app.store.clone(),
        ScryptHasher::default(),
        60_000,
        move || Ok(now.load(Ordering::SeqCst)),
    )
    .unwrap();
    (app, clock)
}
fn context(app: &AuthApplication, principal: Option<Principal>) -> Context<AuthResources> {
    Context {
        call_id: "test".into(),
        principal,
        resources: app.resources.clone(),
        cancellation: CancellationToken::new(),
    }
}
fn executor() -> Executor<impl Fn(CallRecord) + Sync> {
    Executor::new(|_: CallRecord| {})
}
fn input() -> Value {
    json!({"name":"Alice_One","password":"correct horse battery"})
}
#[tokio::test]
async fn shared_input_contract_and_public_outputs() {
    let dir = tempfile::tempdir().unwrap();
    let (app, _) = fixture(&dir.path().join("auth.sqlite"), "auth").await;
    let ctx = context(&app, None);
    let ex = executor();
    let cases: Value =
        serde_json::from_str(include_str!("../../contracts/auth/behavior.json")).unwrap();
    for value in cases["invalid"].as_array().unwrap() {
        assert_eq!(
            ex.call_json(&app.actions.register, &ctx, value.clone())
                .await
                .unwrap_err()
                .code(),
            ErrorCode::InvalidInput
        );
    }
    for value in cases["valid"].as_array().unwrap() {
        let user = ex
            .call_json(&app.actions.register, &ctx, value.clone())
            .await
            .unwrap();
        assert_eq!(
            user["name"],
            value["name"].as_str().unwrap().to_ascii_lowercase()
        );
        assert_eq!(user.as_object().unwrap().len(), 2);
    }
    assert_eq!(
        ex.call_json(
            &app.actions.register,
            &ctx,
            json!({"name":"valid","password":"correct horse battery","subject":"attacker"})
        )
        .await
        .unwrap_err()
        .code(),
        ErrorCode::InvalidInput
    );
}
#[tokio::test]
async fn persistent_sessions_expiry_and_revocation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("auth.sqlite");
    let (app, clock) = fixture(&path, "auth").await;
    let ex = executor();
    let ctx = context(&app, None);
    let user = ex
        .call_json(&app.actions.register, &ctx, input())
        .await
        .unwrap();
    let session = ex
        .call_json(&app.actions.login, &ctx, input())
        .await
        .unwrap();
    let token = session["token"].as_str().unwrap();
    let principal = app.resources.principal(Some(token)).await.unwrap().unwrap();
    assert_eq!(principal.subject, user["id"].as_str().unwrap());
    assert!(principal.scopes.is_empty());
    let trusted = context(&app, Some(principal));
    assert_eq!(
        ex.call_json(&app.actions.session, &trusted, Value::Null)
            .await
            .unwrap(),
        user
    );
    assert_eq!(
        ex.call_json(&app.actions.session, &ctx, Value::Null)
            .await
            .unwrap_err()
            .code(),
        ErrorCode::Unauthenticated
    );
    let db = rusqlite::Connection::open(&path).unwrap();
    let stored: String = db
        .query_row("SELECT token_hash FROM lenso_auth_sessions", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(stored, token_hash(token));
    assert_ne!(stored, token);
    let hash: String = db
        .query_row("SELECT password_hash FROM lenso_auth_users", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(!hash.contains("correct horse battery"));
    let (reopened, _) = fixture(&path, "auth").await;
    assert!(
        reopened
            .resources
            .principal(Some(token))
            .await
            .unwrap()
            .is_some()
    );
    ex.call_json(&app.actions.logout, &trusted, json!({"token":token}))
        .await
        .unwrap();
    assert!(
        app.resources
            .principal(Some(token))
            .await
            .unwrap()
            .is_none()
    );
    let session = ex
        .call_json(&app.actions.login, &ctx, input())
        .await
        .unwrap();
    clock.store(session["expiresMs"].as_i64().unwrap(), Ordering::SeqCst);
    assert!(
        app.resources
            .principal(session["token"].as_str())
            .await
            .unwrap()
            .is_none()
    );
}
#[tokio::test]
async fn invalid_credentials_are_indistinguishable_and_hash_parameters_are_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let (app, _) = fixture(&dir.path().join("auth.sqlite"), "auth").await;
    let ex = executor();
    let ctx = context(&app, None);
    ex.call_json(&app.actions.register, &ctx, input())
        .await
        .unwrap();
    for name in ["Alice_One", "missing"] {
        let e = ex
            .call_json(
                &app.actions.login,
                &ctx,
                json!({"name":name,"password":"incorrect password"}),
            )
            .await
            .unwrap_err();
        assert_eq!(e.code(), ErrorCode::Unauthenticated);
        assert_eq!(e.message(), "Invalid credentials");
    }
    assert_eq!(
        app.resources
            .passwords
            .verify("password".into(), "scrypt$v1$999999999$8$1$bad")
            .await
            .unwrap_err()
            .code(),
        ErrorCode::Internal
    );
}
#[tokio::test]
async fn realms_and_connections_preserve_uniqueness() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("auth.sqlite");
    let (a, _) = fixture(&path, "customer").await;
    let (b, _) = fixture(&path, "staff").await;
    let ex = executor();
    let ua = ex
        .call_json(&a.actions.register, &context(&a, None), input())
        .await
        .unwrap();
    let ub = ex
        .call_json(&b.actions.register, &context(&b, None), input())
        .await
        .unwrap();
    assert_ne!(ua["id"], ub["id"]);
    let session = ex
        .call_json(&a.actions.login, &context(&a, None), input())
        .await
        .unwrap();
    let token = session["token"].as_str().unwrap();
    assert!(b.resources.principal(Some(token)).await.unwrap().is_none());
    let trusted = context(
        &b,
        Some(Principal {
            subject: ub["id"].as_str().unwrap().into(),
            scopes: vec![],
        }),
    );
    assert_eq!(
        ex.call_json(&b.actions.logout, &trusted, json!({"token":token}))
            .await
            .unwrap_err()
            .code(),
        ErrorCode::Unauthenticated
    );
    let (c, _) = fixture(&path, "customer").await;
    let context_a = context(&a, None);
    let context_c = context(&c, None);
    let outcomes = tokio::join!(
        ex.call_json(
            &a.actions.register,
            &context_a,
            json!({"name":"racing","password":"correct horse battery"})
        ),
        ex.call_json(
            &c.actions.register,
            &context_c,
            json!({"name":"RACING","password":"correct horse battery"})
        )
    );
    assert_ne!(outcomes.0.is_ok(), outcomes.1.is_ok());
    let error = outcomes.0.err().or(outcomes.1.err()).unwrap();
    assert_eq!(error.code(), ErrorCode::Conflict);
}
#[tokio::test]
async fn future_schema_is_rejected_without_data_loss() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("auth.sqlite");
    let (app, _) = fixture(&path, "auth").await;
    executor()
        .call_json(&app.actions.register, &context(&app, None), input())
        .await
        .unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute("INSERT INTO lenso_auth_migrations VALUES(2)", [])
        .unwrap();
    assert_eq!(
        app.store.migrate().await.unwrap_err().code(),
        ErrorCode::Internal
    );
    let count: i64 = db
        .query_row("SELECT COUNT(*) FROM lenso_auth_users", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}
