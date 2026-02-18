// src/provider/bedrock.rs — AWS Bedrock provider (SigV4 auth)
//
// Uses the Bedrock Runtime "converse" and "converse-stream" APIs.
// Implements SigV4 request signing without pulling in the full AWS SDK,
// keeping the binary lean.

use async_trait::async_trait;
use chrono::Utc;
use futures::Stream;
use futures::StreamExt;
use reqwest_eventsource::{Event, RequestBuilderExt};
use std::pin::Pin;

use super::{
    ChatChunk, ChatRequest, ChatResponse, ModelInfo, ModelProvider, Role, StopReason, TokenUsage,
};
use crate::infra::errors::OpenKoiError;

/// AWS Bedrock provider — routes requests through Amazon's Bedrock Runtime API
/// using SigV4 request signing.
pub struct BedrockProvider {
    access_key_id: String,
    secret_access_key: String,
    session_token: Option<String>,
    region: String,
    default_model: String,
    client: reqwest::Client,
}

impl BedrockProvider {
    pub fn new(
        access_key_id: String,
        secret_access_key: String,
        session_token: Option<String>,
        region: Option<String>,
        default_model: Option<String>,
    ) -> Self {
        Self {
            access_key_id,
            secret_access_key,
            session_token,
            region: region.unwrap_or_else(|| "us-east-1".into()),
            default_model: default_model
                .unwrap_or_else(|| "anthropic.claude-sonnet-4-20250514-v1:0".into()),
            client: reqwest::Client::new(),
        }
    }

    /// Bedrock Runtime endpoint for the configured region.
    fn endpoint(&self) -> String {
        format!(
            "https://bedrock-runtime.{}.amazonaws.com",
            self.region
        )
    }

    /// Build the Bedrock Converse API request body from a ChatRequest.
    fn build_converse_body(&self, request: &ChatRequest) -> serde_json::Value {
        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .filter(|m| m.role != Role::System)
            .map(|m| {
                serde_json::json!({
                    "role": match m.role {
                        Role::User => "user",
                        Role::Assistant => "assistant",
                        Role::Tool => "user",
                        Role::System => "user", // filtered above, but satisfy match
                    },
                    "content": [{
                        "text": m.content,
                    }],
                })
            })
            .collect();

        let mut body = serde_json::json!({
            "messages": messages,
        });

        // System prompt via the "system" field
        let system_text = request
            .system
            .as_deref()
            .or_else(|| {
                request
                    .messages
                    .iter()
                    .find(|m| m.role == Role::System)
                    .map(|m| m.content.as_str())
            });
        if let Some(sys) = system_text {
            body["system"] = serde_json::json!([{ "text": sys }]);
        }

        // Inference config
        let mut inference = serde_json::Map::new();
        if let Some(max_tokens) = request.max_tokens {
            inference.insert("maxTokens".into(), serde_json::json!(max_tokens));
        }
        if let Some(temp) = request.temperature {
            inference.insert("temperature".into(), serde_json::json!(temp));
        }
        if !inference.is_empty() {
            body["inferenceConfig"] = serde_json::Value::Object(inference);
        }

        body
    }

    /// Sign a request with AWS SigV4.
    ///
    /// This is a minimal implementation covering Bedrock's needs (JSON POST bodies,
    /// single-region). For production use with temporary credentials, session tokens
    /// are included in the canonical request headers.
    fn sign_request(
        &self,
        method: &str,
        url: &str,
        headers: &mut Vec<(String, String)>,
        payload: &[u8],
    ) {
        let now = Utc::now();
        let datestamp = now.format("%Y%m%d").to_string();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();

        // Parse host from URL
        let parsed = url::Url::parse(url).expect("valid URL");
        let host = parsed.host_str().unwrap_or_default();
        let path = parsed.path();

        // Add required headers
        headers.push(("host".into(), host.to_string()));
        headers.push(("x-amz-date".into(), amz_date.clone()));
        headers.push(("content-type".into(), "application/json".into()));

        if let Some(ref token) = self.session_token {
            headers.push(("x-amz-security-token".into(), token.clone()));
        }

        // Sort headers for canonical request
        headers.sort_by(|a, b| a.0.cmp(&b.0));

        let signed_headers: String = headers
            .iter()
            .map(|(k, _)| k.as_str())
            .collect::<Vec<_>>()
            .join(";");

        let canonical_headers: String = headers
            .iter()
            .map(|(k, v)| format!("{}:{}\n", k, v.trim()))
            .collect();

        let payload_hash = sha256_hex(payload);

        let canonical_request = format!(
            "{}\n{}\n{}\n{}\n{}\n{}",
            method,
            path,
            "", // query string (empty for Bedrock)
            canonical_headers,
            signed_headers,
            payload_hash
        );

        let credential_scope = format!("{}/{}/bedrock/aws4_request", datestamp, self.region);

        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{}\n{}\n{}",
            amz_date,
            credential_scope,
            sha256_hex(canonical_request.as_bytes())
        );

