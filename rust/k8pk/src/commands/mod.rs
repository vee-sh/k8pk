//! Command handlers for k8pk

mod alias;
mod config_ui;
mod context;
mod doctor;
mod interactive;
mod kubeconfig_ops;
mod login;
mod organize;
pub mod sessions;
pub mod tmux;
mod update;

pub use alias::run as alias;
pub use config_ui::*;
pub use context::*;
pub use doctor::run as doctor;
pub use interactive::*;
pub use kubeconfig_ops::*;
pub use login::*;
pub use organize::*;
pub use update::*;
