pub use env_var::{EnvVar, bool_env_var, env_var};
use std::sync::LazyLock;

/// Whether Zed is running in stateless mode.
/// When true, Zed will use in-memory databases instead of persistent storage.
pub static ZED_STATELESS: LazyLock<bool> = bool_env_var!("ZED_STATELESS");

/// Optional PostgreSQL connection URL for agent thread storage. When unset,
/// the store defaults to `postgres://localhost/zed_threads`.
pub static ZED_THREADS_DATABASE_URL: LazyLock<Option<String>> =
    LazyLock::new(|| std::env::var("ZED_THREADS_DATABASE_URL").ok());
