use futures_util::StreamExt;
use serde::Deserialize;
use std::error::Error;

const ENV_BACKEND: &str = "KRATT_BACKEND";
const ENV_OLLAMA_MODEL: &str = "OLLAMA_MODEL";
const ENV_OPENROUTER_API_KEY: &str = "OPENROUTER_API_KEY";
const ENV_OPENROUTER_MODEL: &str = "OPENROUTER_MODEL";
const ENV_LLAMACPP_URL: &str = "LLAMACPP_URL";
const ENV_LLAMACPP_API_KEY: &str = "LLAMACPP_API_KEY";
const ENV_LLAMACPP_MODEL: &str = "LLAMACPP_MODEL";

const DEFAULT_BACKEND: &str = "ollama";
const DEFAULT_OLLAMA_MODEL: &str = "lfm2.5";
const DEFAULT_OPENROUTER_MODEL: &str = "deepseek/deepseek-v4-flash";
const DEFAULT_LLAMACPP_MODEL: &str = "default";
const OLLAMA_NUM_CTX: u32 = 16384;

const URL_OPENROUTER: &str = "https://openrouter.ai/api/v1/chat/completions";
const URL_OLLAMA: &str = "http://localhost:11434/api/chat";

pub enum Backend {
    OpenRouter { api_key: String, model: String },
    Ollama { model: String },
    LlamaCpp { base_url: String, api_key: String, model: String },
}

impl Backend {
    pub fn from_env() -> Result<Self, Box<dyn Error>> {
        let name = std::env::var(ENV_BACKEND).unwrap_or_else(|_| DEFAULT_BACKEND.to_string());
        match name.as_str() {
            "ollama" => {
                let model = std::env::var(ENV_OLLAMA_MODEL)
                    .unwrap_or_else(|_| DEFAULT_OLLAMA_MODEL.to_string());
                Ok(Backend::Ollama { model })
            }
            "llamacpp" => {
                let base_url = std::env::var(ENV_LLAMACPP_URL)?;
                let api_key = std::env::var(ENV_LLAMACPP_API_KEY)?;
                let model = std::env::var(ENV_LLAMACPP_MODEL)
                    .unwrap_or_else(|_| DEFAULT_LLAMACPP_MODEL.to_string());
                Ok(Backend::LlamaCpp { base_url, api_key, model })
            }
            _ => {
                let api_key = std::env::var(ENV_OPENROUTER_API_KEY)?;
                let model = std::env::var(ENV_OPENROUTER_MODEL)
                    .unwrap_or_else(|_| DEFAULT_OPENROUTER_MODEL.to_string());
                Ok(Backend::OpenRouter { api_key, model })
            }
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Backend::OpenRouter { model, .. } => model,
            Backend::Ollama { model } => model,
            Backend::LlamaCpp { model, .. } => model,
        }
    }
}

#[derive(Deserialize)]
struct OpenAiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    #[serde(default)]
    cost: Option<f64>,
}

#[derive(Deserialize)]
struct OpenAiChunk {
    choices: Vec<OpenAiChunkChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}

#[derive(Deserialize)]
struct OpenAiChunkChoice {
    delta: OpenAiDelta,
}

#[derive(Deserialize)]
struct OpenAiDelta {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Deserialize)]
struct OllamaMessage {
    content: String,
}

#[derive(Deserialize)]
struct OllamaChunk {
    message: OllamaMessage,
    done: bool,
    #[serde(default)]
    prompt_eval_count: Option<u32>,
    #[serde(default)]
    eval_count: Option<u32>,
}

pub struct LlmResponse {
    pub content: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub cost: Option<f64>,
    pub ttft: f64,
    pub gen_secs: f64,
}

fn clear_tok_line() {
    eprint!("\r                    \r");
}

fn timing_secs(
    t_start: std::time::Instant,
    t_first: Option<std::time::Instant>,
    t_end: std::time::Instant,
) -> (f64, f64) {
    match t_first {
        Some(first_instant) => (
            first_instant.duration_since(t_start).as_secs_f64(),
            t_end.duration_since(first_instant).as_secs_f64(),
        ),
        None => (0.0, 0.0),
    }
}

fn apply_openai_chunk(
    chunk: OpenAiChunk,
    content: &mut String,
    token_count: &mut u32,
    t_first: &mut Option<std::time::Instant>,
) {
    for choice in chunk.choices {
        if let Some(text) = choice.delta.content
            && !text.is_empty()
        {
            if t_first.is_none() {
                *t_first = Some(std::time::Instant::now());
            }
            content.push_str(&text);
            *token_count += 1;
            eprint!("\r  {token_count} tok");
        }
    }
}

async fn stream_ollama(
    model: &str,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<LlmResponse, Box<dyn Error>> {
    let body = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user",   "content": user_prompt },
        ],
        "stream": true,
        "format": "json",
        "options": { "num_ctx": OLLAMA_NUM_CTX },
    });
    let response = reqwest::Client::new().post(URL_OLLAMA).json(&body).send().await?;
    parse_ollama_stream(response).await
}

