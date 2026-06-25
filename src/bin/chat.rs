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
use std::io::{BufRead, Write};
use kratt::agent::{AgentDef, run_agent};
use kratt::conversation::{append, format_context, load_recent};
use kratt::git::git_init_if_needed;
use kratt::graph::load_graph;
use kratt::llm::Backend;
use kratt::prompt::{
    CONSOLIDATE_EXPLANATION, EXPLANATION, EXTRACT_EXPLANATION, RATIONALIZE_EXPLANATION,
    REIFY_EXPLANATION,
};

const CONTEXT_WINDOW: usize = 8;

const AGENT_PIPELINE: [AgentDef; 5] = [
    AgentDef { label: "main",        explanation: EXPLANATION,             use_chat_context: false },
    AgentDef { label: "extract",     explanation: EXTRACT_EXPLANATION,     use_chat_context: false },
    AgentDef { label: "reify",       explanation: REIFY_EXPLANATION,       use_chat_context: false },
    AgentDef { label: "rationalize", explanation: RATIONALIZE_EXPLANATION, use_chat_context: false },
    AgentDef { label: "consolidate", explanation: CONSOLIDATE_EXPLANATION, use_chat_context: true  },
];

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv()?;
    let backend = Backend::from_env()?;
    eprintln!("backend: {}", backend.label());

    git_init_if_needed()?;
    let mut graph = load_graph();

    eprintln!("chat mode — /quit to exit\n");

    loop {
        print!("You: ");
        std::io::stdout().flush()?;

        let mut raw_line = String::new();
        if std::io::stdin().lock().read_line(&mut raw_line)? == 0 {
            break;
        }
        let user_input = raw_line.trim().to_string();
        if user_input.is_empty() {
            continue;
        }
        if user_input == "/quit" || user_input == "/exit" {
            break;
        }

        append("user", &user_input);

        let recent = load_recent(CONTEXT_WINDOW + 1);
        let prior_count = recent.len().saturating_sub(1);
        let chat_context = format_context(&recent[..prior_count]);

        for agent_def in &AGENT_PIPELINE {
            if let Some(reply) =
                run_agent(agent_def, &backend, &user_input, &chat_context, &mut graph).await?
            {
                println!("\nKratt: {reply}\n");
                append("assistant", &reply);
            }
        }
    }

    Ok(())
}
