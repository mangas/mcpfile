use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

const SKILL_MD: &str = include_str!("../skill.md");

fn skill_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME not set")?;
    Ok(PathBuf::from(home).join(".claude/skills/mcpfile"))
}

pub fn install() -> Result<()> {
    let dir = skill_dir()?;
    fs::create_dir_all(&dir)?;
    let path = dir.join("SKILL.md");
    fs::write(&path, SKILL_MD)?;
    println!("Installed skill to {}", path.display());
    Ok(())
}
