//! Firecracker API client for Unix socket communication

use std::path::Path;

use anyhow::{Context, Result};
use bytes::Bytes;
use http::{Method, Request, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper_client_sockets::{Backend, tokio::TokioBackend};
use log::error;
use serde::Serialize;

use super::api_types::FirecrackerError;

/// Make a PUT request to the Firecracker API over Unix socket
pub async fn api_put<T: Serialize>(socket_path: &Path, path: &str, body: &T) -> Result<()> {
    // Serialize request body to JSON
    let json_body = serde_json::to_vec(body).context("Failed to serialize request body")?;

    // Connect to Unix socket
    let io = TokioBackend::connect_to_unix_socket(socket_path)
        .await
        .context("Failed to connect to Firecracker API socket")?;

    // Create HTTP/1 connection
    let (mut send_request, conn) = hyper::client::conn::http1::handshake::<_, Full<Bytes>>(io)
        .await
        .context("Failed to perform HTTP handshake")?;

    // Spawn connection handler
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            error!("Firecracker API connection error: {}", e);
        }
    });

    // Build HTTP request
    let uri = format!("http://localhost/{}", path.trim_start_matches('/'));
    let request = Request::builder()
        .method(Method::PUT)
        .uri(&uri)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .body(Full::new(Bytes::from(json_body)))
        .context("Failed to build HTTP request")?;

    // Send request and await response
    let response = send_request
        .send_request(request)
        .await
        .context("Failed to send API request")?;

    // Check response status
    let status = response.status();

    if status == StatusCode::NO_CONTENT {
        // 204 No Content - success
        return Ok(());
    }

    // Read error response body
    let body_bytes = response
        .into_body()
        .collect()
        .await
        .context("Failed to read error response")?
        .to_bytes();

    let error_text = String::from_utf8_lossy(&body_bytes);

    // Try to parse as FirecrackerError for better error messages
    if let Ok(fc_error) = serde_json::from_slice::<FirecrackerError>(&body_bytes) {
        return Err(anyhow::anyhow!(
            "Firecracker API error ({}): {}",
            status,
            fc_error.fault_message
        ));
    }

    // Fallback to raw error text
    Err(anyhow::anyhow!(
        "Firecracker API request failed with status {}: {}",
        status,
        error_text
    ))
}
