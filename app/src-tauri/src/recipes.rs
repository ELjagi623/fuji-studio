//! Built-in and user recipes (JSON files in the app config dir).

use anyhow::{bail, Result};
use fuji_ptp::Recipe;
use serde::Serialize;
use std::path::{Path, PathBuf};

const BUILTIN: &[&str] = &[
    include_str!("../../../recipes/classic-chrome-street.json"),
    include_str!("../../../recipes/classic-neg-warm.json"),
    include_str!("../../../recipes/acros-red.json"),
];

#[derive(Serialize, Clone)]
pub struct RecipeEntry {
    pub recipe: Recipe,
    pub builtin: bool,
}

fn file_name(name: &str) -> Result<String> {
    let clean: String = name
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '_' })
        .collect();
    if clean.trim().is_empty() {
        bail!("配方名称不能为空");
    }
    Ok(format!("{}.json", clean.trim()))
}

pub fn list(dir: &Path) -> Vec<RecipeEntry> {
    let mut out: Vec<RecipeEntry> = BUILTIN
        .iter()
        .filter_map(|s| serde_json::from_str(s).ok())
        .map(|recipe| RecipeEntry { recipe, builtin: true })
        .collect();
    let mut user: Vec<RecipeEntry> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == "json").unwrap_or(false))
        .filter_map(|p| Recipe::load(&p).ok())
        .map(|recipe| RecipeEntry { recipe, builtin: false })
        .collect();
    user.sort_by(|a, b| a.recipe.name.cmp(&b.recipe.name));
    out.extend(user);
    out
}

pub fn save(dir: &Path, r: &Recipe) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let p = dir.join(file_name(&r.name)?);
    std::fs::write(&p, serde_json::to_string_pretty(r)?)?;
    Ok(p)
}

pub fn delete(dir: &Path, name: &str) -> Result<()> {
    let p = dir.join(file_name(name)?);
    if p.exists() {
        std::fs::remove_file(p)?;
    }
    Ok(())
}
