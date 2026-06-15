#![warn(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cognitive_complexity,
    clippy::too_many_lines,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::wildcard_imports,
)]

use petgraph::Graph;
use petgraph::dot::Dot;
use serde::Deserialize;
use std::collections::HashMap;
use std::error::Error;
use std::fs;

#[derive(Deserialize)]
struct Frontmatter {
    #[serde(rename = "type")]
    note_type: Option<String>,
    links: Option<Vec<Link>>,
}

#[derive(Deserialize)]
struct Link {
    target: String,
    content: Option<String>,
    stability: Option<f64>,
    confidence: Option<f64>,
    importance: Option<f64>,
}

fn strip_wikilink(s: &str) -> &str {
    s.trim_start_matches('[').trim_end_matches(']')
}

fn parse_frontmatter(text: &str) -> Option<Frontmatter> {
    let after_open = text.strip_prefix("---\n")?;
    let close = after_open.find("\n---")?;
    serde_yaml::from_str(&after_open[..close]).ok()
}

fn populate_sled(vault: &str, nodes: &sled::Tree, edges: &sled::Tree) -> Result<(), Box<dyn Error>> {
    nodes.clear()?;
    edges.clear()?;

    let mut edge_id: u64 = 0;

    for entry in fs::read_dir(vault)? {
        let entry = entry?;
        let path = entry.path();

        let ext = match path.extension() {
            Some(e) => match e.to_str() {
                Some(s) => s,
                None => continue,
            },
            None => continue,
        };
        if ext != "md" {
            continue;
        }

        let stem = match path.file_stem() {
            Some(s) => match s.to_str() {
                Some(s) => s.to_string(),
                None => continue,
            },
            None => continue,
        };

        if stem == "CLAUDE" || stem == "predicates" {
            continue;
        }

        let content = fs::read_to_string(&path)?;

        let fm = match parse_frontmatter(&content) {
            Some(fm) => fm,
            None => {
                nodes.insert(stem.as_bytes(), b"entity")?;
                continue;
            }
        };

        let note_type = fm.note_type.unwrap_or_else(|| "entity".to_string());
        nodes.insert(stem.as_bytes(), note_type.as_bytes())?;

        for link in fm.links.unwrap_or_default() {
            let target = strip_wikilink(&link.target).to_string();
            let content = link.content.unwrap_or_default();
            let stability = link.stability.unwrap_or(0.0);
            let confidence = link.confidence.unwrap_or(0.0);
            let importance = link.importance.unwrap_or(0.0);
            let val = format!("{}\t{}\t{}\t{}\t{}\t{}", stem, target, content, stability, confidence, importance);
            edges.insert(edge_id.to_be_bytes(), val.as_bytes())?;
            edge_id += 1;
        }
    }

    Ok(())
}

fn build_graph(nodes: &sled::Tree, edges: &sled::Tree) -> Result<Graph<String, String>, Box<dyn Error>> {
    let mut graph: Graph<String, String> = Graph::new();
    let mut indices: HashMap<String, _> = HashMap::new();

    for result in nodes.iter() {
        let (key, _) = result?;
        let name = String::from_utf8(key.to_vec())?;
        let idx = graph.add_node(name.clone());
        indices.insert(name, idx);
    }

    for result in edges.iter() {
        let (_, val) = result?;
        let s = String::from_utf8(val.to_vec())?;
        let mut parts = s.splitn(6, '\t');
        let (from, to, content, stability, confidence, importance) = match (
            parts.next(), parts.next(), parts.next(),
            parts.next(), parts.next(), parts.next(),
        ) {
            (Some(f), Some(t), Some(c), Some(s), Some(co), Some(i)) => {
                (f.to_string(), t.to_string(), c, s, co, i)
            }
            _ => continue,
        };

        let label = format!("{}\ns={} c={} i={}", content, stability, confidence, importance);

        let from_idx = *indices.entry(from.clone()).or_insert_with(|| graph.add_node(from));
        let to_idx = *indices.entry(to.clone()).or_insert_with(|| graph.add_node(to));
        graph.add_edge(from_idx, to_idx, label);
    }

    Ok(graph)
}

fn render_svg(dot_path: &str, svg_path: &str) -> Result<bool, Box<dyn Error>> {
    let probe = std::process::Command::new("dot").arg("-V").output();
    if probe.is_err() {
        return Ok(false);
    }

    let status = std::process::Command::new("dot")
        .args(["-Tsvg", dot_path, "-o", svg_path])
        .status()?;

    Ok(status.success())
}

fn main() -> Result<(), Box<dyn Error>> {
    let db = sled::open("kratt-graph.sled")?;
    let nodes = db.open_tree("nodes")?;
    let edges = db.open_tree("edges")?;

    println!("Populating sled from kratt-memory/...");
    populate_sled("kratt-memory", &nodes, &edges)?;
    println!("  {} nodes, {} edges written", nodes.len(), edges.len());

    println!("Building petgraph from sled...");
    let graph = build_graph(&nodes, &edges)?;
    println!(
        "  {} nodes, {} edges in graph",
        graph.node_count(),
        graph.edge_count()
    );

    fs::create_dir_all("output")?;

    let dot = format!("{}", Dot::with_config(&graph, &[]));
    fs::write("output/kratt-graph.dot", &dot)?;
    println!("Wrote output/kratt-graph.dot");

    match render_svg("output/kratt-graph.dot", "output/kratt-graph.svg")? {
        true => println!("Rendered output/kratt-graph.svg"),
        false => println!(
            "graphviz not found — install with: brew install graphviz\n\
             Then run: dot -Tsvg output/kratt-graph.dot -o output/kratt-graph.svg"
        ),
    }

    Ok(())
}
