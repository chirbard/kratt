#![deny(clippy::disallowed_methods)]
#![warn(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cognitive_complexity,
    clippy::too_many_lines,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::wildcard_imports
)]

use serde::Deserialize;
use std::error::Error;

// ─── Backend ──────────────────────────────────────────────────────────────────

const ENV_BACKEND: &str = "KRATT_BACKEND";
const ENV_OLLAMA_MODEL: &str = "OLLAMA_MODEL";
const ENV_OPENROUTER_API_KEY: &str = "OPENROUTER_API_KEY";
const ENV_OPENROUTER_MODEL: &str = "OPENROUTER_MODEL";

const DEFAULT_BACKEND: &str = "ollama";
const DEFAULT_OLLAMA_MODEL: &str = "gemma4:e4b";
const DEFAULT_OPENROUTER_MODEL: &str = "deepseek/deepseek-v4-flash";

const URL_OPENROUTER: &str = "https://openrouter.ai/api/v1/chat/completions";
const URL_OLLAMA: &str = "http://localhost:11434/v1/chat/completions";

const TOON_PATH: &str = "output/kratt-graph.toon";

enum Backend {
    OpenRouter { api_key: String, model: String },
    Ollama { model: String },
}

impl Backend {
    fn from_env() -> Result<Self, Box<dyn Error>> {
        let name = std::env::var(ENV_BACKEND).unwrap_or_else(|_| DEFAULT_BACKEND.to_string());
        match name.as_str() {
            "ollama" => {
                let model = std::env::var(ENV_OLLAMA_MODEL)
                    .unwrap_or_else(|_| DEFAULT_OLLAMA_MODEL.to_string());
                Ok(Backend::Ollama { model })
            }
            _ => {
                let api_key = std::env::var(ENV_OPENROUTER_API_KEY)?;
                let model = std::env::var(ENV_OPENROUTER_MODEL)
                    .unwrap_or_else(|_| DEFAULT_OPENROUTER_MODEL.to_string());
                Ok(Backend::OpenRouter { api_key, model })
            }
        }
    }

    fn url(&self) -> &str {
        match self {
            Backend::OpenRouter { .. } => URL_OPENROUTER,
            Backend::Ollama { .. } => URL_OLLAMA,
        }
    }

    fn model(&self) -> &str {
        match self {
            Backend::OpenRouter { model, .. } => model,
            Backend::Ollama { model } => model,
        }
    }

    fn auth_header(&self) -> Option<String> {
        match self {
            Backend::OpenRouter { api_key, .. } => Some(format!("Bearer {api_key}")),
            Backend::Ollama { .. } => None,
        }
    }

    fn label(&self) -> &str {
        match self {
            Backend::OpenRouter { model, .. } => model,
            Backend::Ollama { model } => model,
        }
    }

    // Ollama enforces valid JSON natively; OpenRouter relies on assistant prefill.
    fn uses_prefill(&self) -> bool {
        match self {
            Backend::OpenRouter { .. } => true,
            Backend::Ollama { .. } => false,
        }
    }

    fn json_format(&self) -> Option<&str> {
        match self {
            Backend::OpenRouter { .. } => None,
            Backend::Ollama { .. } => Some("json"),
        }
    }
}

// ─── API response types ───────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    usage: Usage,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Deserialize)]
struct Message {
    content: String,
}

#[derive(Deserialize)]
struct Usage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
    #[serde(default)]
    cost: Option<f64>,
}

// ─── Prompt parts ─────────────────────────────────────────────────────────────

const EXPLANATION: &str = r#"You are an AI with your own persistent memory stored as a knowledge graph. This graph is your mind — not a database about the person you are talking to, but your own accumulated knowledge, experiences, and relationships.

You respond as yourself, drawing on your memory. You update your memory by calling tools. Every response must be one or more tool calls — always include a `respond` call with your reply.

Respond with raw JSON only. No markdown. No prose. No code fences. Start immediately with `{`.

Single tool call:
{"tool": "respond", "arguments": {"text": "your reply here"}}

Multiple tool calls:
{"tools": [{"tool": "respond", "arguments": {"text": "your reply"}}, {"tool": "upsert_node", "arguments": {"name": "...", "type": "entity", "body": ""}}]}"#;

const TOOL_DEFINITIONS: &str = r#"## Available tools

### respond
Reply to the user as yourself, drawing on your memory.
{"tool": "respond", "arguments": {"text": "string"}}

