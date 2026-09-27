use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    pub root: PathBuf,
    pub name: String,
    /// Folder name of a linked worktree; `None` in the main checkout.
    pub worktree: Option<String>,
    pub head: Head,
    pub changed_files: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Head {
    Branch(String),
    Detached(String),
}

impl Head {
    pub fn branch(&self) -> Option<&str> {
        match self {
            Head::Branch(name) => Some(name),
            Head::Detached(_) => None,
        }
    }
}

impl Repo {
    /// `Ok(None)` outside a repository; any other git failure is an error.
    pub fn discover(cwd: &Path) -> Result<Option<Self>> {
        let Some(layout) = Layout::read(cwd)? else {
            return Ok(None);
        };
        let status = git(&layout.root, &["status", "--porcelain=v2", "--branch"])?;
        let (head, changed_files) = parse_status(&status)?;
        let worktree = (layout.git_dir != layout.common_dir).then(|| folder_name(&layout.root));
        Ok(Some(Self { name: repository_name(&layout.common_dir), root: layout.root, worktree, head, changed_files }))
    }
}

struct Layout {
    root: PathBuf,
    git_dir: PathBuf,
    common_dir: PathBuf,
}

impl Layout {
    fn read(cwd: &Path) -> Result<Option<Self>> {
        let output = Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(["rev-parse", "--path-format=absolute", "--show-toplevel", "--git-dir", "--git-common-dir"])
            .output()
            .context("Cannot run git")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("not a git repository") {
                return Ok(None);
            }
            bail!("git rev-parse failed in <{}>: {}", cwd.display(), stderr.trim());
        }
        let stdout = String::from_utf8(output.stdout).context("git printed non-UTF-8 paths")?;
        let mut lines = stdout.lines().map(PathBuf::from);
        match (lines.next(), lines.next(), lines.next()) {
            (Some(root), Some(git_dir), Some(common_dir)) => Ok(Some(Self { root, git_dir, common_dir })),
            _ => bail!("git rev-parse printed <{}>, expected three paths", stdout.trim()),
        }
    }
}

fn git(directory: &Path, args: &[&str]) -> Result<String> {
    // Optional locks would contend with the agent's own git commands.
    let output = Command::new("git")
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(directory)
        .args(args)
        .output()
        .context("Cannot run git")?;
    if !output.status.success() {
        bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim());
    }
    String::from_utf8(output.stdout).context("git printed non-UTF-8 output")
}

fn parse_status(status: &str) -> Result<(Head, usize)> {
    let mut name = None;
    let mut oid = None;
    let mut changed_files = 0;
    for line in status.lines() {
        if let Some(value) = line.strip_prefix("# branch.head ") {
            name = Some(value);
        } else if let Some(value) = line.strip_prefix("# branch.oid ") {
            oid = Some(value);
        } else if !line.starts_with('#') {
            changed_files += 1;
        }
    }
    let head = match (name, oid) {
        (Some("(detached)"), Some(oid)) => Head::Detached(oid.chars().take(7).collect()),
        (Some(name), _) => Head::Branch(name.to_string()),
        _ => bail!("git status printed no branch header"),
    };
    Ok((head, changed_files))
}

/// `/src/app/.git` → `app`; `/src/app.git` (bare) → `app`.
fn repository_name(common_dir: &Path) -> String {
    if common_dir.file_name().is_some_and(|name| name == ".git") {
        return common_dir.parent().map(folder_name).unwrap_or_default();
    }
    folder_name(common_dir).trim_end_matches(".git").to_string()
}

fn folder_name(path: &Path) -> String {
    path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_branch_detached_and_changes() {
        let status =
            "# branch.oid 1234567890\n# branch.head main\n1 .M N... 100644 100644 100644 a b src/x.rs\n? new.txt\n";
        assert_eq!(parse_status(status).unwrap(), (Head::Branch("main".into()), 2));
        let detached = "# branch.oid abcdef1234\n# branch.head (detached)\n";
        assert_eq!(parse_status(detached).unwrap(), (Head::Detached("abcdef1".into()), 0));
    }

    #[test]
    fn names_repositories_from_their_common_dir() {
        assert_eq!(repository_name(Path::new("/src/app/.git")), "app");
        assert_eq!(repository_name(Path::new("/src/app.git")), "app");
    }
}
