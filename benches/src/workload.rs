//! Fixed request fixtures shared by load and streaming measurements.

use anyhow::{Result, bail};
use reqwest::Client;
use serde::Serialize;
use serde_json::{Value, json};
use tokio::time::Instant;

/// Fixed, shared input corpus.
const FIXTURES: &str = include_str!("../fixtures.json");

// -----------------------------------------------------------------------------
// Workload: Prepared provider request and its response checks.
// -----------------------------------------------------------------------------

/// Maximum historical user turns in the long fixture.
const WORKLOAD_LONG_HISTORY_TURNS: usize = 100;

/// Maximum decoded body accepted by the load generator.
const WORKLOAD_RESPONSE_LIMIT_BYTES: usize = 8 * 1024 * 1024;

/// Prepared provider request and its response checks.
#[derive(Clone)]
pub struct Workload {
    /// Stable workload name used in result artifacts.
    name: String,
    /// Provider route for this workload.
    url: String,
    /// Fixed serialized request input.
    #[cfg(test)]
    body: Value,
    /// Encoded request bytes shared between clients.
    payload: bytes::Bytes,
    /// Response validation and payload detection mode.
    kind: String,
    /// Expected provider response media type.
    content_type: &'static str,
}

impl Workload {
    /// # Errors
    /// Returns setup, validation, or I/O failures to the benchmark runner.
    fn new(name: &str, base: &str) -> Result<Self> {
        let fixtures: Value = serde_json::from_str(FIXTURES)?;
        let (path, body, kind, content_type) = if name.starts_with("chat-") {
            if !["chat-short", "chat-long", "chat-sse", "chat-long-sse"].contains(&name) {
                bail!("unknown chat workload: {name}");
            }
            let long = name.contains("long");
            let stream = name.ends_with("sse");
            let count = if long { WORKLOAD_LONG_HISTORY_TURNS } else { 1 };
            let messages = Self::messages(
                count,
                &fixtures["chat"][if long { "long" } else { "short" }],
            );
            (
                "/openai/v1/chat/completions",
                json!({"model":"eliza-1966", "messages":messages, "stream":stream}),
                if stream { "chat-sse" } else { "chat" },
                if stream {
                    "text/event-stream"
                } else {
                    "application/json"
                },
            )
        } else {
            let fields = name.split('-').collect::<Vec<_>>();
            if !matches!(
                fields.as_slice(),
                ["speech", "short" | "medium" | "long", "pcm" | "wav" | "mp3"]
                    | [
                        "speech",
                        "short" | "medium" | "long",
                        "pcm" | "wav" | "mp3",
                        "sse"
                    ]
            ) {
                bail!("unknown speech workload: {name}");
            }
            let stream = fields.len() == 4;
            let format = fields[2];
            let audio_type = match format {
                "pcm" => "audio/pcm",
                "wav" => "audio/wav",
                _ => "audio/mpeg",
            };
            (
                "/openai/v1/audio/speech",
                json!({"model":"flite", "input":fixtures["speech"][fields[1]],
                "voice":"Kore", "speed":1, "response_format":format, "stream_format":if stream {"sse"} else {"audio"}}),
                if stream { "speech-sse" } else { format },
                if stream {
                    "text/event-stream"
                } else {
                    audio_type
                },
            )
        };
        Ok(Self {
            name: name.to_owned(),
            url: format!("{base}{path}"),
            payload: bytes::Bytes::from(serde_json::to_vec(&body)?),
            #[cfg(test)]
            body,
            kind: kind.to_owned(),
            content_type,
        })
    }

    /// Build alternating history ending at the latest user input.
    fn messages(count: usize, text: &Value) -> Vec<Value> {
        let mut messages = Vec::new();
        for _ in 0..count {
            messages.push(json!({"role":"user", "content":text}));
            messages.push(json!({"role":"assistant", "content":"OK"}));
        }
        let _last_assistant = messages.pop();
        messages
    }

