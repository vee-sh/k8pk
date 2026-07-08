//! Current state management for k8pk

use serde::{Deserialize, Serialize};
use std::env;
use std::path::PathBuf;

/// Represents the current k8pk session state
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CurrentState {
    /// Current Kubernetes context name
    pub context: Option<String>,
    /// Display-friendly context name
    pub context_display: Option<String>,
    /// Current namespace
    pub namespace: Option<String>,
    /// Recursive shell depth (0 = not in a k8pk shell)
    pub depth: u32,
    /// Path to the active kubeconfig file
    pub config_path: Option<PathBuf>,
}

impl CurrentState {
    /// Load current state from environment variables
    pub fn from_env() -> Self {
        let context = env::var("K8PK_CONTEXT").ok();
        let context_display = env::var("K8PK_CONTEXT_DISPLAY").ok();
        let namespace = env::var("K8PK_NAMESPACE").ok();
        let depth = env::var("K8PK_DEPTH")
            .ok()
            .and_then(|d| d.parse::<u32>().ok())
            .unwrap_or(0);
        let config_path = env::var("KUBECONFIG").ok().and_then(|k| {
            let p = PathBuf::from(k.split(':').next()?);
            if p.exists() {
                Some(p)
            } else {
                None
            }
        });

        Self {
            context,
            context_display,
            namespace,
            depth,
            config_path,
        }
    }

    /// Convert to JSON for `info all` command
    pub fn to_json(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        if let Some(ref ctx) = self.context {
            map.insert(
                "context".to_string(),
                serde_json::Value::String(ctx.clone()),
            );
        }
        if let Some(ref ctx) = self.context_display {
            map.insert(
                "context_display".to_string(),
                serde_json::Value::String(ctx.clone()),
            );
        }
        if let Some(ref ns) = self.namespace {
            map.insert(
                "namespace".to_string(),
                serde_json::Value::String(ns.clone()),
            );
        }
        map.insert(
            "depth".to_string(),
            serde_json::Value::Number(self.depth.into()),
        );
        if let Some(ref p) = self.config_path {
            map.insert(
                "config".to_string(),
                serde_json::Value::String(p.to_string_lossy().to_string()),
            );
        }
        serde_json::Value::Object(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_depth_zero() {
        let s = CurrentState::default();
        assert_eq!(s.depth, 0);
        assert!(s.context.is_none());
        assert!(s.namespace.is_none());
        assert!(s.config_path.is_none());
    }

    #[test]
    fn to_json_includes_set_fields() {
        let s = CurrentState {
            context: Some("dev".into()),
            namespace: Some("prod".into()),
            depth: 1,
            ..Default::default()
        };
        let j = s.to_json();
        assert_eq!(j["context"], "dev");
        assert_eq!(j["namespace"], "prod");
        assert_eq!(j["depth"], 1);
        assert!(j.get("config").is_none());
    }

    #[test]
    fn to_json_omits_none_fields() {
        let s = CurrentState::default();
        let j = s.to_json();
        assert!(j.get("context").is_none());
        assert!(j.get("namespace").is_none());
        assert_eq!(j["depth"], 0);
    }

    static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn from_env_parses_k8pk_vars() {
        let _guard = ENV_MUTEX.lock().unwrap();

        let saved_ctx = env::var_os("K8PK_CONTEXT");
        let saved_disp = env::var_os("K8PK_CONTEXT_DISPLAY");
        let saved_ns = env::var_os("K8PK_NAMESPACE");
        let saved_depth = env::var_os("K8PK_DEPTH");
        let saved_kc = env::var_os("KUBECONFIG");

        let dir = tempfile::tempdir().unwrap();
        let kc_path = dir.path().join("config");
        std::fs::write(&kc_path, "apiVersion: v1").unwrap();

        env::set_var("K8PK_CONTEXT", "test-ctx");
        env::set_var("K8PK_CONTEXT_DISPLAY", "TestCtx");
        env::set_var("K8PK_NAMESPACE", "test-ns");
        env::set_var("K8PK_DEPTH", "2");
        env::set_var("KUBECONFIG", kc_path.to_str().unwrap());

        let state = CurrentState::from_env();
        assert_eq!(state.context, Some("test-ctx".to_string()));
        assert_eq!(state.context_display, Some("TestCtx".to_string()));
        assert_eq!(state.namespace, Some("test-ns".to_string()));
        assert_eq!(state.depth, 2);
        assert_eq!(state.config_path, Some(kc_path));

        // Restore
        for (key, val) in [
            ("K8PK_CONTEXT", saved_ctx),
            ("K8PK_CONTEXT_DISPLAY", saved_disp),
            ("K8PK_NAMESPACE", saved_ns),
            ("K8PK_DEPTH", saved_depth),
            ("KUBECONFIG", saved_kc),
        ] {
            if let Some(v) = val {
                env::set_var(key, v);
            } else {
                env::remove_var(key);
            }
        }
    }

    #[test]
    fn from_env_depth_defaults_to_zero_on_bad_value() {
        let _guard = ENV_MUTEX.lock().unwrap();

        let saved = env::var_os("K8PK_DEPTH");
        env::set_var("K8PK_DEPTH", "notanumber");

        let state = CurrentState::from_env();
        assert_eq!(state.depth, 0);

        if let Some(v) = saved {
            env::set_var("K8PK_DEPTH", v);
        } else {
            env::remove_var("K8PK_DEPTH");
        }
    }

    #[test]
    fn from_env_config_path_none_when_file_missing() {
        let _guard = ENV_MUTEX.lock().unwrap();

        let saved = env::var_os("KUBECONFIG");
        env::set_var("KUBECONFIG", "/nonexistent/path/for/test/config");

        let state = CurrentState::from_env();
        assert!(state.config_path.is_none());

        if let Some(v) = saved {
            env::set_var("KUBECONFIG", v);
        } else {
            env::remove_var("KUBECONFIG");
        }
    }
}
