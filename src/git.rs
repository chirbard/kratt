use std::error::Error;
use std::process::Command;

use crate::graph::{Graph, DATA_DIR, save_graph};

fn run_git(args: &[&str]) -> Result<(), Box<dyn Error>> {
    let status = Command::new("git").args(args).status()?;
    if !status.success() {
        return Err(format!("git {args:?} failed with {status}").into());
    }
    Ok(())
}

pub fn git_init_if_needed() -> Result<(), Box<dyn Error>> {
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

pub fn git_commit(label: &str) -> Result<(), Box<dyn Error>> {
    run_git(&["-C", DATA_DIR, "add", "kratt-graph.toon"])?;
    // git diff --cached --quiet exits 0 when nothing is staged, 1 when there are changes
    let staged = Command::new("git")
        .args(["-C", DATA_DIR, "diff", "--cached", "--quiet"])
        .status()?;
    if staged.success() {
        return Ok(());
    }
    run_git(&["-C", DATA_DIR, "commit", "-m", &format!("{label}: update memory")])?;
    Ok(())
}
