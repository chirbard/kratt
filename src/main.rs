#![deny(clippy::disallowed_methods)]
#![warn(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cognitive_complexity,
    clippy::too_many_lines,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::wildcard_imports,
    clippy::min_ident_chars,
)]

use std::error::Error;
use kratt::agent::{AgentDef, run_agent};
use kratt::git::git_init_if_needed;
use kratt::graph::load_graph;
use kratt::llm::Backend;
use kratt::prompt::{
    CONSOLIDATE_EXPLANATION, EXPLANATION, EXTRACT_EXPLANATION, RATIONALIZE_EXPLANATION,
    REIFY_EXPLANATION,
};

const AGENT_PIPELINE: [AgentDef; 5] = [
    AgentDef { label: "main",        explanation: EXPLANATION,             use_chat_context: false },
    AgentDef { label: "extract",     explanation: EXTRACT_EXPLANATION,     use_chat_context: false },
    AgentDef { label: "reify",       explanation: REIFY_EXPLANATION,       use_chat_context: false },
    AgentDef { label: "rationalize", explanation: RATIONALIZE_EXPLANATION, use_chat_context: false },
    AgentDef { label: "consolidate", explanation: CONSOLIDATE_EXPLANATION, use_chat_context: false },
];

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv()?;
    let backend = Backend::from_env()?;
    eprintln!("backend: {}", backend.label());

    git_init_if_needed()?;
    let mut graph = load_graph();

    let user_prompt = match std::env::args().nth(1) {
        Some(cli_arg) => cli_arg,
        None => {
            eprintln!("waiting for input...");
            let mut line = String::new();
            std::io::stdin().read_line(&mut line)?;
            line.trim().to_string()
        }
    };

    for agent_def in &AGENT_PIPELINE {
        if let Some(text) = run_agent(agent_def, &backend, &user_prompt, "", &mut graph).await? {
            println!("\n{text}");
        }
    }

    Ok(())
}
