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
use std::collections::HashMap;
use std::error::Error;
use std::process::Command;

// ─── Backend ──────────────────────────────────────────────────────────────────

const ENV_BACKEND: &str = "KRATT_BACKEND";
const ENV_OLLAMA_MODEL: &str = "OLLAMA_MODEL";
const ENV_OPENROUTER_API_KEY: &str = "OPENROUTER_API_KEY";
const ENV_OPENROUTER_MODEL: &str = "OPENROUTER_MODEL";

const DEFAULT_BACKEND: &str = "ollama";
const DEFAULT_OLLAMA_MODEL: &str = "gemma4:e4b";
const DEFAULT_OPENROUTER_MODEL: &str = "deepseek/deepseek-v4-flash";
const OLLAMA_NUM_CTX: u32 = 16384;

const URL_OPENROUTER: &str = "https://openrouter.ai/api/v1/chat/completions";
const URL_OLLAMA: &str = "http://localhost:11434/api/chat";

const DATA_DIR: &str = "kratt-data";
const TOON_PATH: &str = "kratt-data/kratt-graph.toon";

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

    fn label(&self) -> &str {
        match self {
            Backend::OpenRouter { model, .. } => model,
            Backend::Ollama { model } => model,
        }
    }
}

// ─── API response types ───────────────────────────────────────────────────────

// OpenRouter / OpenAI-compatible
#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
    usage: OpenAiUsage,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}

#[derive(Deserialize)]
struct OpenAiMessage {
    content: String,
}

#[derive(Deserialize)]
struct OpenAiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    #[serde(default)]
    cost: Option<f64>,
}

// Ollama native /api/chat
#[derive(Deserialize)]
struct OllamaResponse {
    message: OllamaMessage,
    #[serde(default)]
    prompt_eval_count: Option<u32>,
    #[serde(default)]
    eval_count: Option<u32>,
}

#[derive(Deserialize)]
struct OllamaMessage {
    content: String,
}

// Unified response used through the rest of the code
struct LlmResponse {
    content: String,
    prompt_tokens: u32,
    completion_tokens: u32,
    cost: Option<f64>,
}

// ─── Graph ────────────────────────────────────────────────────────────────────

struct Graph {
    nodes: HashMap<String, String>,
    edges: Vec<Edge>,
}

struct Edge {
    source: String,
    target: String,
    content: String,
    stability: f64,
    confidence: f64,
    importance: f64,
}

impl Graph {
    fn new() -> Self {
        Graph {
            nodes: HashMap::new(),
            edges: Vec::new(),
        }
    }
}

enum ToonSection {
    None,
    Edges,
    Nodes,
}

fn parse_toon_row(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut chars = line.trim().chars().peekable();
    while chars.peek().is_some() {
        if chars.peek() == Some(&'"') {
            let _ = chars.next();
            let mut field = String::new();
            loop {
                match chars.next() {
                    Some('"') | None => break,
                    Some(c) => field.push(c),
                }
            }
            if chars.peek() == Some(&',') {
                let _ = chars.next();
            }
            fields.push(field);
        } else {
            let mut field = String::new();
            loop {
                match chars.peek() {
                    Some(&',') | None => break,
                    _ => {
                        if let Some(c) = chars.next() {
                            field.push(c);
                        }
                    }
                }
            }
            if chars.peek() == Some(&',') {
                let _ = chars.next();
            }
            fields.push(field);
        }
    }
    fields
}

fn load_graph() -> Graph {
    let toon = match std::fs::read_to_string(TOON_PATH) {
        Ok(s) => s,
        Err(_) => return Graph::new(),
    };
    let mut graph = Graph::new();
    let mut section = ToonSection::None;
    for line in toon.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("edges[") {
            section = ToonSection::Edges;
            continue;
        }
        if trimmed.starts_with("nodes[") {
            section = ToonSection::Nodes;
            continue;
        }
        match section {
            ToonSection::None => {}
            ToonSection::Edges => {
                let f = parse_toon_row(trimmed);
                if f.len() < 6 {
                    continue;
                }
                // TOON encodes edge columns alphabetically: confidence, content, importance, source, stability, target
                graph.edges.push(Edge {
                    confidence: f[0].parse().unwrap_or(0.0),
                    content: f[1].clone(),
                    importance: f[2].parse().unwrap_or(0.0),
                    source: f[3].clone(),
                    stability: f[4].parse().unwrap_or(0.0),
                    target: f[5].clone(),
                });
            }
            ToonSection::Nodes => {
                let f = parse_toon_row(trimmed);
                if f.len() < 2 {
                    continue;
                }
                graph.nodes.insert(f[0].clone(), f[1].clone());
            }
        }
    }
    graph
}

