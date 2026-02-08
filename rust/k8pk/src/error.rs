//! Custom error types for k8pk

use std::path::PathBuf;
use thiserror::Error;

/// Main error type for k8pk operations
#[derive(Error, Debug)]
pub enum K8pkError {
    #[error("context '{0}' not found\n\n  Run 'k8pk contexts' to see available contexts")]
    ContextNotFound(String),

    #[error("context '{pattern}' not found. Did you mean:\n{suggestions}\n\n  Run 'k8pk contexts' to see all contexts")]
    ContextNotFoundSuggestions {
        pattern: String,
        suggestions: String,
    },

    #[error("cluster '{0}' not found in kubeconfig\n\n  The context may reference a deleted cluster. Run 'k8pk lint' to check")]
    ClusterNotFound(String),

    #[error("user '{0}' not found in kubeconfig\n\n  The context may reference a deleted user. Run 'k8pk lint' to check")]
    UserNotFound(String),

    #[error("no contexts found\n\n  Get started:\n    k8pk login --server https://your-cluster:6443\n    k8pk login --wizard\n\n  Or check your kubeconfig:\n    kubectl config get-contexts\n    k8pk --kubeconfig /path/to/config contexts")]
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

    #[error(
        "no previous context in history\n\n  Use 'k8pk ctx -' only after switching at least once"
    )]
    NoPreviousContext,

    #[error(
        "no previous namespace in history\n\n  Use 'k8pk ns -' only after switching at least once"
    )]
    NoPreviousNamespace,

    #[error("interactive selection requires a TTY\n\n  This command needs an interactive terminal.\n  For scripts, specify values directly:\n    k8pk ctx <context> -n <namespace>")]
    NoTty,

    #[error("selection cancelled")]
    Cancelled,

    #[error("cannot resolve home directory\n\n  HOME environment variable may not be set")]
    NoHomeDir,

    #[error("command failed: {0}")]
    CommandFailed(String),

    #[error("session expired for '{0}'\n\n  Re-authenticate interactively:\n    k8pk ctx {0}\n\n  Or login directly:\n    k8pk login")]
    SessionExpired(String),

    #[error("unknown output format: '{0}'\n\n  Valid formats: env, json, spawn")]
    UnknownOutputFormat(String),

    #[error("unsupported shell: '{0}'\n\n  Supported shells: bash, zsh, fish, powershell, elvish")]
    UnsupportedShell(String),

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error("login failed: {0}")]
    LoginFailed(String),

    #[error("lint failed\n\n  Run 'k8pk lint' for details")]
    LintFailed,

    #[error("HTTP request failed: {0}")]
    HttpError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML parse error: {0}\n\n  Run 'k8pk lint' to diagnose kubeconfig issues")]
    Yaml(#[from] serde_yaml_ng::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}

/// Compute edit distance between two strings (Levenshtein).
/// Used for "did you mean?" suggestions.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a_len = a.len();
    let b_len = b.len();
    let mut dp = vec![vec![0usize; b_len + 1]; a_len + 1];
    for (i, row) in dp.iter_mut().enumerate().take(a_len + 1) {
        row[0] = i;
    }
    for (j, val) in dp[0].iter_mut().enumerate().take(b_len + 1) {
        *val = j;
    }
    for (i, ac) in a.chars().enumerate() {
        for (j, bc) in b.chars().enumerate() {
            let cost = if ac == bc { 0 } else { 1 };
            dp[i + 1][j + 1] = (dp[i][j + 1] + 1)
                .min(dp[i + 1][j] + 1)
                .min(dp[i][j] + cost);
        }
    }
    dp[a_len][b_len]
}

