use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub enum LlmBackend {
    Anthropic,
    OpenAI,
}

pub struct LlmProvider {
    backend: LlmBackend,
    api_key: String,
    client: reqwest::blocking::Client,
}

impl LlmProvider {
    pub fn new(backend: LlmBackend, api_key: String) -> Self {
        Self {
            backend,
            api_key,
            client: reqwest::blocking::Client::new(),
        }
    }

    pub fn from_env() -> Option<Self> {
        if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            return Some(Self::new(LlmBackend::Anthropic, key));
        }
        if let Ok(key) = std::env::var("OPENAI_API_KEY") {
            return Some(Self::new(LlmBackend::OpenAI, key));
        }
        None
    }

    pub fn send_message(
        &self,
        messages: &[ChatMessage],
        schema_context: Option<&str>,
        db_type_label: Option<&str>,
    ) -> Result<String> {
        match self.backend {
            LlmBackend::Anthropic => self.send_anthropic(messages, schema_context, db_type_label),
            LlmBackend::OpenAI => self.send_openai(messages, schema_context, db_type_label),
        }
    }

    fn send_anthropic(
        &self,
        messages: &[ChatMessage],
        schema_context: Option<&str>,
        db_type_label: Option<&str>,
    ) -> Result<String> {
        let dialect = db_type_label.unwrap_or("SQL");
        let system = if let Some(schema) = schema_context {
            format!(
                "You are a helpful SQL assistant. The user has connected to a {} database with the following schema:\n\n{}\n\nHelp them write {} queries, explain results, and answer questions about their data. When generating SQL, always output it in a code block.",
                dialect, schema, dialect
            )
        } else {
            format!(
                "You are a helpful SQL assistant. Help users write {} queries and explain database concepts. When generating SQL, always output it in a code block.",
                dialect
            )
        };

        let api_messages: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": m.role,
                    "content": m.content
                })
            })
            .collect();

        let body = serde_json::json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 4096,
            "system": system,
            "messages": api_messages
        });

        let resp = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()?;

        let json: serde_json::Value = resp.json()?;

        if let Some(content) = json["content"].as_array() {
            if let Some(first) = content.first() {
                if let Some(text) = first["text"].as_str() {
                    return Ok(text.to_string());
                }
            }
        }

        if let Some(error) = json["error"]["message"].as_str() {
            return Err(anyhow::anyhow!("API error: {}", error));
        }

        Err(anyhow::anyhow!("Unexpected API response"))
    }

    fn send_openai(
        &self,
        messages: &[ChatMessage],
        schema_context: Option<&str>,
        db_type_label: Option<&str>,
    ) -> Result<String> {
        let dialect = db_type_label.unwrap_or("SQL");
        let system_msg = if let Some(schema) = schema_context {
            format!(
                "You are a helpful SQL assistant. The user has connected to a {} database with the following schema:\n\n{}\n\nHelp them write {} queries, explain results, and answer questions about their data. When generating SQL, always output it in a code block.",
                dialect, schema, dialect
            )
        } else {
            format!(
                "You are a helpful SQL assistant. Help users write {} queries and explain database concepts. When generating SQL, always output it in a code block.",
                dialect
            )
        };

        let mut api_messages = vec![serde_json::json!({
            "role": "system",
            "content": system_msg
        })];

        for m in messages {
            api_messages.push(serde_json::json!({
                "role": m.role,
                "content": m.content
            }));
        }

        let body = serde_json::json!({
            "model": "gpt-4o",
            "messages": api_messages,
            "max_tokens": 4096
        });

        let resp = self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()?;

        let json: serde_json::Value = resp.json()?;

        if let Some(choices) = json["choices"].as_array() {
            if let Some(first) = choices.first() {
                if let Some(content) = first["message"]["content"].as_str() {
                    return Ok(content.to_string());
                }
            }
        }

        if let Some(error) = json["error"]["message"].as_str() {
            return Err(anyhow::anyhow!("API error: {}", error));
        }

        Err(anyhow::anyhow!("Unexpected API response"))
    }
}