fn graph_to_json(graph: &Graph) -> serde_json::Value {
    let mut node_names: Vec<&String> = graph.nodes.keys().collect();
    node_names.sort();
    let mut nodes_json: Vec<serde_json::Value> = Vec::new();
    for name in &node_names {
        let node_type = match graph.nodes.get(*name) {
            Some(t) => t.as_str(),
            None => "entity",
        };
        nodes_json.push(serde_json::json!({ "id": name, "type": node_type }));
    }
    let mut edges_json: Vec<serde_json::Value> = Vec::new();
    for e in &graph.edges {
        edges_json.push(serde_json::json!({
            "source": e.source, "target": e.target, "content": e.content,
            "stability": e.stability, "confidence": e.confidence, "importance": e.importance,
        }));
    }
    serde_json::json!({ "nodes": nodes_json, "edges": edges_json })
}

fn save_graph(graph: &Graph) -> Result<(), Box<dyn Error>> {
    std::fs::create_dir_all(DATA_DIR)?;
    std::fs::write(TOON_PATH, toon::encode(&graph_to_json(graph), None))?;
    Ok(())
}

// ─── Git ──────────────────────────────────────────────────────────────────────

fn run_git(args: &[&str]) -> Result<(), Box<dyn Error>> {
    let status = Command::new("git").args(args).status()?;
    if !status.success() {
        return Err(format!("git {args:?} failed with {status}").into());
    }
    Ok(())
}

fn git_init_if_needed() -> Result<(), Box<dyn Error>> {
    let git_dir = std::path::Path::new(DATA_DIR).join(".git");
    if git_dir.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(DATA_DIR)?;
    run_git(&["init", DATA_DIR])?;
    run_git(&["-C", DATA_DIR, "config", "user.email", "kratt@local"])?;
    run_git(&["-C", DATA_DIR, "config", "user.name", "kratt"])?;
    save_graph(&Graph::new())?;
    run_git(&["-C", DATA_DIR, "add", "kratt-graph.toon"])?;
    run_git(&["-C", DATA_DIR, "commit", "-m", "init: empty memory state"])?;
    Ok(())
}

fn git_commit(label: &str) -> Result<(), Box<dyn Error>> {
    run_git(&["-C", DATA_DIR, "add", "kratt-graph.toon"])?;
    // git diff --cached --quiet exits 0 when nothing is staged, 1 when there are changes
    let staged = Command::new("git")
        .args(["-C", DATA_DIR, "diff", "--cached", "--quiet"])
        .status()?;
    if staged.success() {
        return Ok(());
    }
    run_git(&[
        "-C",
        DATA_DIR,
        "commit",
        "-m",
        &format!("{label}: update memory"),
    ])?;
    Ok(())
}

// ─── Tool calls ───────────────────────────────────────────────────────────────

enum ToolCall {
    Respond(String),
    UpsertNode {
        name: String,
        node_type: String,
    },
    UpsertEdge {
        source: String,
        target: String,
        content: String,
        stability: f64,
        confidence: f64,
        importance: f64,
    },
    DeleteNode {
        name: String,
    },
    DeleteEdge {
        source: String,
        target: String,
    },
}

fn json_str(v: &serde_json::Value, key: &str) -> Option<String> {
    if let Some(val) = v.get(key)
        && let Some(s) = val.as_str()
    {
        return Some(s.to_string());
    }
    None
}

fn json_f64(v: &serde_json::Value, key: &str) -> f64 {
    match v.get(key) {
        Some(val) => val.as_f64().unwrap_or(0.0),
        None => 0.0,
    }
}

fn parse_single_tool(v: &serde_json::Value) -> Option<ToolCall> {
    let tool = v.get("tool")?.as_str()?;
    let args = v.get("arguments")?;
    match tool {
        "respond" => Some(ToolCall::Respond(json_str(args, "text")?)),
        "upsert_node" => Some(ToolCall::UpsertNode {
            name: json_str(args, "name")?,
            node_type: json_str(args, "type")?,
        }),
        "upsert_edge" => Some(ToolCall::UpsertEdge {
            source: json_str(args, "source")?,
            target: json_str(args, "target")?,
            content: json_str(args, "content").unwrap_or_default(),
            stability: json_f64(args, "stability"),
            confidence: json_f64(args, "confidence"),
            importance: json_f64(args, "importance"),
        }),
        "delete_node" => Some(ToolCall::DeleteNode {
            name: json_str(args, "name")?,
        }),
        "delete_edge" => Some(ToolCall::DeleteEdge {
            source: json_str(args, "source")?,
            target: json_str(args, "target")?,
        }),
        _ => None,
    }
}

