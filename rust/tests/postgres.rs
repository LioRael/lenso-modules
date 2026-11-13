use lenso_app::{CallRecord, CancellationToken, Context, ErrorCode, Executor};
use lenso_auth_source::application::{
    auth::{Actions, Auth, AuthStore, Credentials, TokenInput, token_hash},
    passwords::ScryptHasher,
    postgres::PostgresAuthStore,
};

#[tokio::test]
#[ignore = "requires AUTH_POSTGRES_URL pointing to a disposable PostgreSQL database"]
async fn real_postgres_contract() {
    let url = std::env::var("AUTH_POSTGRES_URL").expect("AUTH_POSTGRES_URL is required");
    let realm = format!(
        "rust_{}",
        lenso_auth_source::application::auth::random_hex(8).unwrap()
    );
    let a = PostgresAuthStore::connect(&url, &realm).await.unwrap();
    let b = PostgresAuthStore::connect(&url, &realm).await.unwrap();
    let other = PostgresAuthStore::connect(&url, &format!("{realm}_other"))
        .await
        .unwrap();
    let (ma, mb) = tokio::join!(a.migrate(), b.migrate());
    ma.unwrap();
    mb.unwrap();
    a.check_schema().await.unwrap();
    let resources = Auth::with_clock(a.clone(), ScryptHasher::default(), 60000, || {
        Ok(1700000000000)
    })
    .unwrap();
    let actions = Actions::new(&realm).unwrap();
    let mut ctx = Context {
        call_id: "pg-test".into(),
        principal: None,
        resources,
        cancellation: CancellationToken::new(),
    };
    let ex = Executor::new(|_: CallRecord| {});
    let credentials = || Credentials {
        name: "Alice".into(),
        password: "correct horse battery".into(),
    };
    let user = ex
        .call(&actions.register, &ctx, credentials())
        .await
        .unwrap();
    let race = tokio::join!(
        a.create_user("racing".into(), "hash".into()),
        b.create_user("racing".into(), "hash".into())
    );
    assert_ne!(race.0.is_ok(), race.1.is_ok());
    assert_eq!(
        race.0.err().or(race.1.err()).unwrap().code(),
        ErrorCode::Conflict
    );
    let session = ex.call(&actions.login, &ctx, credentials()).await.unwrap();
    let hash = token_hash(&session.token);
    assert_ne!(hash, session.token);
    assert!(b.session(&hash, 1700000000000).await.unwrap().is_some());
    assert!(other.session(&hash, 1700000000000).await.unwrap().is_none());
    other
        .create_user("alice".into(), "hash".into())
        .await
        .unwrap();
    other.revoke_session(&hash, &user.id).await.unwrap();
    assert!(b.session(&hash, 1700000000000).await.unwrap().is_some());
    assert!(
        b.session(&hash, session.expires_ms)
            .await
            .unwrap()
            .is_none()
    );
    ctx.principal = ctx.resources.principal(Some(&session.token)).await.unwrap();
    assert_eq!(ex.call(&actions.session, &ctx, ()).await.unwrap(), user);
    ex.call(
        &actions.logout,
        &ctx,
        TokenInput {
            token: session.token,
        },
    )
    .await
    .unwrap();
    assert!(b.session(&hash, 1700000000000).await.unwrap().is_none());
    assert_eq!(
        b.save_session("bad-fk".into(), "missing".into(), 0)
            .await
            .unwrap_err()
            .code(),
        ErrorCode::Internal
    );
    let persisted = ex.call(&actions.login, &ctx, credentials()).await.unwrap();
    drop(ctx);
    drop(a);
    drop(b);
    let reopened = PostgresAuthStore::connect(&url, &realm).await.unwrap();
    assert!(
        reopened
            .session(&token_hash(&persisted.token), 1700000000000)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        reopened
            .credential("alice")
            .await
            .unwrap()
            .unwrap()
            .password_hash
            .starts_with("scrypt$v1$")
    );
    let (admin, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
        .await
        .unwrap();
    let driver = tokio::spawn(connection);
    admin
        .execute("INSERT INTO lenso_auth_migrations(version) VALUES(2)", &[])
        .await
        .unwrap();
    let rejected = reopened.migrate().await;
    admin
        .execute("DELETE FROM lenso_auth_migrations WHERE version=2", &[])
        .await
        .unwrap();
    assert_eq!(rejected.unwrap_err().code(), ErrorCode::Internal);
    assert!(reopened.credential("alice").await.unwrap().is_some());
    reopened.check_schema().await.unwrap();
    driver.abort();
}
