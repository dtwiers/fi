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
    pub hooks: Option<Vec<HookConfig>>,
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
    #[serde(rename = "branchFormat")]
    pub branch_format: Option<String>,
}

impl CommonConfig {
    /// Render a branch name from the format template.
    /// Variables: {branchPrefix}, {ticket.key}, {slug}, {conflictBase} (conditional).
    pub fn render_branch(
        &self,
        prefix: &str,
        ticket_key: &str,
        slug: &str,
        conflict_base: Option<&str>,
    ) -> String {
        use crate::template::render_template;
        use std::collections::HashMap;

        let fmt = self
            .branch_format
            .as_deref()
            .unwrap_or("{branchPrefix}/{ticket.key}-{slug}");

        let mut vars = HashMap::new();
        vars.insert("branchPrefix".into(), prefix.to_string());
        vars.insert("ticket.key".into(), ticket_key.to_string());
        vars.insert("slug".into(), slug.to_string());
        vars.insert(
            "conflictBase".into(),
            conflict_base.unwrap_or("").to_string(),
        );

        render_template(fmt, &vars)
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct RepoConfig {
    pub name: String,
    pub root: String,
    #[serde(rename = "type")]
    pub repo_type: RepoType,
    #[serde(rename = "defaultBranch")]
    pub default_branch: Option<String>,
    /// Git remote name. Defaults to `"origin"` when not set.
    pub remote: Option<String>,
    pub commands: Option<Vec<RepoCommand>>,
    #[serde(rename = "persistentBranches")]
    pub persistent_branches: Option<Vec<String>>,
    #[serde(rename = "featurePath")]
    pub feature_path: Option<String>,
    #[serde(rename = "mergeConflictPath")]
    pub merge_conflict_path: Option<String>,
    #[serde(rename = "prToBranches")]
    pub pr_to_branches: Option<Vec<String>>,
    #[serde(rename = "prTemplate")]
    pub pr_template: Option<PrTemplate>,
    pub hooks: Option<Vec<HookConfig>>,
}

impl RepoConfig {
    /// Returns the configured remote name, falling back to `"origin"`.
    pub fn remote(&self) -> &str {
        self.remote.as_deref().unwrap_or("origin")
    }
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

// ── Hooks ─────────────────────────────────────────────────────────────────────

/// Which timing a hook runs at relative to a command.
#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum HookWhen {
    Pre,
    Post,
}

/// The fi subcommand(s) a hook is attached to. Accepts a single string or a list.
#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum HookOn {
    One(String),
    Many(Vec<String>),
}

impl HookOn {
    pub fn matches(&self, command: &str) -> bool {
        match self {
            Self::One(s) => s == command,
            Self::Many(v) => v.iter().any(|s| s == command),
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize, Clone)]
pub struct HookConfig {
    /// Optional display name shown when prompting or logging.
    pub name: Option<String>,
    /// fi subcommand(s) this hook applies to: "new", "open", "pr", "cull", "sync".
    pub trigger: HookOn,
    /// Whether to run before (`pre`) or after (`post`) the command.
    pub when: HookWhen,
    /// If true, the user is asked whether to run the hook.
    #[serde(default)]
    pub optional: bool,
    /// Default answer when the hook is optional. Defaults to `true` (run by default).
    #[serde(rename = "defaultOn", default = "default_true")]
    pub default_on: bool,
    pub runner: String,
    pub ask: Option<HashMap<String, AskField>>,
    pub env: Option<HashMap<String, String>>,
    pub run: String,
}

impl fmt::Display for HookConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.name.as_deref().unwrap_or("hook")
        )
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
        matches!(
            self,
            Self::Complex {
                optional: Some(true),
                ..
            }
        )
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
            let content =
                std::fs::read_to_string(path).with_context(|| format!("reading {:?}", path))?;
            return serde_yaml::from_str(&content).with_context(|| format!("parsing {:?}", path));
        }
    }

    anyhow::bail!(
        "No config file found (tried: {})\n  → Run 'fi init' to create a starter config.",
        paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// Return only the *path* of the first config file that exists, without fully
/// parsing it. Useful for `fi config path` and `fi config edit`.
pub fn find_config_path(override_path: Option<&str>) -> Result<PathBuf> {
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
            // Return the canonicalised absolute path so callers always get a
            // fully-qualified path regardless of where fi was invoked from.
            return path
                .canonicalize()
                .with_context(|| format!("resolving path {:?}", path));
        }
    }

    anyhow::bail!(
        "No config file found (tried: {})\n  → Run 'fi init' to create a starter config.",
        paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn common(branch_format: Option<&str>) -> CommonConfig {
        CommonConfig {
            branch_prefixes: vec!["fix".into(), "feat".into()],
            default_branch_prefix: "feat".into(),
            branch_format: branch_format.map(|s| s.to_string()),
        }
    }

    // ── expand_tilde ──────────────────────────────────────────────────────────

    #[test]
    fn expand_tilde_home_dir() {
        let home = dirs::home_dir().unwrap_or_default();
        assert_eq!(expand_tilde("~"), home);
    }

    #[test]
    fn expand_tilde_with_path() {
        let home = dirs::home_dir().unwrap_or_default();
        assert_eq!(expand_tilde("~/foo/bar"), home.join("foo/bar"));
    }

    #[test]
    fn expand_tilde_absolute_path_unchanged() {
        assert_eq!(
            expand_tilde("/absolute/path"),
            PathBuf::from("/absolute/path")
        );
    }

    #[test]
    fn expand_tilde_relative_path_unchanged() {
        assert_eq!(
            expand_tilde("relative/path"),
            PathBuf::from("relative/path")
        );
    }

    // ── render_branch ─────────────────────────────────────────────────────────

    #[test]
    fn render_branch_default_format() {
        let c = common(None);
        assert_eq!(
            c.render_branch("fix", "CAPY-1234", "some-feature", None),
            "fix/CAPY-1234-some-feature"
        );
    }

    #[test]
    fn render_branch_custom_format() {
        let c = common(Some("{branchPrefix}/{ticket.key}-{slug}"));
        assert_eq!(
            c.render_branch("feat", "PROJ-99", "add-widget", None),
            "feat/PROJ-99-add-widget"
        );
    }

    #[test]
    fn render_branch_with_conflict_base() {
        let fmt = "{branchPrefix}/{ticket.key}{conflictBase: '-$1'}-{slug}";
        let c = common(Some(fmt));
        assert_eq!(
            c.render_branch("fix", "CAPY-1234", "some-feature", Some("DEVELOP")),
            "fix/CAPY-1234-DEVELOP-some-feature"
        );
    }

    #[test]
    fn render_branch_conflict_base_absent_suppresses_segment() {
        let fmt = "{branchPrefix}/{ticket.key}{conflictBase: '-$1'}-{slug}";
        let c = common(Some(fmt));
        assert_eq!(
            c.render_branch("fix", "CAPY-1234", "some-feature", None),
            "fix/CAPY-1234-some-feature"
        );
    }

    // ── AskField ─────────────────────────────────────────────────────────────

    #[test]
    fn ask_field_simple_type() {
        let f = AskField::Simple("editor".into());
        assert_eq!(f.field_type(), "editor");
        assert!(!f.is_optional());
    }

    #[test]
    fn ask_field_complex_type() {
        let f = AskField::Complex {
            field_type: "boolean".into(),
            optional: None,
        };
        assert_eq!(f.field_type(), "boolean");
        assert!(!f.is_optional());
    }

    #[test]
    fn ask_field_complex_optional() {
        let f = AskField::Complex {
            field_type: "editor".into(),
            optional: Some(true),
        };
        assert!(f.is_optional());
    }

    #[test]
    fn ask_field_complex_optional_false() {
        let f = AskField::Complex {
            field_type: "editor".into(),
            optional: Some(false),
        };
        assert!(!f.is_optional());
    }
}