    /// Detect a complete content delta or the first audio sample.
    #[allow(
        rlib::results_converted_to_options,
        reason = "incomplete SSE JSON is expected until another chunk arrives"
    )]
    fn is_meaningful(&self, bytes: &[u8]) -> bool {
        // Binary content has no SSE record boundary.
        if !self.kind.ends_with("sse") {
            return bytes.len() > if self.kind == "wav" { 44 } else { 0 };
        }
        let text = String::from_utf8_lossy(bytes);
        let records = text.lines().filter_map(|line| line.strip_prefix("data: "));
        records
            .filter_map(|record| serde_json::from_str::<Value>(record).ok())
            .any(|record| {
                if self.kind == "chat-sse" {
                    record["choices"][0]["delta"]["content"]
                        .as_str()
                        .is_some_and(|text| !text.is_empty())
                } else {
                    record["type"] == "speech.audio.delta"
                        && record["audio"]
                            .as_str()
                            .is_some_and(|audio| !audio.is_empty())
                }
            })
    }

    /// Require audio content followed by the speech completion record.
    #[allow(
        rlib::results_converted_to_options,
        reason = "malformed terminal JSON means an invalid response"
    )]
    fn is_valid_speech_sse(&self, bytes: &[u8]) -> bool {
        let text = String::from_utf8_lossy(bytes);
        let last = text
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .next_back();
        let terminal = last.and_then(|record| serde_json::from_str::<Value>(record).ok());
        self.is_meaningful(bytes)
            && terminal.is_some_and(|record| record["type"] == "speech.audio.done")
    }

    /// Check complete responses, including their terminal streaming record.
    #[allow(
        rlib::results_converted_to_options,
        reason = "invalid JSON is a failed benchmark response"
    )]
    fn is_valid(&self, bytes: &[u8]) -> bool {
        match self.kind.as_str() {
            "chat" => serde_json::from_slice::<Value>(bytes)
                .ok()
                .is_some_and(|value| {
                    value["choices"][0]["message"]["content"]
                        .as_str()
                        .is_some_and(|text| !text.is_empty())
                }),
            "chat-sse" => {
                self.is_meaningful(bytes)
                    && String::from_utf8_lossy(bytes)
                        .trim_end()
                        .ends_with("data: [DONE]")
            }
            "speech-sse" => self.is_valid_speech_sse(bytes),
            "pcm" => !bytes.is_empty() && bytes.len().is_multiple_of(2),
            "wav" => bytes.len() > 44 && bytes.starts_with(b"RIFF"),
            _ => !bytes.is_empty(),
        }
    }

    /// Consume and validate a bounded response, measuring from its offered time.
    pub async fn perform(&self, client: &Client, offered: Instant) -> Sample {
        let dispatched = Instant::now();
        let mut first = None;
        let result = async {
            let request = client
                .post(&self.url)
                .body(self.payload.clone())
                .header(reqwest::header::CONTENT_TYPE, "application/json");
            let mut response = request.send().await?;
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .ok_or_else(|| anyhow::anyhow!("response lacks Content-Type"))?
                .to_str()?;
            let headers_valid =
                response.status() == 200 && content_type.starts_with(self.content_type);
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await? {
                if bytes.len() + chunk.len() > WORKLOAD_RESPONSE_LIMIT_BYTES {
                    bail!("response exceeds benchmark's 8 MiB bound");
                }
                bytes.extend_from_slice(&chunk);
                if first.is_none() && self.is_meaningful(&bytes) {
                    first = Some(Instant::now().duration_since(offered));
                }
            }
            if !headers_valid || !self.is_valid(&bytes) {
                bail!("invalid status, content type, or response body");
            }
            Ok::<_, anyhow::Error>(bytes.len())
        }
        .await;
        let (bytes, error) = match result {
            Ok(bytes) => (bytes, None),
            Err(error) => (0, Some(error.to_string())),
        };
        Sample {
            finished_at: Instant::now(),
            workload: self.name.clone(),
            completion_us: micros(Instant::now().duration_since(offered)),
            first_us: first.map(micros),
            dispatch_us: micros(dispatched.duration_since(offered)),
            bytes,
            error,
        }
    }
}

