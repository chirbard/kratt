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

mod agent;
mod git;
mod graph;
mod llm;
mod prompt;

use std::error::Error;
use agent::{AgentDef, run_agent};
use git::git_init_if_needed;
use graph::load_graph;
use llm::Backend;
use prompt::{
    CONSOLIDATE_EXPLANATION, EXPLANATION, EXTRACT_EXPLANATION, RATIONALIZE_EXPLANATION,
    REIFY_EXPLANATION,
};

const AGENT_PIPELINE: [AgentDef; 5] = [
    AgentDef { label: "main",        explanation: EXPLANATION,             show_reply: false },
    AgentDef { label: "extract",     explanation: EXTRACT_EXPLANATION,     show_reply: false },
    AgentDef { label: "reify",       explanation: REIFY_EXPLANATION,       show_reply: false },
    AgentDef { label: "rationalize", explanation: RATIONALIZE_EXPLANATION, show_reply: false },
    AgentDef { label: "consolidate", explanation: CONSOLIDATE_EXPLANATION, show_reply: true  },
];

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv()?;
    let backend = Backend::from_env()?;
    println!("backend: {}", backend.label());

    git_init_if_needed()?;
    let mut graph = load_graph();

    let user_prompt = match std::env::args().nth(1) {
        Some(cli_arg) => cli_arg,
        None => {
            println!("waiting for input...");
            let mut line = String::new();
            std::io::stdin().read_line(&mut line)?;
            line.trim().to_string()
        }
    };

    for agent_def in &AGENT_PIPELINE {
        run_agent(agent_def, &backend, &user_prompt, &mut graph).await?;
    }

    Ok(())
}