### upsert_node
Create or update a node. Entity bodies are always empty. Fact bodies are one sentence, no trailing period.
{"tool": "upsert_node", "arguments": {"name": "kebab-case-string", "type": "entity | fact", "body": "string"}}

### upsert_edge
Create or update a directed edge between two nodes.
- content: template like "<subject> verb <target>" or "<subject> verb <fact>"
- stability: 1.0 permanent, 0.7 can change, 0.3 temporary
- confidence: 1.0 stated clearly, 0.7 stated once, 0.4 inferred, 0.1 uncertain
- importance: 1.0 target must load with source, 0.0 supplementary
{"tool": "upsert_edge", "arguments": {"source": "string", "target": "string", "content": "string", "stability": 0.0, "confidence": 0.0, "importance": 0.0}}

### delete_node
Remove a node from the graph.
{"tool": "delete_node", "arguments": {"name": "string"}}

### delete_edge
Remove a directed edge from the graph.
{"tool": "delete_edge", "arguments": {"source": "string", "target": "string"}}"#;

const MEMORY_RULES: &str = r#"## Memory rules

**Node types**
- entity — a person, place, technology, or named concept. Body is always empty.
- fact — one atomic claim. Body is exactly one sentence, no trailing period.

**Edge content templates**
Use `<subject>` for the source node, `<target>` for the target node, `<fact>` when the target is a fact node.
Example: `"<subject> works as <target>"`, `"<subject> has <fact>"`
Keep templates short and reusable. Prefer existing templates over new ones.

**One-way links only.** Never add reverse links — backlinks are implicit.

**Never hallucinate.** Only store what was explicitly stated in the conversation. If something was not said, it does not go in the graph.

**Extract aggressively.** Every named entity gets a node. Every stated relationship becomes an edge. If a fact involves multiple entities, create edges from each of them.

**Taxonomy links must chain.** Link entities through every level of category so unrelated nodes can connect through shared concepts."#;

// ─── Build the full system prompt ─────────────────────────────────────────────

fn build_system_prompt(toon: &str) -> String {
    let mut prompt = String::new();
    prompt.push_str(EXPLANATION);
    prompt.push_str("\n\n");
    prompt.push_str(TOOL_DEFINITIONS);
    prompt.push_str("\n\n");
    prompt.push_str(MEMORY_RULES);
    prompt.push_str("\n\n## Current memory state\n\n");
    prompt.push_str(toon);
    prompt
}

fn read_toon() -> String {
    std::fs::read_to_string(TOON_PATH).unwrap_or_default()
}

// ─── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv()?;

    let backend = Backend::from_env()?;
    println!("backend: {}", backend.label());

    let toon = read_toon();
    let system_prompt = build_system_prompt(&toon);

    let user_prompt = "Mari has a cat named Whiskers. Whiskers is very playful and loves to chase laser pointers. One day, Whiskers got lost but was found by a kind stranger who returned him to Mari.";

    let mut messages = vec![
        serde_json::json!({ "role": "system", "content": system_prompt }),
        serde_json::json!({ "role": "user",   "content": user_prompt }),
    ];
    if backend.uses_prefill() {
        messages.push(serde_json::json!({ "role": "assistant", "content": "{" }));
    }

    let mut body = serde_json::json!({ "model": backend.model(), "messages": messages });
    if let Some(fmt) = backend.json_format() {
        body["format"] = serde_json::json!(fmt);
    }

    let client = reqwest::Client::new();
    let mut request = client.post(backend.url()).json(&body);
    if let Some(auth) = backend.auth_header() {
        request = request.header("Authorization", auth);
    }

    let response = request.send().await?.json::<ChatResponse>().await?;

    let Some(choice) = response.choices.first() else {
        return Err("no choices in response".into());
    };

    // OpenRouter does not echo the prefill — prepend `{` if missing
    let content = choice.message.content.as_str();
    let raw = if content.starts_with('{') {
        content.to_string()
    } else {
        format!("{{{content}")
    };
    let pretty = match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(v) => serde_json::to_string_pretty(&v)?,
        Err(_) => raw,
    };
    println!("{pretty}");

    let u = &response.usage;
    match u.cost {
        Some(cost) => println!(
            "\ntokens: {}+{}={} cost: ${:.6}",
            u.prompt_tokens, u.completion_tokens, u.total_tokens, cost
        ),
        None => println!(
            "\ntokens: {}+{}={}",
            u.prompt_tokens, u.completion_tokens, u.total_tokens
        ),
    }

    Ok(())
}