/// Find the closest matching strings to `query` from `candidates`.
/// Returns up to `max` suggestions within a reasonable edit distance.
pub fn closest_matches<'a>(query: &str, candidates: &'a [String], max: usize) -> Vec<&'a str> {
    let threshold = (query.len() / 3).clamp(2, 4);
    let mut scored: Vec<_> = candidates
        .iter()
        .map(|c| {
            (
                c.as_str(),
                edit_distance(&query.to_lowercase(), &c.to_lowercase()),
            )
        })
        .filter(|(_, d)| *d <= threshold)
        .collect();
    scored.sort_by_key(|(_, d)| *d);
    scored.into_iter().take(max).map(|(s, _)| s).collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edit_distance_identical() {
        assert_eq!(edit_distance("abc", "abc"), 0);
    }

    #[test]
    fn test_edit_distance_one_char() {
        assert_eq!(edit_distance("abc", "abd"), 1);
        assert_eq!(edit_distance("abc", "abcd"), 1);
        assert_eq!(edit_distance("abc", "ab"), 1);
    }

    #[test]
    fn test_edit_distance_empty() {
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("abc", ""), 3);
        assert_eq!(edit_distance("", ""), 0);
    }

    #[test]
    fn test_closest_matches_finds_typo() {
        let candidates = vec![
            "prod-cluster".to_string(),
            "staging-cluster".to_string(),
            "dev-cluster".to_string(),
        ];
        let suggestions = closest_matches("prod-cluter", &candidates, 3);
        assert!(!suggestions.is_empty());
        assert_eq!(suggestions[0], "prod-cluster");
    }

    #[test]
    fn test_closest_matches_no_match() {
        let candidates = vec!["prod".to_string(), "staging".to_string()];
        let suggestions = closest_matches("completely-different-name", &candidates, 3);
        assert!(suggestions.is_empty());
    }

    #[test]
    fn test_closest_matches_respects_max() {
        let candidates = vec![
            "aa".to_string(),
            "ab".to_string(),
            "ac".to_string(),
            "ad".to_string(),
        ];
        let suggestions = closest_matches("aa", &candidates, 2);
        assert!(suggestions.len() <= 2);
    }

    #[test]
    fn test_error_display_unknown_format() {
        let err = K8pkError::UnknownOutputFormat("xml".to_string());
        let msg = format!("{}", err);
        assert!(msg.contains("xml"));
        assert!(msg.contains("env, json, spawn"));
    }

    #[test]
    fn test_error_display_unsupported_shell() {
        let err = K8pkError::UnsupportedShell("csh".to_string());
        let msg = format!("{}", err);
        assert!(msg.contains("csh"));
        assert!(msg.contains("bash"));
    }

    #[test]
    fn test_error_display_context_suggestions() {
        let err = K8pkError::ContextNotFoundSuggestions {
            pattern: "prod-cluter".to_string(),
            suggestions: "    - prod-cluster".to_string(),
        };
        let msg = format!("{}", err);
        assert!(msg.contains("prod-cluter"));
        assert!(msg.contains("Did you mean"));
        assert!(msg.contains("prod-cluster"));
    }

    #[test]
    fn test_no_contexts_suggests_login() {
        let err = K8pkError::NoContexts;
        let msg = format!("{}", err);
        assert!(msg.contains("k8pk login"));
    }

    #[test]
    fn test_error_display_invalid_argument() {
        let err = K8pkError::InvalidArgument("--json cannot be used with --dry-run".into());
        let msg = format!("{}", err);
        assert!(msg.contains("--json"));
        assert!(msg.contains("invalid argument"));
    }

    #[test]
    fn test_error_display_login_failed() {
        let err = K8pkError::LoginFailed("kubeconfig not generated".into());
        let msg = format!("{}", err);
        assert!(msg.contains("login failed"));
        assert!(msg.contains("kubeconfig"));
    }

    #[test]
    fn test_error_display_lint_failed() {
        let err = K8pkError::LintFailed;
        let msg = format!("{}", err);
        assert!(msg.contains("lint failed"));
    }

    #[test]
    fn test_error_display_http_error() {
        let err = K8pkError::HttpError("connection refused".into());
        let msg = format!("{}", err);
        assert!(msg.contains("HTTP"));
        assert!(msg.contains("connection refused"));
    }
}
