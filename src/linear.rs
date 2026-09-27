//! Linear issues for the status line. `render` only ever reads the cache; the
//! network lives in `refresh`, which runs out of band (under `op run`, cron,
//! launchd) because status lines are re-run far too often to block on HTTP.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::paths;

const ENDPOINT: &str = "https://api.linear.app/graphql";
const TIMEOUT: Duration = Duration::from_secs(20);
const SEEN_RETENTION_SECONDS: u64 = 7 * 24 * 60 * 60;
/// Re-recording a key on every render would write the file constantly.
const SEEN_RESOLUTION_SECONDS: u64 = 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub identifier: String,
    pub title: String,
    pub state: String,
    pub url: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Cache {
    pub fetched_at: u64,
    pub team_keys: Vec<String>,
    /// Issues assigned to the viewer in a started state.
    pub started: Vec<Issue>,
    /// Issues seen on branches, keyed by identifier.
    pub branch_issues: BTreeMap<String, Issue>,
}

impl Cache {
    pub fn load() -> Result<Option<Self>> {
        read_json(&cache_file()?)
    }

    pub fn issue(&self, identifier: &str) -> Option<&Issue> {
        self.branch_issues.get(identifier).or_else(|| self.started.iter().find(|issue| issue.identifier == identifier))
    }

    /// The Linear key a branch refers to, e.g. `mehdi/eng-123-fix-auth` → `ENG-123`.
    /// Only teams the viewer belongs to count, so `feat/utf-8` is not an issue.
    pub fn issue_key(&self, branch: &str) -> Option<String> {
        static KEY: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"(?i)\b([a-z][a-z0-9]*)-([0-9]+)\b").expect("valid regex"));
        KEY.captures_iter(branch).find_map(|captures| {
            let team = captures[1].to_uppercase();
            self.team_keys.contains(&team).then(|| format!("{team}-{}", &captures[2]))
        })
    }
}

/// Marks `identifier` as on screen so the next `refresh` fetches it.
pub fn record_seen(identifier: &str) -> Result<()> {
    let path = seen_file()?;
    let mut seen: BTreeMap<String, u64> = read_json(&path)?.unwrap_or_default();
    let now = now();
    if seen.get(identifier).is_some_and(|at| now.saturating_sub(*at) < SEEN_RESOLUTION_SECONDS) {
        return Ok(());
    }
    seen.insert(identifier.to_string(), now);
    write_json_atomically(&path, &seen)
}

pub fn refresh(api_key: &str) -> Result<Cache> {
    let seen_path = seen_file()?;
    let now = now();
    let mut seen: BTreeMap<String, u64> = read_json(&seen_path)?.unwrap_or_default();
    seen.retain(|_, at| now.saturating_sub(*at) < SEEN_RETENTION_SECONDS);
    let wanted: Vec<&String> = seen.keys().collect();

    let data = query(api_key, &wanted)?;
    let started = issues_at(&data, "/viewer/assignedIssues/nodes")?;
    let team_keys = data
        .pointer("/teams/nodes")
        .and_then(Value::as_array)
        .context("Linear response has no teams")?
        .iter()
        .filter_map(|team| team["key"].as_str().map(str::to_string))
        .collect();
    let mut branch_issues = BTreeMap::new();
    for index in 0..wanted.len() {
        // A null alias is an issue that was deleted or is not visible to the key.
        if let Some(issue) = data.get(format!("w{index}")).filter(|value| !value.is_null()) {
            let issue = parse_issue(issue)?;
            branch_issues.insert(issue.identifier.clone(), issue);
        }
    }

    let cache = Cache { fetched_at: now, team_keys, started, branch_issues };
    write_json_atomically(&cache_file()?, &cache)?;
    write_json_atomically(&seen_path, &seen)?;
    Ok(cache)
}

