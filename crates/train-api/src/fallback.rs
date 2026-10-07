use serde::{Deserialize, Serialize};

pub const PROTOCOL: &str = "train-auth-fallback-v1";

#[derive(Serialize, Deserialize)]
pub struct FallbackStatus {
    pub available: bool,
    pub enabled_until: u64,
    pub protocol: String,
}

#[derive(Serialize)]
pub struct CredentialRequest {
    pub public_key: String,
    pub minecraft_access_token: String,
}

#[derive(Deserialize)]
pub struct CredentialReceipt {
    pub credential_id: String,
    pub expires_at: u64,
}

#[derive(Deserialize)]
pub struct FallbackTicket {
    pub ticket_id: String,
    pub expires_at: u64,
    pub mc_uuid: String,
    pub mc_name: String,
    pub server_id: String,
    pub protocol: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HttpTrainApiClient, TrainApiClient, TrainApiError};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn server(
        status: u16,
        body: &'static str,
    ) -> (HttpTrainApiClient, tokio::task::JoinHandle<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = HttpTrainApiClient::new(
            format!("http://{}", listener.local_addr().unwrap()),
            Some("discord-proof".into()),
        );
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut buffer = [0; 1024];
                let count = stream.read(&mut buffer).await.unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&buffer[..count]);
                if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                    let length = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length: "))
                        .map(|v| v.parse::<usize>().unwrap())
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            stream.write_all(format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            String::from_utf8(bytes).unwrap()
        });
        (client, task)
    }

    #[tokio::test]
    async fn all_requests_use_discord_and_minimal_contract_bodies() {
        let (client, request) = server(
            200,
            r#"{"available":true,"enabled_until":123,"protocol":"train-auth-fallback-v1"}"#,
        )
        .await;
        let status = client.fallback_status("server").await.unwrap();
        assert!(status.available);
        let request = request.await.unwrap();
        assert!(request.starts_with("GET /api/servers/server/auth-fallback/status "));
        assert!(request
            .to_lowercase()
            .contains("authorization: bearer discord-proof"));

        let (client, request) =
            server(200, r#"{"credential_id":"credential","expires_at":123}"#).await;
        client
            .enroll_fallback(
                "server",
                &CredentialRequest {
                    public_key: "public-spki".into(),
                    minecraft_access_token: "minecraft-proof".into(),
                },
            )
            .await
            .unwrap();
        let request = request.await.unwrap();
        assert!(request.starts_with("POST /api/servers/server/auth-fallback/credentials "));
        assert!(request
            .to_lowercase()
            .contains("authorization: bearer discord-proof"));
        let body: serde_json::Value =
            serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(
            body,
            serde_json::json!({"public_key":"public-spki","minecraft_access_token":"minecraft-proof"})
        );

        let (client, request) = server(200, r#"{"ticket_id":"ticket","expires_at":123,"mc_uuid":"uuid","mc_name":"Player","server_id":"server","protocol":"train-auth-fallback-v1"}"#).await;
        client
            .fallback_ticket("server", "credential")
            .await
            .unwrap();
        let request = request.await.unwrap();
        assert!(request.starts_with("POST /api/servers/server/auth-fallback/tickets "));
        assert!(request
            .to_lowercase()
            .contains("authorization: bearer discord-proof"));
        let body: serde_json::Value =
            serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body, serde_json::json!({"credential_id":"credential"}));
    }

    #[tokio::test]
    async fn errors_fail_closed_and_mock_cannot_issue_tickets() {
        for (status, body) in [
            (409, r#"{"error":"fallback_unavailable"}"#),
            (503, r#"{"error":"minecraft_unavailable"}"#),
            (403, r#"{"error":"minecraft_identity_mismatch"}"#),
            (403, r#"{"error":"link_required"}"#),
            (403, r#"{"error":"login_denied"}"#),
            (401, r#"{"error":"unauthorized"}"#),
        ] {
            let (client, request) = server(status, body).await;
            let result = client.fallback_ticket("server", "credential").await;
            assert!(match (status, result) {
                (409, Err(TrainApiError::FallbackUnavailable))
                | (503, Err(TrainApiError::MinecraftUnavailable))
                | (403, Err(TrainApiError::FallbackDenied))
                | (401, Err(TrainApiError::Unauthorized)) => true,
                _ => false,
            });
            request.await.unwrap();
        }
        assert!(
            !crate::MockTrainApiClient
                .fallback_status("server")
                .await
                .unwrap()
                .available
        );
        assert!(matches!(
            crate::MockTrainApiClient
                .fallback_ticket("server", "credential")
                .await,
            Err(TrainApiError::FallbackUnavailable)
        ));
    }
}