fn parse_tool_calls(v: &serde_json::Value) -> Vec<ToolCall> {
    let mut calls = Vec::new();
    if let Some(tools) = v.get("tools") {
        if let Some(arr) = tools.as_array() {
            for item in arr {
                if let Some(call) = parse_single_tool(item) {
                    calls.push(call);
                }
            }
        }
        return calls;
    }
    if let Some(call) = parse_single_tool(v) {
        calls.push(call);
    }
    calls
}

fn remove_edges(edges: &mut Vec<Edge>, source: &str, target: &str) {
    let mut i = 0;
    while i < edges.len() {
        if edges[i].source == source && edges[i].target == target {
            edges.remove(i);
        } else {
            i += 1;
        }
    }
}

fn execute_tool(call: ToolCall, graph: &mut Graph) -> Option<String> {
    match call {
        ToolCall::Respond(text) => Some(text),
        ToolCall::UpsertNode { name, node_type } => {
            graph.nodes.insert(name, node_type);
            None
        }
        ToolCall::UpsertEdge {
            source,
            target,
            content,
            stability,
            confidence,
            importance,
        } => {
            remove_edges(&mut graph.edges, &source, &target);
            graph.edges.push(Edge {
                source,
                target,
                content,
                stability,
                confidence,
                importance,
            });
            None
        }
        ToolCall::DeleteNode { name } => {
            graph.nodes.remove(&name);
            let mut i = 0;
            while i < graph.edges.len() {
                if graph.edges[i].source == name || graph.edges[i].target == name {
                    graph.edges.remove(i);
                } else {
                    i += 1;
                }
            }
            None
        }
        ToolCall::DeleteEdge { source, target } => {
            remove_edges(&mut graph.edges, &source, &target);
            None
        }
    }
}

// ─── Prompt parts ─────────────────────────────────────────────────────────────

const JSON_OUTPUT_RULES: &str = r#"Respond with raw JSON only. No markdown. No prose. No code fences. Start immediately with `{`.

Single tool call:
{"tool": "respond", "arguments": {"text": "your reply here"}}

Multiple tool calls:
{"tools": [{"tool": "respond", "arguments": {"text": "your reply"}}, {"tool": "upsert_node", "arguments": {"name": "...", "type": "entity", "body": ""}}]}"#;

const EXPLANATION: &str = r#"You are an AI with your own persistent memory stored as a knowledge graph. This graph is your mind — your accumulated knowledge, experiences, and relationships.

Extract entities and relationships from the user message and add them to the graph using tool calls. Do not include a respond call — the final agent in the pipeline will answer the user."#;

const EXTRACT_EXPLANATION: &str = r#"You are an aggressive extraction agent for a memory graph. Extract every entity and relationship from the user message that is not yet in the graph.

- Every named person, place, object, technology, or concept gets a node
- Every stated relationship, action, or property becomes an edge
- When uncertain, add it anyway — over-extraction is corrected in the next step
- Do not include a respond call"#;

const RATIONALIZE_EXPLANATION: &str = r#"You are a rationalization agent for a memory graph. Review recent additions and remove anything not directly supported by what was stated.

- Remove entries inferred from training knowledge rather than stated directly
- Remove duplicates with different wording
- Correct dishonest confidence values
- Use delete_node and delete_edge — when uncertain, keep it
- Do not include a respond call"#;

const CONSOLIDATE_EXPLANATION: &str = r#"You are the final agent in a memory pipeline. After updating memory, you respond to the user.

First, update the graph:
- Add missing taxonomy links so every named concept links through to its category
- Normalise inconsistent edge content templates
- Add relationships clearly implied by two or more existing edges

Then include a respond call with your reply to the user's original message, drawing on the complete memory graph. This is the reply the user will see — write it as yourself, not as a summary of what you did."#;

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

fn build_system_prompt(explanation: &str, toon: &str) -> String {
    let mut prompt = String::new();
    prompt.push_str(explanation);
    prompt.push_str("\n\n");
    prompt.push_str(JSON_OUTPUT_RULES);
    prompt.push_str("\n\n");
    prompt.push_str(TOOL_DEFINITIONS);
    prompt.push_str("\n\n");
    prompt.push_str(MEMORY_RULES);
    prompt.push_str("\n\n## Current memory state\n\n");
    prompt.push_str(toon);
    prompt
}

// ─── HTTP ─────────────────────────────────────────────────────────────────────

