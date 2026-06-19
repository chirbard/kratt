#![warn(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cognitive_complexity,
    clippy::too_many_lines,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::wildcard_imports
)]

use std::collections::HashMap;
use std::error::Error;
use std::fmt::Write as FmtWrite;

const DEFAULT_TOON: &str = "kratt-data/kratt-graph.toon";
const OUT_DIR: &str = "output";
const OUT_GEXF: &str = "output/kratt-graph.gexf";
const OUT_DOT: &str = "output/kratt-graph.dot";
const OUT_SVG: &str = "output/kratt-graph.svg";

// ─── Types ────────────────────────────────────────────────────────────────────

struct Edge {
    source: String,
    target: String,
    content: String,
    stability: f64,
    confidence: f64,
    importance: f64,
}

type NodeMap = HashMap<String, String>;

enum ToonSection {
    None,
    Edges,
    Nodes,
}

// ─── TOON parsing ─────────────────────────────────────────────────────────────

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

fn load_toon(path: &str) -> Result<(NodeMap, Vec<Edge>), Box<dyn Error>> {
    let raw = std::fs::read_to_string(path)?;
    let mut nodes: NodeMap = HashMap::new();
    let mut edges: Vec<Edge> = Vec::new();
    let mut section = ToonSection::None;
    for line in raw.lines() {
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
                // alphabetical: confidence, content, importance, source, stability, target
                edges.push(Edge {
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
                nodes.insert(f[0].clone(), f[1].clone());
            }
        }
    }
    Ok((nodes, edges))
}

// ─── Output formats ───────────────────────────────────────────────────────────

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

fn write_gexf(nodes: &NodeMap, edges: &[Edge]) -> Result<String, Box<dyn Error>> {
    let mut all_nodes: Vec<String> = nodes.keys().cloned().collect();
    for e in edges {
        if !nodes.contains_key(&e.source) {
            all_nodes.push(e.source.clone());
        }
        if !nodes.contains_key(&e.target) {
            all_nodes.push(e.target.clone());
        }
    }
    all_nodes.sort();
    all_nodes.dedup();

    let mut out = String::new();
    writeln!(out, r#"<?xml version="1.0" encoding="UTF-8"?>"#)?;
    writeln!(out, r#"<gexf xmlns="http://gexf.net/1.3" version="1.3">"#)?;
    writeln!(out, r#"  <graph defaultedgetype="directed">"#)?;
    writeln!(out, r#"    <attributes class="edge">"#)?;
    writeln!(out, r#"      <attribute id="0" title="content" type="string"/>"#)?;
    writeln!(out, r#"      <attribute id="1" title="stability" type="float"/>"#)?;
    writeln!(out, r#"      <attribute id="2" title="confidence" type="float"/>"#)?;
    writeln!(out, r#"      <attribute id="3" title="importance" type="float"/>"#)?;
    writeln!(out, r#"    </attributes>"#)?;
    writeln!(out, r#"    <nodes>"#)?;
    for name in &all_nodes {
        writeln!(out, r#"      <node id="{n}" label="{n}"/>"#, n = xml_escape(name))?;
    }
    writeln!(out, r#"    </nodes>"#)?;
    writeln!(out, r#"    <edges>"#)?;
    for (i, e) in edges.iter().enumerate() {
        writeln!(
            out,
            r#"      <edge id="{i}" source="{}" target="{}">"#,
            xml_escape(&e.source),
            xml_escape(&e.target)
        )?;
        writeln!(out, r#"        <attvalues>"#)?;
        writeln!(out, r#"          <attvalue for="0" value="{}"/>"#, xml_escape(&e.content))?;
        writeln!(out, r#"          <attvalue for="1" value="{}"/>"#, e.stability)?;
        writeln!(out, r#"          <attvalue for="2" value="{}"/>"#, e.confidence)?;
        writeln!(out, r#"          <attvalue for="3" value="{}"/>"#, e.importance)?;
        writeln!(out, r#"        </attvalues>"#)?;
        writeln!(out, r#"      </edge>"#)?;
    }
    writeln!(out, r#"    </edges>"#)?;
    writeln!(out, r#"  </graph>"#)?;
    writeln!(out, r#"</gexf>"#)?;
    Ok(out)
}

fn write_dot(nodes: &NodeMap, edges: &[Edge]) -> Result<String, Box<dyn Error>> {
    let mut out = String::new();
    writeln!(out, "digraph {{")?;
    writeln!(out, "  rankdir=LR;")?;
    let mut node_names: Vec<&String> = nodes.keys().collect();
    node_names.sort();
    for name in node_names {
        writeln!(out, r#"  "{}";"#, name.replace('"', r#"\""#))?;
    }
    for e in edges {
        let label = format!(
            "{}\\ns={:.1} c={:.1} i={:.1}",
            e.content, e.stability, e.confidence, e.importance
        );
        writeln!(
            out,
            r#"  "{}" -> "{}" [label="{}"];"#,
            e.source.replace('"', r#"\""#),
            e.target.replace('"', r#"\""#),
            label.replace('"', r#"\""#),
        )?;
    }
    writeln!(out, "}}")?;
    Ok(out)
}

fn render_svg(dot_path: &str, svg_path: &str) -> Result<bool, Box<dyn Error>> {
    if std::process::Command::new("dot").arg("-V").output().is_err() {
        return Ok(false);
    }
    let status = std::process::Command::new("dot")
        .args(["-Tsvg", dot_path, "-o", svg_path])
        .status()?;
    Ok(status.success())
}

// ─── Main ─────────────────────────────────────────────────────────────────────

fn main() -> Result<(), Box<dyn Error>> {
    let toon_path = std::env::args().nth(1).unwrap_or_else(|| DEFAULT_TOON.to_string());
    let (nodes, edges) = load_toon(&toon_path)?;
    println!("loaded {} nodes, {} edges from {toon_path}", nodes.len(), edges.len());

    std::fs::create_dir_all(OUT_DIR)?;

    let gexf = write_gexf(&nodes, &edges)?;
    std::fs::write(OUT_GEXF, &gexf)?;
    println!("wrote {OUT_GEXF}");

    let dot = write_dot(&nodes, &edges)?;
    std::fs::write(OUT_DOT, &dot)?;
    println!("wrote {OUT_DOT}");

    match render_svg(OUT_DOT, OUT_SVG)? {
        true => println!("rendered {OUT_SVG}"),
        false => println!("graphviz not found — skipping SVG (brew install graphviz)"),
    }

    Ok(())
}
