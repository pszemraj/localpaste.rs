mod protocol;
mod worker;

pub use protocol::{CoreCmd, CoreEvent, PasteSummary};
pub use worker::{spawn_backend, BackendHandle};
