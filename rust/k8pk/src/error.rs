//! Custom error types for k8pk

use std::path::PathBuf;
use thiserror::Error;

/// Main error type for k8pk operations
#[derive(Error, Debug)]
pub enum K8pkError {
    #[error("context '{0}' not found")]
    ContextNotFound(String),


    #[error("cluster '{0}' not found")]
    ClusterNotFound(String),

    #[error("user '{0}' not found")]
    UserNotFound(String),

    #[error("no contexts found")]
    NoContexts,

    #[error("no namespaces found for context '{0}'")]
    NoNamespaces(String),

    #[error("kubeconfig file not found: {0}")]
    KubeconfigNotFound(PathBuf),

    #[error("invalid kubeconfig: {0}")]
    InvalidKubeconfig(String),

    #[error("neither 'oc' nor 'kubectl' found on PATH")]
    NoK8sCli,

    #[error("not in a k8pk context. Use 'k8pk ctx <context>' first")]
    NotInContext,

    #[error("no previous context in history")]
    NoPreviousContext,

    #[error("no previous namespace in history")]
    NoPreviousNamespace,

    #[error("interactive selection requires a TTY")]
    NoTty,

    #[error("selection cancelled")]
    Cancelled,

    #[error("cannot resolve home directory")]
    NoHomeDir,

    #[error("command failed: {0}")]
    CommandFailed(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML parse error: {0}")]
    Yaml(#[from] serde_yaml_ng::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}

impl From<&str> for K8pkError {
    fn from(s: &str) -> Self {
        K8pkError::Other(s.to_string())
    }
}

impl From<String> for K8pkError {
    fn from(s: String) -> Self {
        K8pkError::Other(s)
    }
}

/// Result type alias for k8pk operations
pub type Result<T> = std::result::Result<T, K8pkError>;

