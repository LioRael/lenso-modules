//! Password authentication and opaque sessions, independent of storage and hashing providers.
use lenso_app::{Action, ActionError, Context, ErrorCode, Module, Principal, require_principal};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    future::Future,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

/// Public account information, excluding credentials.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct User {
    pub id: String,
    pub name: String,
}
/// Credentials owned by the store; never returned by an Action.
pub struct Credential {
    pub user: User,
    pub password_hash: String,
}
/// Atomic account/session operations within one application realm.
pub trait AuthStore: Send + Sync {
    fn create_user(
        &self,
        name: String,
        password_hash: String,
    ) -> impl Future<Output = Result<User, ActionError>> + Send;
    fn credential(
        &self,
        name: &str,
    ) -> impl Future<Output = Result<Option<Credential>, ActionError>> + Send;
    fn user(&self, id: &str) -> impl Future<Output = Result<Option<User>, ActionError>> + Send;
    fn save_session(
        &self,
        token_hash: String,
        subject: String,
        expires_ms: i64,
    ) -> impl Future<Output = Result<(), ActionError>> + Send;
    fn session(
        &self,
        token_hash: &str,
        now_ms: i64,
    ) -> impl Future<Output = Result<Option<User>, ActionError>> + Send;
    fn revoke_session(
        &self,
        token_hash: &str,
        subject: &str,
    ) -> impl Future<Output = Result<(), ActionError>> + Send;
}
/// Encoded password hashing with bounded provider-defined work factors.
pub trait PasswordHasher: Send + Sync {
    fn hash(&self, password: String) -> impl Future<Output = Result<String, ActionError>> + Send;
    fn verify(
        &self,
        password: String,
        encoded: &str,
    ) -> impl Future<Output = Result<bool, ActionError>> + Send;
    fn dummy(&self) -> String;
}
/// Untrusted registration/login input; its password is deliberately not Debug.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    pub name: String,
    pub password: String,
}
/// The only response containing the raw session token.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub token: String,
    pub expires_ms: i64,
}
/// Token supplied by the trusted HTTP adapter for logout.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenInput {
    pub token: String,
}
/// Successful session revocation.
#[derive(Debug, Serialize)]
pub struct Revoked {
    pub revoked: bool,
}

