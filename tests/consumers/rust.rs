use auth_consumer::generated::{i_61757468,auth::Credentials};
use lenso_app::{Executor,Context,CancellationToken,CallRecord};
fn main()->Result<(),Box<dyn std::error::Error>>{
 tokio::runtime::Builder::new_current_thread().build()?.block_on(async {
 let app=i_61757468(Some(std::path::Path::new("consumer.sqlite")))?;app.store.migrate().await?;
 let executor=Executor::new(|_:CallRecord|{});
 let mut context=Context{call_id:"consumer".into(),principal:None,resources:app.resources,cancellation:CancellationToken::new()};
 let user=executor.call(&app.actions.register,&context,Credentials{name:format!("consumer_{}",std::process::id()),password:"correct horse battery".into()}).await?;
 let session=executor.call(&app.actions.login,&context,Credentials{name:format!("consumer_{}",std::process::id()),password:"correct horse battery".into()}).await?;
 context.principal=context.resources.principal(Some(&session.token)).await?;
 assert_eq!(context.principal.as_ref().unwrap().subject,user.id);
 executor.call(&app.actions.logout,&context,auth_consumer::generated::auth::TokenInput{token:session.token.clone()}).await?;
 assert!(context.resources.principal(Some(&session.token)).await?.is_none());println!("AUTH_USER_ID={}",user.id);println!("AUTH_CONSUMER_OK");Ok(())
 })
}