// -----------------------------------------------------------------------------
// Micros: Benchmark data.
// -----------------------------------------------------------------------------

/// Express a duration in microseconds for bounded latency histograms.
fn micros(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

// -----------------------------------------------------------------------------
// Sample: One bounded HTTP exchange with timing and validation results.
// -----------------------------------------------------------------------------

/// One bounded HTTP exchange with timing and validation results.
#[derive(Serialize)]
pub struct Sample {
    /// Completion time used to distinguish measurement from drain.
    #[serde(skip)]
    pub finished_at: Instant,
    /// Workload that produced this sample.
    pub workload: String,
    /// Scheduled arrival to complete body receipt in microseconds.
    pub completion_us: u64,
    /// Scheduled arrival to first meaningful payload in microseconds.
    pub first_us: Option<u64>,
    /// Arrival scheduling delay in microseconds.
    pub dispatch_us: u64,
    /// Received body size; zero on failure.
    bytes: usize,
    /// Transport or validation failure, when present.
    pub error: Option<String>,
}

// -----------------------------------------------------------------------------
// Workloads: Benchmark data.
// -----------------------------------------------------------------------------
/// One workload or the fixed nine-chat/one-speech request mix.
pub struct Workloads {
    /// Prepared provider requests.
    items: Vec<Workload>,
}

impl Workloads {
    /// Prepare the selected request or the fixed mixed corpus.
    ///
    /// # Errors
    /// Returns unknown workload or invalid fixture errors.
    pub fn new(name: &str, base: &str) -> Result<Self> {
        let items = if name == "mixed" {
            vec![
                Workload::new("chat-short", base)?,
                Workload::new("speech-medium-pcm", base)?,
            ]
        } else {
            vec![Workload::new(name, base)?]
        };
        Ok(Self { items })
    }

    /// Select the next deterministic request by its global sequence number.
    pub fn selected(&self, iteration: u64) -> Workload {
        self.items[usize::from(self.items.len() == 2 && iteration % 10 == 9)].clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Default server bound on combined request text.
    const MAX_INPUT_CHARS: usize = 8_000;

    /// Validate benchmark fixtures and response boundaries.
    ///
    /// # Panics
    /// Panics if a fixture or response check regresses.
    #[test]
    fn split_events_and_terminal_records_are_validated() {
        let chat = Workload::new("chat-sse", "http://localhost").unwrap();
        assert!(!chat.is_meaningful(b"data: {\"choices\":[{\"delta\":{\"content\":\"Hi"));
        let event = b"data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n\n";
        assert!(chat.is_meaningful(event));
        assert!(!chat.is_valid(event));
        assert!(chat.is_valid(&[event.as_slice(), b"data: [DONE]\n\n"].concat()));
        let speech = Workload::new("speech-short-pcm-sse", "http://localhost").unwrap();
        assert!(!speech.is_valid(b"data: {\"type\":\"speech.audio.done\"}\n\n"));
        assert!(speech.is_valid(b"data: {\"type\":\"speech.audio.delta\",\"audio\":\"AAAA\"}\n\ndata: {\"type\":\"speech.audio.done\"}\n\n"));
    }

    /// Validate benchmark fixtures and response boundaries.
    ///
    /// # Panics
    /// Panics if a fixture or response check regresses.
    #[test]
    fn history_fits_limits_and_unknown_workloads_fail() {
        let long = Workload::new("chat-long", "http://localhost").unwrap();
        assert_eq!(long.body["messages"].as_array().unwrap().len(), 199);
        let messages = long.body["messages"].as_array().unwrap();
        let chars = messages
            .iter()
            .map(|message| {
                let text = message["content"].as_str().unwrap();
                text.chars().count()
            })
            .sum::<usize>();
        assert!(chars <= MAX_INPUT_CHARS);
        assert!(Workload::new("speech-short-flac", "http://localhost").is_err());
    }
}