        // Derive signing key
        let k_date = hmac_sha256(
            format!("AWS4{}", self.secret_access_key).as_bytes(),
            datestamp.as_bytes(),
        );
        let k_region = hmac_sha256(&k_date, self.region.as_bytes());
        let k_service = hmac_sha256(&k_region, b"bedrock");
        let k_signing = hmac_sha256(&k_service, b"aws4_request");

        let signature = hex_encode(&hmac_sha256(&k_signing, string_to_sign.as_bytes()));

        let auth_header = format!(
            "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
            self.access_key_id, credential_scope, signed_headers, signature
        );

        headers.push(("authorization".into(), auth_header));
    }
}

#[async_trait]
impl ModelProvider for BedrockProvider {
    fn id(&self) -> &str {
        "bedrock"
    }

    fn name(&self) -> &str {
        "AWS Bedrock"
    }

    fn models(&self) -> Vec<ModelInfo> {
        vec![
            ModelInfo {
                id: "anthropic.claude-sonnet-4-20250514-v1:0".into(),
                name: "Claude Sonnet 4 (Bedrock)".into(),
                context_window: 200_000,
                max_output_tokens: 8_192,
                supports_tools: true,
                supports_streaming: true,
                input_price_per_mtok: 3.0,
                output_price_per_mtok: 15.0,
            },
            ModelInfo {
                id: "anthropic.claude-haiku-3-5-20241022-v1:0".into(),
                name: "Claude 3.5 Haiku (Bedrock)".into(),
                context_window: 200_000,
                max_output_tokens: 8_192,
                supports_tools: true,
                supports_streaming: true,
                input_price_per_mtok: 0.8,
                output_price_per_mtok: 4.0,
            },
            ModelInfo {
                id: "amazon.nova-pro-v1:0".into(),
                name: "Amazon Nova Pro".into(),
                context_window: 300_000,
                max_output_tokens: 5_120,
                supports_tools: true,
                supports_streaming: true,
                input_price_per_mtok: 0.8,
                output_price_per_mtok: 3.2,
            },
            ModelInfo {
                id: "meta.llama3-3-70b-instruct-v1:0".into(),
                name: "Llama 3.3 70B (Bedrock)".into(),
                context_window: 128_000,
                max_output_tokens: 4_096,
                supports_tools: false,
                supports_streaming: true,
                input_price_per_mtok: 0.72,
                output_price_per_mtok: 0.72,
            },
        ]
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, OpenKoiError> {
        let model_id = if request.model.is_empty() {
            &self.default_model
        } else {
            &request.model
        };
        let url = format!("{}/model/{}/converse", self.endpoint(), model_id);
        let body = self.build_converse_body(&request);
        let payload = serde_json::to_vec(&body).unwrap_or_default();

        let mut sig_headers = Vec::new();
        self.sign_request("POST", &url, &mut sig_headers, &payload);

        let mut req = self.client.post(&url);
        for (k, v) in &sig_headers {
            req = req.header(k.as_str(), v.as_str());
        }
        req = req.body(payload);

        let response = req.send().await.map_err(|e| OpenKoiError::Provider {
            provider: "bedrock".into(),
            message: e.to_string(),
            retriable: e.is_timeout(),
        })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            return Err(OpenKoiError::Provider {
                provider: "bedrock".into(),
                message: format!("HTTP {}: {}", status, error_body),
                retriable: status.as_u16() == 429 || status.is_server_error(),
            });
        }

        let resp: serde_json::Value =
            response
                .json()
                .await
                .map_err(|e| OpenKoiError::Provider {
                    provider: "bedrock".into(),
                    message: e.to_string(),
                    retriable: false,
                })?;

        // Bedrock Converse response: { output: { message: { content: [{ text: "..." }] } }, usage: { ... }, stopReason }
        let content = resp["output"]["message"]["content"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|c| c["text"].as_str())
            .unwrap_or("")
            .to_string();

        let usage = TokenUsage {
            input_tokens: resp["usage"]["inputTokens"].as_u64().unwrap_or(0) as u32,
            output_tokens: resp["usage"]["outputTokens"].as_u64().unwrap_or(0) as u32,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        };

        let stop_reason = match resp["stopReason"].as_str() {
            Some("end_turn") => StopReason::EndTurn,
            Some("max_tokens") => StopReason::MaxTokens,
            Some("tool_use") => StopReason::ToolUse,
            Some("stop_sequence") => StopReason::StopSequence,
            _ => StopReason::Unknown,
        };

        Ok(ChatResponse {
            content,
            tool_calls: Vec::new(),
            usage,
            stop_reason,
        })
    }