async fn parse_ollama_stream(response: reqwest::Response) -> Result<LlmResponse, Box<dyn Error>> {
    let t_start = std::time::Instant::now();
    let mut byte_stream = response.bytes_stream();
    let mut line_buffer = String::new();
    let mut content = String::new();
    let mut token_count = 0u32;
    let mut prompt_tokens = 0u32;
    let mut completion_tokens = 0u32;
    let mut t_first: Option<std::time::Instant> = None;
    while let Some(item) = byte_stream.next().await {
        line_buffer.push_str(&String::from_utf8_lossy(&item?));
        while let Some(newline_pos) = line_buffer.find('\n') {
            let line = line_buffer[..newline_pos].trim().to_string();
            line_buffer = line_buffer[newline_pos + 1..].to_string();
            if line.is_empty() {
                continue;
            }
            let Ok(chunk) = serde_json::from_str::<OllamaChunk>(&line) else { continue };
            content.push_str(&chunk.message.content);
            if chunk.done {
                prompt_tokens = chunk.prompt_eval_count.unwrap_or(0);
                completion_tokens = chunk.eval_count.unwrap_or(0);
            } else {
                if t_first.is_none() {
                    t_first = Some(std::time::Instant::now());
                }
                token_count += 1;
                eprint!("\r  {token_count} tok");
            }
        }
    }
    clear_tok_line();
    let (ttft, gen_secs) = timing_secs(t_start, t_first, std::time::Instant::now());
    Ok(LlmResponse { content, prompt_tokens, completion_tokens, cost: None, ttft, gen_secs })
}

async fn parse_openai_sse_stream(
    response: reqwest::Response,
    prepend: &str,
) -> Result<LlmResponse, Box<dyn Error>> {
    let t_start = std::time::Instant::now();
    let mut byte_stream = response.bytes_stream();
    let mut line_buffer = String::new();
    let mut content = String::new();
    let mut token_count = 0u32;
    let mut prompt_tokens = 0u32;
    let mut completion_tokens = 0u32;
    let mut cost: Option<f64> = None;
    let mut t_first: Option<std::time::Instant> = None;
    while let Some(item) = byte_stream.next().await {
        line_buffer.push_str(&String::from_utf8_lossy(&item?));
        while let Some(newline_pos) = line_buffer.find('\n') {
            let line = line_buffer[..newline_pos].trim().to_string();
            line_buffer = line_buffer[newline_pos + 1..].to_string();
            let Some(data) = line.strip_prefix("data: ") else { continue };
            if data == "[DONE]" {
                continue;
            }
            let Ok(chunk) = serde_json::from_str::<OpenAiChunk>(data) else { continue };
            if let Some(ref usage_data) = chunk.usage {
                prompt_tokens = usage_data.prompt_tokens;
                completion_tokens = usage_data.completion_tokens;
                cost = usage_data.cost;
            }
            apply_openai_chunk(chunk, &mut content, &mut token_count, &mut t_first);
        }
    }
    clear_tok_line();
    let (ttft, gen_secs) = timing_secs(t_start, t_first, std::time::Instant::now());
    if !prepend.is_empty() {
        content = format!("{prepend}{content}");
    }
    Ok(LlmResponse { content, prompt_tokens, completion_tokens, cost, ttft, gen_secs })
}

async fn post_openai_compat(
    url: &str,
    api_key: &str,
    body: serde_json::Value,
    prepend: &str,
) -> Result<LlmResponse, Box<dyn Error>> {
    let response = reqwest::Client::new()
        .post(url)
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&body)
        .send()
        .await?;
    parse_openai_sse_stream(response, prepend).await
}

async fn stream_openrouter(
    api_key: &str,
    model: &str,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<LlmResponse, Box<dyn Error>> {
    let body = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system",    "content": system_prompt },
            { "role": "user",      "content": user_prompt },
            { "role": "assistant", "content": "{" },
        ],
        "stream": true,
        "stream_options": { "include_usage": true },
    });
    post_openai_compat(URL_OPENROUTER, api_key, body, "{").await
}

async fn stream_llamacpp(
    base_url: &str,
    api_key: &str,
    model: &str,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<LlmResponse, Box<dyn Error>> {
    let endpoint = format!("{base_url}/v1/chat/completions");
    let body = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user",   "content": user_prompt },
        ],
        "stream": true,
        "stream_options": { "include_usage": true },
    });
    post_openai_compat(&endpoint, api_key, body, "").await
}

pub async fn call_llm(
    backend: &Backend,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<LlmResponse, Box<dyn Error>> {
    match backend {
        Backend::Ollama { model } => stream_ollama(model, system_prompt, user_prompt).await,
        Backend::OpenRouter { api_key, model } => {
            stream_openrouter(api_key, model, system_prompt, user_prompt).await
        }
        Backend::LlamaCpp { base_url, api_key, model } => {
            stream_llamacpp(base_url, api_key, model, system_prompt, user_prompt).await
        }
    }
}

fn strip_fences(text: &str) -> &str {
    let text = text.trim();
    let text = if let Some(rest) = text.strip_prefix("```json") {
        rest
    } else if let Some(rest) = text.strip_prefix("```") {
        rest
    } else {
        text
    };
    let text = if let Some(inner) = text.strip_suffix("```") {
        inner
    } else {
        text
    };
    text.trim()
}

pub fn normalize_response(content: &str) -> String {
    let content = strip_fences(content);
    if content.starts_with('{') {
        content.to_string()
    } else {
        format!("{{{content}")
    }
}

pub async fn fix_json(backend: &Backend, broken: &str) -> Result<String, Box<dyn Error>> {
    let system = "Fix the following malformed JSON and return only valid JSON, nothing else. No markdown, no explanation.";
    let response = call_llm(backend, system, broken).await?;
    Ok(normalize_response(&response.content))
}

pub fn print_usage(response: &LlmResponse) {
    let tokens_per_sec = if response.gen_secs > 0.0 {
        response.completion_tokens as f64 / response.gen_secs
    } else {
        0.0
    };
    let total = response.prompt_tokens + response.completion_tokens;
    print!(
        "  tokens: {}+{}={}  ttft: {:.2}s  {:.1}tok/s",
        response.prompt_tokens, response.completion_tokens, total, response.ttft, tokens_per_sec,
    );
    if let Some(cost) = response.cost {
        print!("  cost: ${cost:.6}");
    }
    println!();
}
