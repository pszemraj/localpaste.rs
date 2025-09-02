pub mod config;
pub mod db;
pub mod error;
pub mod handlers;
pub mod models;
pub mod naming;

pub use config::Config;
pub use db::Database;
pub use error::AppError;

#[derive(Clone)]
pub struct AppState {
    pub db: std::sync::Arc<Database>,
    pub config: std::sync::Arc<Config>,
}