fn query(api_key: &str, wanted: &[&String]) -> Result<Value> {
    let fields = "identifier title url state { name }";
    let declarations: Vec<String> = (0..wanted.len()).map(|index| format!("$w{index}: String!")).collect();
    let aliases: String =
        (0..wanted.len()).map(|index| format!(" w{index}: issue(id: $w{index}) {{ {fields} }}")).collect();
    let signature = if declarations.is_empty() { String::new() } else { format!("({})", declarations.join(", ")) };
    let document = format!(
        "query Status{signature} {{ viewer {{ assignedIssues(first: 50, filter: {{ state: {{ type: {{ eq: \"started\" }} }} }}) {{ nodes {{ {fields} }} }} }} teams(first: 250) {{ nodes {{ key }} }}{aliases} }}"
    );
    let variables: serde_json::Map<String, Value> =
        wanted.iter().enumerate().map(|(index, key)| (format!("w{index}"), json!(key))).collect();

    let agent: ureq::Agent =
        ureq::Agent::config_builder().http_status_as_error(false).timeout_global(Some(TIMEOUT)).build().into();
    let mut response = agent
        .post(ENDPOINT)
        .header("Authorization", api_key)
        .send_json(json!({ "query": document, "variables": variables }))
        .context("Cannot reach the Linear API")?;
    let status = response.status();
    let body: Value =
        response.body_mut().read_json().with_context(|| format!("Linear answered HTTP <{status}> without JSON"))?;

    let errors = body["errors"].as_array().map(Vec::as_slice).unwrap_or_default();
    let fatal: Vec<&str> = errors
        .iter()
        .filter(|error| !is_alias_error(error, wanted.len()))
        .map(|error| error["message"].as_str().unwrap_or("unknown error"))
        .collect();
    if !fatal.is_empty() || !status.is_success() {
        bail!("Linear rejected the query (HTTP <{status}>): {}", fatal.join("; "));
    }
    Ok(body["data"].clone())
}

/// A per-issue lookup failure (issue gone, no access) must not fail the refresh.
fn is_alias_error(error: &Value, alias_count: usize) -> bool {
    error.pointer("/path/0").and_then(Value::as_str).is_some_and(|alias| {
        alias.strip_prefix('w').and_then(|index| index.parse::<usize>().ok()).is_some_and(|index| index < alias_count)
    })
}

fn issues_at(data: &Value, pointer: &str) -> Result<Vec<Issue>> {
    data.pointer(pointer)
        .and_then(Value::as_array)
        .with_context(|| format!("Linear response has nothing at <{pointer}>"))?
        .iter()
        .map(parse_issue)
        .collect()
}

fn parse_issue(value: &Value) -> Result<Issue> {
    let field = |pointer: &str| {
        value
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::to_string)
            .with_context(|| format!("Linear issue is missing <{pointer}>"))
    };
    Ok(Issue {
        identifier: field("/identifier")?,
        title: field("/title")?,
        state: field("/state/name")?,
        url: field("/url")?,
    })
}

fn cache_file() -> Result<PathBuf> {
    Ok(paths::cache_dir()?.join("linear.json"))
}

fn seen_file() -> Result<PathBuf> {
    Ok(paths::cache_dir()?.join("linear-seen.json"))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Option<T>> {
    match fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str(&contents)
            .map(Some)
            .with_context(|| format!("<{}> is corrupt; delete it and refresh", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("Cannot read <{}>", path.display())),
    }
}

fn write_json_atomically<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    paths::write_atomically(path, &serde_json::to_string_pretty(value)?)
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_secs()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_issue_keys_only_for_known_teams() {
        let cache = Cache { team_keys: vec!["ENG".into(), "OPS2".into()], ..Cache::default() };
        let cases = [
            ("mehdi/eng-123-fix-auth", Some("ENG-123")),
            ("ENG-7", Some("ENG-7")),
            ("feat/ops2-9-deploy", Some("OPS2-9")),
            ("fix/utf-8-decoding", None),
            ("main", None),
        ];
        for (branch, expected) in cases {
            assert_eq!(cache.issue_key(branch).as_deref(), expected, "branch <{branch}>");
        }
    }
}
