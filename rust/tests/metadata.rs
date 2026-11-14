use lenso_app::{CancellationToken, Context, Executor, Principal};
use lenso_auth_source::{
    application::{
        AuthResources, auth, create_auth, passwords::ScryptHasher, sqlite::SqliteAuthStore,
    },
    file_local::LocalBlobs,
    file_sqlite::SqliteFiles,
    files::{self, Files},
};
use serde_json::{Value, json};
use tokio::io::AsyncReadExt;
fn context<R>(resources: R) -> Context<R> {
    Context {
        call_id: "metadata".into(),
        resources,
        principal: Some(Principal {
            subject: "alice".into(),
            scopes: vec![],
        }),
        cancellation: CancellationToken::new(),
    }
}
#[tokio::test]
async fn auth_sdk_descriptions_and_input_issues_preserve_native_calls() {
    let directory = tempfile::tempdir().unwrap();
    let app = create_auth(&directory.path().join("auth.sqlite"), "staff.auth").unwrap();
    app.store.migrate().await.unwrap();
    let expected: Value =
        serde_json::from_str(include_str!("../../contracts/auth/metadata.json")).unwrap();
    let module = app
        .actions
        .describe::<SqliteAuthStore, ScryptHasher>("staff.auth");
    assert_eq!(
        serde_json::to_value(module.metadata()).unwrap(),
        expected["module"]
    );
    for descriptor in module.describe_actions() {
        assert_eq!(
            serde_json::to_value(descriptor.metadata).unwrap(),
            expected["actions"][descriptor.id.rsplit('.').next().unwrap()]
        );
    }
    let ctx = context(app.resources);
    let ex = Executor::new(|_| {});
    for (input, path) in [
        (
            json!({"name":1,"password":"private-password"}),
            json!(["name"]),
        ),
        (
            json!({"name":"Alice","password":"short"}),
            json!(["password"]),
        ),
        (
            json!({"name":"Alice","password":"private-password","private-key":true}),
            json!([]),
        ),
    ] {
        let error = ex
            .call_json(&app.actions.register, &ctx, input)
            .await
            .unwrap_err();
        let public = serde_json::to_value(error.public()).unwrap();
        assert_eq!(public["code"], "invalid_input");
        assert_eq!(public["issues"][0]["path"], path);
        assert!(!public.to_string().contains("private-"));
    }
    let input = auth::Credentials {
        name: "NativeUser".into(),
        password: "native-secret-password".into(),
    };
    let created = ex.call(&app.actions.register, &ctx, input).await.unwrap();
    assert_eq!(created.name, "nativeuser");
    let _: &AuthResources = &ctx.resources;
}
#[tokio::test]
async fn files_sdk_actions_preserve_owned_streams_and_existing_files_methods() {
    let directory = tempfile::tempdir().unwrap();
    let store = SqliteFiles::open(&directory.path().join("files.sqlite"), "files").unwrap();
    store.migrate().await.unwrap();
    let resources = Files::new(
        store,
        LocalBlobs::open(directory.path(), "files").unwrap(),
        8,
    )
    .unwrap();
    let actions = files::Actions::new("private.files").unwrap();
    let expected: Value =
        serde_json::from_str(include_str!("../../contracts/files/metadata.json")).unwrap();
    let module = actions.describe::<SqliteFiles, LocalBlobs>("private.files");
    assert_eq!(
        serde_json::to_value(module.metadata()).unwrap(),
        expected["module"]
    );
    for descriptor in module.describe_actions() {
        assert_eq!(
            serde_json::to_value(descriptor.metadata).unwrap(),
            expected["actions"][descriptor.id.rsplit('.').next().unwrap()]
        );
    }
    let mut ctx = context(resources);
    let ex = Executor::new(|_| {});
    let input =
        || files::UploadInput::new("native".into(), Box::new(std::io::Cursor::new(b"native")));
    ctx.principal = None;
    let error = ex.call(&actions.upload, &ctx, input()).await.unwrap_err();
    assert_eq!(error.code(), lenso_app::ErrorCode::Unauthenticated);
    ctx.principal = Some(Principal {
        subject: "alice".into(),
        scopes: vec![],
    });
    let invalid = files::UploadInput::new("../private".into(), Box::new(tokio::io::empty()));
    let error = ex.call(&actions.upload, &ctx, invalid).await.unwrap_err();
    assert_eq!(
        serde_json::to_value(error.public()).unwrap()["issues"],
        json!([{"path":["name"],"code":"invalid_value"}])
    );
    let file = ex.call(&actions.upload, &ctx, input()).await.unwrap();
    assert_eq!(
        ctx.resources
            .stat(ctx.principal.as_ref(), &file.id)
            .await
            .unwrap()
            .id,
        file.id
    );
    let (_, mut reader) = ex
        .call(
            &actions.download,
            &ctx,
            files::FileInput {
                id: file.id.clone(),
            },
        )
        .await
        .unwrap();
    let mut bytes = vec![];
    reader.read_to_end(&mut bytes).await.unwrap();
    assert_eq!(bytes, b"native");
    let error = ctx
        .resources
        .list(ctx.principal.as_ref(), "private-cursor", 0)
        .await
        .unwrap_err();
    assert_eq!(
        serde_json::to_value(error.public()).unwrap()["issues"],
        json!([{"path":["after"],"code":"invalid_value"},{"path":["limit"],"code":"out_of_range"}])
    );
    let error = ex
        .call(
            &actions.upload,
            &ctx,
            files::UploadInput::new("large".into(), Box::new(std::io::Cursor::new(vec![0; 9]))),
        )
        .await
        .unwrap_err();
    assert_eq!(
        serde_json::to_value(error.public()).unwrap()["issues"],
        json!([{"path":["source"],"code":"out_of_range"}])
    );
    assert_eq!(
        ctx.resources
            .list(ctx.principal.as_ref(), "", 20)
            .await
            .unwrap()
            .len(),
        1
    );
    ctx.principal = Some(Principal {
        subject: "bob".into(),
        scopes: vec![],
    });
    assert_eq!(
        ex.call(
            &actions.stat,
            &ctx,
            files::FileInput {
                id: file.id.clone()
            }
        )
        .await
        .unwrap_err()
        .code(),
        lenso_app::ErrorCode::NotFound
    );
    ctx.principal = Some(Principal {
        subject: "alice".into(),
        scopes: vec![],
    });
    ex.call(&actions.delete, &ctx, files::FileInput { id: file.id })
        .await
        .unwrap();
    assert!(
        ctx.resources
            .list(ctx.principal.as_ref(), "", 20)
            .await
            .unwrap()
            .is_empty()
    );
}
