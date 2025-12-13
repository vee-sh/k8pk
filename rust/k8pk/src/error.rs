//! Custom error types for k8pk

use std::path::PathBuf;
use thiserror::Error;

/// Main error type for k8pk operations
#[derive(Error, Debug)]
pub enum K8pkError {
    #[error("context '{0}' not found\n\n  Run 'k8pk contexts' to see available contexts")]
    ContextNotFound(String),

    #[error("cluster '{0}' not found in kubeconfig\n\n  The context may reference a deleted cluster. Run 'k8pk lint' to check")]
    ClusterNotFound(String),

    #[error("user '{0}' not found in kubeconfig\n\n  The context may reference a deleted user. Run 'k8pk lint' to check")]
    UserNotFound(String),

    #[error("no contexts found\n\n  Check your kubeconfig:\n    kubectl config get-contexts\n\n  Or specify a different file:\n    k8pk --kubeconfig /path/to/config contexts")]
    NoContexts,

    #[error("no namespaces found for context '{0}'\n\n  The cluster may be unreachable or you may lack permissions.\n  Try: kubectl --context {0} get namespaces")]
    NoNamespaces(String),

    #[error("kubeconfig file not found: {0}\n\n  Create it with:\n    kubectl config set-cluster <name> --server=https://...\n\n  Or check KUBECONFIG environment variable")]
    KubeconfigNotFound(PathBuf),

    #[error("invalid kubeconfig: {0}\n\n  Run 'k8pk lint' to diagnose issues")]
    InvalidKubeconfig(String),

    #[error("neither 'oc' nor 'kubectl' found on PATH\n\n  Install kubectl:\n    brew install kubectl\n    # or: https://kubernetes.io/docs/tasks/tools/")]
    NoK8sCli,

    #[error("not in a k8pk context\n\n  Switch to a context first:\n    k8pk ctx <context-name>\n\n  Or run interactively:\n    k8pk")]
    NotInContext,

    #[error("no previous context in history\n\n  Use 'k8pk ctx -' only after switching at least once")]
    NoPreviousContext,

    #[error("no previous namespace in history\n\n  Use 'k8pk ns -' only after switching at least once")]
    NoPreviousNamespace,

    #[error("interactive selection requires a TTY\n\n  This command needs an interactive terminal.\n  For scripts, specify values directly:\n    k8pk ctx <context> -n <namespace>")]
    NoTty,

    #[error("selection cancelled")]
    Cancelled,

    #[error("cannot resolve home directory\n\n  HOME environment variable may not be set")]
    NoHomeDir,

    #[error("command failed: {0}")]
    CommandFailed(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML parse error: {0}\n\n  Run 'k8pk lint' to diagnose kubeconfig issues")]
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
