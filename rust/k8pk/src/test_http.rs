//! Test-only minimal HTTP servers for unit tests (no extra crates).
//!
//! ## What can be mocked here
//!
//! - **Rancher** (`reqwest::blocking` in `commands::login`): v3-public login, `/v3/clusters`
//!   lookup, pagination. Downstream **RKE1/RKE2** clusters are all represented through this
//!   Rancher HTTP API in k8pk — there is no separate “RKE wire protocol” in this codebase.
//!
//! ## What cannot (without refactors)
//!
//! - **OpenShift login** uses the **`oc` subprocess** (`oc login`), not in-process HTTP. To mock
//!   it you would need a fake `oc` on `PATH`, or an injectable command hook (not implemented).
//! - **GKE** uses **`gcloud`** for auth; same story.
//! - **kubectl / namespace listing** uses **`kubectl` or `oc`** subprocesses in `kubeconfig.rs`.
//!
//! For those flows, use integration tests with real tools or a wrapper script as `oc`.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// One HTTP response (status line uses reason phrase for 200/401).
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

impl HttpResponse {
    pub fn json(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into(),
        }
    }
}

fn write_http_response(stream: &mut impl Write, response: &HttpResponse) -> std::io::Result<()> {
    let reason = if response.status == 200 {
        "OK"
    } else if response.status == 401 {
        "Unauthorized"
    } else {
        "Error"
    };
    let resp = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        reason,
        response.body.len(),
        response.body
    );
    stream.write_all(resp.as_bytes())
}

/// Single accept: read one request, send one response. Returns base URL `http://host:port`.
pub fn spawn_one_shot(response: HttpResponse) -> String {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        tx.send(listener.local_addr().expect("addr"))
            .expect("send addr");
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 65536];
            let _ = stream.read(&mut buf);
            let _ = write_http_response(&mut stream, &response);
        }
    });
    let addr = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("test server addr");
    format!("http://{}", addr)
}

/// Multiple accepts on the **same** listener (same host:port), in order — for pagination on one Rancher URL.
pub fn spawn_sequential_same_socket(responses: Vec<HttpResponse>) -> String {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        tx.send(listener.local_addr().expect("addr"))
            .expect("send addr");
        for response in responses {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 65536];
                let _ = stream.read(&mut buf);
                let _ = write_http_response(&mut stream, &response);
            }
        }
    });
    let addr = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("test server addr");
    format!("http://{}", addr)
}

/// Two-page `/v3/clusters` sequence on one host:port (tests pagination `pagination.next`).
pub fn spawn_rancher_clusters_paginated(api_endpoint: &str, cluster_id: &str) -> String {
    let (tx, rx) = mpsc::channel();
    let api_ep = api_endpoint.to_string();
    let cid = cluster_id.to_string();
    thread::spawn(move || {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        let addr = listener.local_addr().expect("addr");
        tx.send(addr).expect("send addr");
        let base = format!("http://{}", addr);
        let page2_url = format!("{}/v3/clusters?limit=500&marker=1", base);
        let page1_body = serde_json::json!({
            "data": [],
            "pagination": { "next": page2_url }
        })
        .to_string();
        let page2_body = serde_json::json!({
            "data": [{
                "id": cid,
                "status": { "apiEndpoint": api_ep }
            }]
        })
        .to_string();
        for body in [page1_body, page2_body] {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 65536];
                let _ = stream.read(&mut buf);
                let _ = write_http_response(&mut stream, &HttpResponse::json(200, body));
            }
        }
    });
    let addr = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("test server addr");
    format!("http://{}", addr)
}

/// Single-page `/v3/clusters` listing with `(id, name)` pairs (for `rancher_list_clusters`).
pub fn spawn_rancher_clusters_named(clusters: &[(&str, &str)]) -> String {
    let data: Vec<serde_json::Value> = clusters
        .iter()
        .map(|(id, name)| serde_json::json!({ "id": id, "name": name }))
        .collect();
    let body = serde_json::json!({ "data": data }).to_string();
    spawn_one_shot(HttpResponse::json(200, body))
}

/// Local login returns 401, then Active Directory returns a token (same as `rancher_get_token` with provider `local`).
pub fn spawn_rancher_local_401_then_ad_token(token: &str) -> String {
    let ok_body = serde_json::json!({ "token": token }).to_string();
    spawn_sequential_same_socket(vec![
        HttpResponse::json(401, r#"{"type":"error","status":"401"}"#.to_string()),
        HttpResponse::json(200, ok_body),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_shot_responds() {
        let base = spawn_one_shot(HttpResponse::json(
            200,
            r#"{"token":"x","msg":"rancher-style"}"#,
        ));
        let body: serde_json::Value = reqwest::blocking::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .unwrap()
            .post(format!(
                "{}/v3-public/localProviders/local?action=login",
                base
            ))
            .json(&serde_json::json!({"username":"u","password":"p"}))
            .send()
            .expect("post")
            .json()
            .expect("json");
        assert_eq!(body.get("token").and_then(|t| t.as_str()), Some("x"));
    }
}
