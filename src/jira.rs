use anyhow::Result;
use colored::Colorize;
use serde::Deserialize;
use std::fmt;

use crate::config::Config;

#[derive(Debug, Deserialize, Clone)]
pub struct JiraUser {
    #[serde(rename = "displayName")]
    pub display_name: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct JiraStatus {
    pub name: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct JiraFields {
    pub summary: String,
    pub status: JiraStatus,
    pub assignee: Option<JiraUser>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct JiraIssue {
    pub key: String,
    pub fields: JiraFields,
}

impl fmt::Display for JiraIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let key = format!("[{:<12}]", self.key).blue().bold().to_string();
        let status = format!("{:<17}", self.fields.status.name)
            .yellow()
            .to_string();
        let summary = &self.fields.summary;
        let truncated = if summary.len() > 47 {
            format!("{}...", &summary[..47])
        } else {
            format!("{:<50}", summary)
        };
        let assignee = self
            .fields
            .assignee
            .as_ref()
            .map(|a| format!(" ({})", a.display_name).cyan().to_string())
            .unwrap_or_default();
        write!(f, "{} {} {}{}", key, status, truncated.dimmed(), assignee)
    }
}

#[derive(Debug, Deserialize)]
struct QuickFilter {
    jql: String,
}

#[derive(Debug, Deserialize)]
struct IssueListResponse {
    issues: Vec<JiraIssue>,
}

async fn jira_get<T: for<'de> Deserialize<'de>>(
    config: &Config,
    path: &str,
    params: &[(&str, &str)],
) -> Result<T> {
    let token = std::env::var(&config.jira.token.env).map_err(|_| {
        anyhow::anyhow!(
            "Jira token not set.\n               Expected env var '{}' to contain a base64-encoded 'email:token' string.\n               Generate it with: echo -n \"you@company.com:your_api_token\" | base64\n               Then export it in your shell profile, e.g.: export {}=<result>",
            config.jira.token.env,
            config.jira.token.env
        )
    })?;

    let client = reqwest::Client::new();
    let url = format!("{}{}", config.jira.base_url, path);
    let response = client
        .get(&url)
        .header("Authorization", format!("Basic {}", token))
        .header("Accept", "application/json")
        .query(params)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let hint = match status.as_u16() {
            401 => {
                "\n  → Your Jira token may be invalid or expired. Regenerate it and update the env var."
            }
            403 => "\n  → Your Jira account may lack permission to access this board or resource.",
            404 => {
                "\n  → Board ID or quick-filter ID not found. Check 'boardId' and 'quickFilterId' in your config."
            }
            _ => "",
        };
        anyhow::bail!(
            "Jira API error {} for {}{}{}",
            status,
            url,
            hint,
            if body.is_empty() {
                String::new()
            } else {
                format!("\n  Response: {}", body)
            }
        );
    }

    Ok(response.json().await?)
}

pub async fn fetch_issues(config: &Config) -> Result<Vec<JiraIssue>> {
    let qf: QuickFilter = jira_get(
        config,
        &format!(
            "/rest/agile/1.0/board/{}/quickfilter/{}",
            config.jira.board_id, config.jira.quick_filter_id
        ),
        &[],
    )
    .await?;

    let mut jql = qf.jql;
    if let Some(ext) = &config.jira.jql_extension {
        jql.push(' ');
        jql.push_str(ext);
    }

    let jql_ref = jql.as_str();
    let response: IssueListResponse = jira_get(
        config,
        &format!("/rest/agile/latest/board/{}/issue", config.jira.board_id),
        &[
            ("jql", jql_ref),
            ("fields", "summary,status,assignee"),
            ("maxResults", "200"),
        ],
    )
    .await?;

    let mut issues = response.issues;
    issues.sort_by(|a, b| {
        let assigned_b = b.fields.assignee.is_some() as i32;
        let assigned_a = a.fields.assignee.is_some() as i32;
        assigned_b
            .cmp(&assigned_a)
            .then(a.fields.status.name.cmp(&b.fields.status.name))
            .then(a.key.cmp(&b.key))
    });

    Ok(issues)
}