async fn call_ollama(
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
        "stream": false,
        "format": "json",
        "options": { "num_ctx": OLLAMA_NUM_CTX },
    });
    let resp: OllamaResponse = reqwest::Client::new()
        .post(URL_OLLAMA)
        .json(&body)
        .send()
        .await?
        .json()
        .await?;
    let prompt = resp.prompt_eval_count.unwrap_or(0);
    let completion = resp.eval_count.unwrap_or(0);
    Ok(LlmResponse {
        content: resp.message.content,
        prompt_tokens: prompt,
        completion_tokens: completion,
        cost: None,
    })
}

async fn call_openrouter(
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
    });
    let resp: OpenAiResponse = reqwest::Client::new()
        .post(URL_OPENROUTER)
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&body)
        .send()
        .await?
        .json()
        .await?;
    let Some(choice) = resp.choices.into_iter().next() else {
        return Err("no choices in response".into());
    };
    Ok(LlmResponse {
        content: choice.message.content,
        prompt_tokens: resp.usage.prompt_tokens,
        completion_tokens: resp.usage.completion_tokens,
        cost: resp.usage.cost,
    })
}

async fn call_llm(
    backend: &Backend,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<LlmResponse, Box<dyn Error>> {
    match backend {
        Backend::Ollama { model } => call_ollama(model, system_prompt, user_prompt).await,
        Backend::OpenRouter { api_key, model } => {
            call_openrouter(api_key, model, system_prompt, user_prompt).await
        }
    }
}

fn normalize_response(content: &str) -> String {
    let content = content.trim();
    if content.starts_with('{') {
        content.to_string()
    } else {
        format!("{{{content}")
    }
}

fn print_usage(r: &LlmResponse) {
    match r.cost {
        Some(cost) => println!(
            "  tokens: {}+{}={} cost: ${:.6}",
            r.prompt_tokens,
            r.completion_tokens,
            r.prompt_tokens + r.completion_tokens,
            cost
        ),
        None => println!(
            "  tokens: {}+{}={}",
            r.prompt_tokens,
            r.completion_tokens,
            r.prompt_tokens + r.completion_tokens
        ),
    }
}

// ─── Agent pipeline ───────────────────────────────────────────────────────────

struct AgentDef {
    label: &'static str,
    explanation: &'static str,
    show_reply: bool,
}

fn collect_tool_calls(raw: &str) -> Vec<ToolCall> {
    let mut calls = Vec::new();

    // Try as-is
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
        for call in parse_tool_calls(&v) {
            calls.push(call);
        }
        return calls;
    }
    // Try with a missing closing brace (model truncated the outer object)
    let with_brace = format!("{raw}}}");
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&with_brace) {
        for call in parse_tool_calls(&v) {
            calls.push(call);
        }
        return calls;
    }
    // Try each line separately (model emitted multiple JSON objects)
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            for call in parse_tool_calls(&v) {
                calls.push(call);
            }
        }
    }

    calls
}

async fn run_agent(
    def: &AgentDef,
    backend: &Backend,
    user_message: &str,
    graph: &mut Graph,
) -> Result<(), Box<dyn Error>> {
    println!("\n[{}]", def.label);
    let toon = toon::encode(&graph_to_json(graph), None);
    let system = build_system_prompt(def.explanation, &toon);
    let response = call_llm(backend, &system, user_message).await?;
    let raw = normalize_response(&response.content);
    let calls = collect_tool_calls(&raw);
    if calls.is_empty() {
        eprintln!("  could not parse any tool calls");
        println!("  {raw}");
    } else {
        for call in calls {
            if let Some(text) = execute_tool(call, graph)
                && def.show_reply
            {
                println!("\n{text}");
            }
        }
        save_graph(graph)?;
        git_commit(def.label)?;
    }
    print_usage(&response);
    Ok(())
}

// ─── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv()?;
    let backend = Backend::from_env()?;
    println!("backend: {}", backend.label());

    git_init_if_needed()?;
    let mut graph = load_graph();

    let user_prompt = match std::env::args().nth(1) {
        Some(arg) => arg,
        None => {
            println!("waiting for input...");
            let mut line = String::new();
            std::io::stdin().read_line(&mut line)?;
            line.trim().to_string()
        }
    };

    let agents = [
        AgentDef {
            label: "main",
            explanation: EXPLANATION,
            show_reply: false,
        },
        AgentDef {
            label: "extract",
            explanation: EXTRACT_EXPLANATION,
            show_reply: false,
        },
        AgentDef {
            label: "rationalize",
            explanation: RATIONALIZE_EXPLANATION,
            show_reply: false,
        },
        AgentDef {
            label: "consolidate",
            explanation: CONSOLIDATE_EXPLANATION,
            show_reply: true,
        },
    ];
    for def in &agents {
        run_agent(def, &backend, &user_prompt, &mut graph).await?;
    }

    Ok(())
}
