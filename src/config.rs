use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    #[allow(dead_code)]
    pub version: u32,
    pub jira: JiraConfig,
    pub common: CommonConfig,
    pub repos: Vec<RepoConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct JiraConfig {
    #[serde(rename = "baseUrl")]
    pub base_url: String,
    #[serde(rename = "boardId")]
    pub board_id: u32,
    #[serde(rename = "quickFilterId")]
    pub quick_filter_id: u32,
    pub token: TokenConfig,
    #[serde(rename = "jqlExtension")]
    pub jql_extension: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TokenConfig {
    pub env: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CommonConfig {
    #[serde(rename = "branchPrefixes")]
    pub branch_prefixes: Vec<String>,
    #[serde(rename = "defaultBranchPrefix")]
    #[allow(dead_code)]
    pub default_branch_prefix: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RepoConfig {
    pub name: String,
    pub root: String,
    #[serde(rename = "type")]
    pub repo_type: RepoType,
    #[serde(rename = "defaultBranch")]
    pub default_branch: Option<String>,
    pub commands: Option<Vec<RepoCommand>>,
    #[serde(rename = "persistentBranches")]
    pub persistent_branches: Option<Vec<String>>,
    #[serde(rename = "featurePath")]
    pub feature_path: Option<String>,
    #[serde(rename = "prToBranches")]
    pub pr_to_branches: Option<Vec<String>>,
    #[serde(rename = "prTemplate")]
    pub pr_template: Option<PrTemplate>,
}

impl fmt::Display for RepoConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RepoType {
    Standard,
    Worktree,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PrTemplate {
    pub ask: Option<HashMap<String, AskField>>,
    pub title: String,
    pub body: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RepoCommand {
    pub command: String,
    pub runner: String,
    pub ask: Option<HashMap<String, AskField>>,
    pub env: Option<HashMap<String, String>>,
    pub run: String,
}

impl fmt::Display for RepoCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.command)
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum AskField {
    Simple(String),
    Complex {
        #[serde(rename = "type")]
        field_type: String,
        optional: Option<bool>,
    },
}

impl AskField {
    pub fn field_type(&self) -> &str {
        match self {
            Self::Simple(t) => t,
            Self::Complex { field_type, .. } => field_type,
        }
    }

    pub fn is_optional(&self) -> bool {
        matches!(self, Self::Complex { optional: Some(true), .. })
    }
}

pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        dirs::home_dir().unwrap_or_default().join(rest)
    } else if path == "~" {
        dirs::home_dir().unwrap_or_default()
    } else {
        PathBuf::from(path)
    }
}

pub fn find_config(override_path: Option<&str>) -> Result<Config> {
    let paths: Vec<PathBuf> = match override_path {
        Some(p) => vec![PathBuf::from(p)],
        None => {
            let home = dirs::home_dir().unwrap_or_default();
            vec![
                PathBuf::from("fi.yaml"),
                home.join(".config/fi/fi.yaml"),
                home.join(".config/fi/fi.yml"),
            ]
        }
    };

    for path in &paths {
        if path.exists() {
            let content = std::fs::read_to_string(path)
                .with_context(|| format!("reading {:?}", path))?;
            return serde_yaml::from_str(&content)
                .with_context(|| format!("parsing {:?}", path));
        }
    }

    anyhow::bail!("No config file found. Tried: {:?}", paths)
}