    async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatChunk, OpenKoiError>> + Send>>, OpenKoiError>
    {
        let model_id = if request.model.is_empty() {
            self.default_model.clone()
        } else {
            request.model.clone()
        };
        let url = format!(
            "{}/model/{}/converse-stream",
            self.endpoint(),
            model_id
        );
        let body = self.build_converse_body(&request);
        let payload = serde_json::to_vec(&body).unwrap_or_default();

        let mut sig_headers = Vec::new();
        self.sign_request("POST", &url, &mut sig_headers, &payload);

        let mut req = self.client.post(&url);
        for (k, v) in &sig_headers {
            req = req.header(k.as_str(), v.as_str());
        }
        req = req.body(payload);

        let mut es = req.eventsource().unwrap();

        let stream = async_stream::stream! {
            while let Some(event) = es.next().await {
                match event {
                    Ok(Event::Open) => {},
                    Ok(Event::Message(msg)) => {
                        let parsed: serde_json::Value = match serde_json::from_str(&msg.data) {
                            Ok(v) => v,
                            Err(e) => {
                                yield Err(OpenKoiError::Provider {
                                    provider: "bedrock".into(),
                                    message: format!("Failed to parse stream event: {}", e),
                                    retriable: false,
                                });
                                break;
                            }
                        };

                        // Bedrock stream events: contentBlockDelta, metadata
                        if let Some(delta) = parsed.get("contentBlockDelta") {
                            let text = delta["delta"]["text"]
                                .as_str()
                                .unwrap_or("")
                                .to_string();
                            if !text.is_empty() {
                                yield Ok(ChatChunk {
                                    delta: text,
                                    tool_call_delta: None,
                                    usage: None,
                                });
                            }
                        }

                        // Final metadata event with usage
                        if let Some(metadata) = parsed.get("metadata") {
                            if let Some(usage) = metadata.get("usage") {
                                yield Ok(ChatChunk {
                                    delta: String::new(),
                                    tool_call_delta: None,
                                    usage: Some(TokenUsage {
                                        input_tokens: usage["inputTokens"]
                                            .as_u64()
                                            .unwrap_or(0) as u32,
                                        output_tokens: usage["outputTokens"]
                                            .as_u64()
                                            .unwrap_or(0) as u32,
                                        cache_read_tokens: 0,
                                        cache_write_tokens: 0,
                                    }),
                                });
                            }
                        }
                    }
                    Err(reqwest_eventsource::Error::StreamEnded) => break,
                    Err(e) => {
                        yield Err(OpenKoiError::Provider {
                            provider: "bedrock".into(),
                            message: format!("Stream error: {}", e),
                            retriable: false,
                        });
                        break;
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }

    async fn embed(&self, _texts: &[&str]) -> Result<Vec<Vec<f32>>, OpenKoiError> {
        // Bedrock supports Titan Embeddings and Cohere Embed, but we'd need
        // a separate model invocation. For now, delegate to a dedicated embedder.
        Err(OpenKoiError::Provider {
            provider: "bedrock".into(),
            message: "Use a dedicated embedding provider (e.g. openai/text-embedding-3-small). \
                      Bedrock embedding support is planned."
                .into(),
            retriable: false,
        })
    }
}

// ─── Minimal crypto helpers for SigV4 ───────────────────────────────────────
// These avoid pulling in ring/sha2/hmac crates by using a simple implementation.
// In production, consider using the `ring` crate for FIPS-validated crypto.

fn sha256_hex(data: &[u8]) -> String {
    hex_encode(&sha256(data))
}

fn sha256(data: &[u8]) -> [u8; 32] {
    // Minimal SHA-256 implementation for SigV4 signing.
    // Uses Rust's standard approach — in production, prefer `ring::digest`.
    use std::num::Wrapping;

    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let mut h: [Wrapping<u32>; 8] = [
        Wrapping(0x6a09e667),
        Wrapping(0xbb67ae85),
        Wrapping(0x3c6ef372),
        Wrapping(0xa54ff53a),
        Wrapping(0x510e527f),
        Wrapping(0x9b05688c),
        Wrapping(0x1f83d9ab),
        Wrapping(0x5be0cd19),
    ];

    // Pre-processing: pad message
    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    // Process each 512-bit (64-byte) chunk
    for chunk in msg.chunks_exact(64) {
        let mut w = [Wrapping(0u32); 64];
        for i in 0..16 {
            w[i] = Wrapping(u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]));
        }
        for i in 16..64 {
            let s0 = (w[i - 15].0.rotate_right(7))
                ^ (w[i - 15].0.rotate_right(18))
                ^ (w[i - 15].0 >> 3);
            let s1 = (w[i - 2].0.rotate_right(17))
                ^ (w[i - 2].0.rotate_right(19))
                ^ (w[i - 2].0 >> 10);
            w[i] = w[i - 16] + Wrapping(s0) + w[i - 7] + Wrapping(s1);
        }

        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);

        for i in 0..64 {
            let s1 = Wrapping(
                e.0.rotate_right(6) ^ e.0.rotate_right(11) ^ e.0.rotate_right(25),
            );
            let ch = Wrapping((e.0 & f.0) ^ ((!e.0) & g.0));
            let temp1 = hh + s1 + ch + Wrapping(K[i]) + w[i];
            let s0 = Wrapping(
                a.0.rotate_right(2) ^ a.0.rotate_right(13) ^ a.0.rotate_right(22),
            );
            let maj = Wrapping((a.0 & b.0) ^ (a.0 & c.0) ^ (b.0 & c.0));
            let temp2 = s0 + maj;

            hh = g;
            g = f;
            f = e;
            e = d + temp1;
            d = c;
            c = b;
            b = a;
            a = temp1 + temp2;
        }

        h[0] = h[0] + a;
        h[1] = h[1] + b;
        h[2] = h[2] + c;
        h[3] = h[3] + d;
        h[4] = h[4] + e;
        h[5] = h[5] + f;
        h[6] = h[6] + g;
        h[7] = h[7] + hh;
    }

    let mut result = [0u8; 32];
    for (i, val) in h.iter().enumerate() {
        result[i * 4..i * 4 + 4].copy_from_slice(&val.0.to_be_bytes());
    }
    result
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let block_size = 64;

    let key = if key.len() > block_size {
        sha256(key).to_vec()
    } else {
        key.to_vec()
    };

    let mut k_ipad = vec![0x36u8; block_size];
    let mut k_opad = vec![0x5cu8; block_size];

    for (i, &b) in key.iter().enumerate() {
        k_ipad[i] ^= b;
        k_opad[i] ^= b;
    }

    k_ipad.extend_from_slice(data);
    let inner_hash = sha256(&k_ipad);

    k_opad.extend_from_slice(&inner_hash);
    sha256(&k_opad)
}

fn hex_encode(data: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(data.len() * 2);
    for b in data {
        write!(s, "{:02x}", b).unwrap();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_empty() {
        let hash = sha256_hex(b"");
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_sha256_hello() {
        let hash = sha256_hex(b"hello");
        assert_eq!(
            hash,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_hmac_sha256_rfc4231_case1() {
        // RFC 4231 test case 1
        let key = [0x0b; 20];
        let data = b"Hi There";
        let result = hex_encode(&hmac_sha256(&key, data));
        assert_eq!(
            result,
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[test]
    fn test_bedrock_provider_basic() {
        let provider = BedrockProvider::new(
            "AKIAIOSFODNN7EXAMPLE".into(),
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".into(),
            None,
            Some("us-west-2".into()),
            None,
        );
        assert_eq!(provider.id(), "bedrock");
        assert_eq!(provider.name(), "AWS Bedrock");
        assert!(!provider.models().is_empty());
        assert_eq!(
            provider.endpoint(),
            "https://bedrock-runtime.us-west-2.amazonaws.com"
        );
    }

    #[test]
    fn test_converse_body_structure() {
        let provider = BedrockProvider::new(
            "key".into(),
            "secret".into(),
            None,
            None,
            None,
        );
        let request = ChatRequest {
            model: "anthropic.claude-sonnet-4-20250514-v1:0".into(),
            messages: vec![
                super::super::Message::user("Hello"),
            ],
            system: Some("You are helpful.".into()),
            max_tokens: Some(1024),
            temperature: Some(0.7),
            ..Default::default()
        };

        let body = provider.build_converse_body(&request);
        assert!(body["messages"].is_array());
        assert!(body["system"].is_array());
        assert!(body["inferenceConfig"]["maxTokens"].is_number());
    }
}
