use std::error::Error;

use crate::git::git_commit;
use crate::graph::{Edge, Graph, Node, delete_node, graph_to_json, insert_edge, normalize_id, remove_edges, save_graph};
use crate::llm::{Backend, call_llm, fix_json, normalize_response, print_usage};
use crate::prompt::build_system_prompt;

pub struct AgentDef {
    pub label: &'static str,
    pub explanation: &'static str,
    pub show_reply: bool,
}

enum ToolCall {
    Respond(String),
    UpsertNode { name: String, node_type: String, body: String, date: String },
    UpsertEdge {
        source: String,
        target: String,
        content: String,
        stability: f64,
        confidence: f64,
        importance: f64,
        subject: bool,
    },
    DeleteNode { name: String },
    DeleteEdge { source: String, target: String },
}

fn json_str(json: &serde_json::Value, field_name: &str) -> Option<String> {
    if let Some(field_value) = json.get(field_name)
        && let Some(text_value) = field_value.as_str()
    {
        return Some(text_value.to_string());
    }
    None
}

fn json_f64(json: &serde_json::Value, field_name: &str) -> f64 {
    match json.get(field_name) {
        Some(field_value) => field_value.as_f64().unwrap_or(0.0),
        None => 0.0,
    }
}

fn json_bool(json: &serde_json::Value, field_name: &str) -> bool {
    match json.get(field_name) {
        Some(field_value) => field_value.as_bool().unwrap_or(false),
        None => false,
    }
}

fn parse_single_tool(json: &serde_json::Value) -> Option<ToolCall> {
    let tool = json.get("tool")?.as_str()?;
    let args = json.get("arguments")?;
    match tool {
        "respond" => Some(ToolCall::Respond(json_str(args, "text")?)),
        "upsert_node" => Some(ToolCall::UpsertNode {
            name: json_str(args, "name")?,
            node_type: json_str(args, "type")?,
            body: json_str(args, "body").unwrap_or_default(),
            date: json_str(args, "date").unwrap_or_default(),
        }),
        "upsert_edge" => Some(ToolCall::UpsertEdge {
            source: json_str(args, "source")?,
            target: json_str(args, "target")?,
            content: json_str(args, "content").unwrap_or_default(),
            stability: json_f64(args, "stability"),
            confidence: json_f64(args, "confidence"),
            importance: json_f64(args, "importance"),
            subject: json_bool(args, "subject"),
        }),
        "delete_node" => Some(ToolCall::DeleteNode { name: json_str(args, "name")? }),
        "delete_edge" => Some(ToolCall::DeleteEdge {
            source: json_str(args, "source")?,
            target: json_str(args, "target")?,
        }),
        _ => None,
    }
}

fn parse_tool_calls(json: &serde_json::Value) -> Vec<ToolCall> {
    let mut calls = Vec::new();
    if let Some(tools) = json.get("tools") {
        if let Some(tool_array) = tools.as_array() {
            for item in tool_array {
                if let Some(call) = parse_single_tool(item) {
                    calls.push(call);
                }
            }
        }
        return calls;
    }
    if let Some(call) = parse_single_tool(json) {
        calls.push(call);
    }
    calls
}

fn execute_tool(call: ToolCall, graph: &mut Graph) -> Option<String> {
    match call {
        ToolCall::Respond(text) => Some(text),
        ToolCall::UpsertNode { name, node_type, body, date } => {
            graph.nodes.insert(normalize_id(&name), Node { node_type, body, date });
            None
        }
        ToolCall::UpsertEdge { source, target, content, stability, confidence, importance, subject } => {
            insert_edge(graph, Edge {
                source: normalize_id(&source),
                target: normalize_id(&target),
                content,
                stability,
                confidence,
                importance,
                subject,
            });
            None
        }
        ToolCall::DeleteNode { name } => {
            delete_node(graph, &normalize_id(&name));
            None
        }
        ToolCall::DeleteEdge { source, target } => {
            remove_edges(&mut graph.edges, &normalize_id(&source), &normalize_id(&target));
            None
        }
    }
}

fn collect_tool_calls(llm_output: &str) -> Vec<ToolCall> {
    let mut calls = Vec::new();

    // Try as-is
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(llm_output) {
        for call in parse_tool_calls(&parsed) {
            calls.push(call);
        }
        return calls;
    }
    // Try with a missing closing brace (model truncated the outer object)
    let with_brace = format!("{llm_output}}}");
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&with_brace) {
        for call in parse_tool_calls(&parsed) {
            calls.push(call);
        }
        return calls;
    }
    // Try each line separately (model emitted multiple JSON objects)
    for line in llm_output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(line) {
            for call in parse_tool_calls(&parsed) {
                calls.push(call);
            }
        }
    }

    calls
}

async fn collect_with_retry(backend: &Backend, normalized_response: &str) -> Vec<ToolCall> {
    let mut calls = collect_tool_calls(normalized_response);
    if calls.is_empty() {
        eprintln!("  json parse failed, retrying with fixer");
        match fix_json(backend, normalized_response).await {
            Ok(fixed) => {
                calls = collect_tool_calls(&fixed);
                if calls.is_empty() {
                    eprintln!("  fixer also failed — skipping");
                    println!("  {fixed}");
                }
            }
            Err(err) => eprintln!("  fixer error: {err}"),
        }
    }
    calls
}

pub async fn run_agent(
    agent_def: &AgentDef,
    backend: &Backend,
    user_message: &str,
    graph: &mut Graph,
) -> Result<(), Box<dyn Error>> {
    println!("\n[{}]", agent_def.label);
    let toon = toon::encode(&graph_to_json(graph), None);
    let system = build_system_prompt(agent_def.explanation, &toon);
    let response = call_llm(backend, &system, user_message).await?;
    let normalized_response = normalize_response(&response.content);
    let calls = collect_with_retry(backend, &normalized_response).await;
    if !calls.is_empty() {
        for call in calls {
            if let Some(text) = execute_tool(call, graph)
                && agent_def.show_reply
            {
                println!("\n{text}");
            }
        }
        save_graph(graph)?;
        git_commit(agent_def.label)?;
    }
    print_usage(&response);
    Ok(())
}
