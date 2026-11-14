fn describe<R, A: lenso_app::Action<R>>(action: &A, _: &Context<R>) -> lenso_app::ActionDescription {
    lenso_app::describe_action::<R, A>(action)
}
let description = describe(&app.actions.register, &context);
assert_eq!(description.metadata.unwrap().input_schema.unwrap()["properties"]["password"]["writeOnly"], true);
let error = executor.call_json(&app.actions.register, &context, serde_json::json!({"name":17,"password":"private-value"})).await.unwrap_err();
assert_eq!(error.code(), lenso_app::ErrorCode::InvalidInput);
assert_eq!(error.issues()[0].path, vec![lenso_app::InputSegment::Field("name".into())]);
assert!(!serde_json::to_string(&error.public())?.contains("private-value"));