pub(crate) fn internal(error: impl std::error::Error + Send + Sync + 'static) -> ActionError {
    ActionError::new(ErrorCode::Internal, "Authentication operation failed").with_source(error)
}
/// Generates lower-case hexadecimal identifiers/tokens with OS entropy.
pub fn random_hex(length: usize) -> Result<String, ActionError> {
    let mut bytes = vec![0; length];
    getrandom::fill(&mut bytes).map_err(|e| internal(std::io::Error::other(e.to_string())))?;
    Ok(hex(&bytes))
}
pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(output, "{byte:02x}").expect("formatting into String cannot fail");
    }
    output
}
/// Digests a raw bearer token before persistence or lookup.
pub fn token_hash(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}
fn valid_token(token: &str) -> bool {
    token.len() == 64
        && token
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn validate(input: &Credentials) -> Result<(), ActionError> {
    if !(3..=64).contains(&input.name.len())
        || !input
            .name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        || !(12..=128).contains(&input.password.len())
    {
        return Err(ActionError::new(
            ErrorCode::InvalidInput,
            "Use a 3–64 character ASCII username and 12–128 byte password",
        ));
    }
    Ok(())
}
/// Explicit native resources; callers own lifecycle and choose the clock.
#[derive(Clone)]
pub struct Auth<S, H> {
    pub store: S,
    pub passwords: H,
    session_ttl_ms: i64,
    now: Arc<dyn Fn() -> Result<i64, ActionError> + Send + Sync>,
}
impl<S: AuthStore, H: PasswordHasher> Auth<S, H> {
    /// Validates TTL before any authentication effects.
    pub fn new(store: S, passwords: H, ttl_ms: i64) -> Result<Self, ActionError> {
        Self::with_clock(store, passwords, ttl_ms, || {
            i64::try_from(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(internal)?
                    .as_millis(),
            )
            .map_err(internal)
        })
    }
    /// Supplies an application clock, also used by shared expiry fixtures.
    pub fn with_clock(
        store: S,
        passwords: H,
        ttl_ms: i64,
        clock: impl Fn() -> Result<i64, ActionError> + Send + Sync + 'static,
    ) -> Result<Self, ActionError> {
        if !(60_000..=2_592_000_000).contains(&ttl_ms) {
            return Err(ActionError::new(
                ErrorCode::InvalidInput,
                "Invalid session TTL",
            ));
        }
        Ok(Self {
            store,
            passwords,
            session_ttl_ms: ttl_ms,
            now: Arc::new(clock),
        })
    }
    fn time(&self) -> Result<i64, ActionError> {
        let now = (self.now)()?;
        if !(0..=9_007_199_254_740_991 - self.session_ttl_ms).contains(&now) {
            return Err(ActionError::new(
                ErrorCode::Internal,
                "Invalid authentication clock",
            ));
        }
        Ok(now)
    }
    /// Establishes identity from the store; never accepts scopes from a request.
    pub async fn principal(&self, token: Option<&str>) -> Result<Option<Principal>, ActionError> {
        let Some(token) = token.filter(|t| valid_token(t)) else {
            return Ok(None);
        };
        Ok(self
            .store
            .session(&token_hash(token), self.time()?)
            .await?
            .map(|u| Principal {
                subject: u.id,
                scopes: vec![],
            }))
    }
}
pub(crate) fn valid_instance_id(id: &str) -> bool {
    let segments: Vec<_> = id.split('.').collect();
    segments.len() <= 32
        && segments.iter().all(|s| {
            !s.is_empty()
                && s.len() <= 64
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        })
}
/// Native operation handles, namespaced by the application instance.
#[derive(Debug, Clone)]
pub struct Actions {
    pub register: Register,
    pub login: Login,
    pub session: Current,
    pub logout: Logout,
}
#[derive(Debug, Clone)]
pub struct Register(String);
#[derive(Debug, Clone)]
pub struct Login(String);
#[derive(Debug, Clone)]
pub struct Current(String);
#[derive(Debug, Clone)]
pub struct Logout(String);
impl Actions {
    /// Keeps multiple native module instances collision-free.
    pub fn new(id: &str) -> Result<Self, ActionError> {
        if !valid_instance_id(id) {
            return Err(ActionError::new(
                ErrorCode::InvalidInput,
                "Invalid Auth instance identifier",
            ));
        }
        Ok(Self {
            register: Register(format!("{id}.register")),
            login: Login(format!("{id}.login")),
            session: Current(format!("{id}.session")),
            logout: Logout(format!("{id}.logout")),
        })
    }
    /// Describes the actual native handles for SDK introspection.
    pub fn describe<S: AuthStore, H: PasswordHasher>(&self, id: &str) -> Module {
        let mut module = Module::new(id);
        module
            .action::<Auth<S, H>, _>(&self.register)
            .action::<Auth<S, H>, _>(&self.login)
            .action::<Auth<S, H>, _>(&self.session)
            .action::<Auth<S, H>, _>(&self.logout)
            .resource(format!("{id}.store"))
            .resource(format!("{id}.passwords"));
        module
    }
}
impl<S: AuthStore, H: PasswordHasher> Action<Auth<S, H>> for Register {
    type Input = Credentials;
    type Output = User;
    fn id(&self) -> &str {
        &self.0
    }
    fn validate_input(&self, input: &Credentials) -> Result<(), ActionError> {
        validate(input)
    }
    async fn authorize(&self, _: &Context<Auth<S, H>>, _: &Credentials) -> Result<(), ActionError> {
        Ok(())
    }
    async fn run(
        &self,
        ctx: &Context<Auth<S, H>>,
        input: Credentials,
    ) -> Result<User, ActionError> {
        let hash = ctx.resources.passwords.hash(input.password).await?;
        ctx.resources
            .store
            .create_user(input.name.to_ascii_lowercase(), hash)
            .await
    }
}
impl<S: AuthStore, H: PasswordHasher> Action<Auth<S, H>> for Login {
    type Input = Credentials;
    type Output = Session;
    fn id(&self) -> &str {
        &self.0
    }
    fn validate_input(&self, input: &Credentials) -> Result<(), ActionError> {
        validate(input)
    }
    async fn authorize(&self, _: &Context<Auth<S, H>>, _: &Credentials) -> Result<(), ActionError> {
        Ok(())
    }
    async fn run(
        &self,
        ctx: &Context<Auth<S, H>>,
        input: Credentials,
    ) -> Result<Session, ActionError> {
        let r = &ctx.resources;
        let credential = r.store.credential(&input.name.to_ascii_lowercase()).await?;
        let encoded = credential
            .as_ref()
            .map(|c| c.password_hash.clone())
            .unwrap_or_else(|| r.passwords.dummy());
        let valid = r.passwords.verify(input.password, &encoded).await?;
        let Some(credential) = credential.filter(|_| valid) else {
            return Err(ActionError::new(
                ErrorCode::Unauthenticated,
                "Invalid credentials",
            ));
        };
        let token = random_hex(32)?;
        let expires_ms = r.time()? + r.session_ttl_ms;
        r.store
            .save_session(token_hash(&token), credential.user.id, expires_ms)
            .await?;
        Ok(Session { token, expires_ms })
    }
}
impl<S: AuthStore, H: PasswordHasher> Action<Auth<S, H>> for Current {
    type Input = ();
    type Output = User;
    fn id(&self) -> &str {
        &self.0
    }
    async fn authorize(&self, ctx: &Context<Auth<S, H>>, _: &()) -> Result<(), ActionError> {
        require_principal(ctx.principal.as_ref()).map(|_| ())
    }
    async fn run(&self, ctx: &Context<Auth<S, H>>, _: ()) -> Result<User, ActionError> {
        ctx.resources
            .store
            .user(&require_principal(ctx.principal.as_ref())?.subject)
            .await?
            .ok_or_else(|| ActionError::new(ErrorCode::Unauthenticated, "Authentication required"))
    }
}
impl<S: AuthStore, H: PasswordHasher> Action<Auth<S, H>> for Logout {
    type Input = TokenInput;
    type Output = Revoked;
    fn id(&self) -> &str {
        &self.0
    }
    fn validate_input(&self, input: &TokenInput) -> Result<(), ActionError> {
        if !valid_token(&input.token) {
            Err(ActionError::new(ErrorCode::InvalidInput, "Invalid token"))
        } else {
            Ok(())
        }
    }
    async fn authorize(
        &self,
        ctx: &Context<Auth<S, H>>,
        input: &TokenInput,
    ) -> Result<(), ActionError> {
        let p = require_principal(ctx.principal.as_ref())?;
        let user = ctx
            .resources
            .store
            .session(&token_hash(&input.token), ctx.resources.time()?)
            .await?;
        if user.is_none_or(|u| u.id != p.subject) {
            return Err(ActionError::new(
                ErrorCode::Unauthenticated,
                "Authentication required",
            ));
        }
        Ok(())
    }
    async fn run(
        &self,
        ctx: &Context<Auth<S, H>>,
        input: TokenInput,
    ) -> Result<Revoked, ActionError> {
        ctx.resources
            .store
            .revoke_session(
                &token_hash(&input.token),
                &require_principal(ctx.principal.as_ref())?.subject,
            )
            .await?;
        Ok(Revoked { revoked: true })
    }
}
