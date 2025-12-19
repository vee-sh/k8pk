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

    /// Get the next depth level for recursive shells
    pub fn next_depth(&self) -> u32 {
        self.depth + 1
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
