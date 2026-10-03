use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::worktree;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    pub root: PathBuf,
    /// This worktree's own git dir, for state that belongs to one checkout.
    pub git_dir: PathBuf,
    pub name: String,
    /// Folder name of a linked worktree; `None` in the main checkout.
    pub worktree: Option<String>,
    /// The path the agent declared it works in, when discovery followed it here.
    pub declared: Option<PathBuf>,
    pub head: Head,
    pub changed_files: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Head {
    Branch(String),
    Detached(String),
}

impl Repo {
    /// The repository `cwd` belongs to — or the worktree this session declared
    /// it works in, which redirects the whole repository context there.
    /// `Ok(None)` outside a repository; any other git failure is an error.
    pub fn discover(cwd: &Path, session_id: Option<&str>) -> Result<Option<Self>> {
        let Some(layout) = Layout::read(cwd)? else {
            return Ok(None);
        };
        // A declaration whose target is gone is stale state, not an error:
        // `git worktree remove` does not clean our marker up.
        if let Some(declared) =
            worktree::declared(&layout.git_dir, session_id)?.filter(|path| *path != layout.root && path.is_dir())
            && let Some(repo) = Self::at(&declared)?
        {
            return Ok(Some(Self { declared: Some(declared), ..repo }));
        }
        Self::assemble(layout)
    }

    /// The repository `path` belongs to, without the declared-worktree redirect.
    /// `worktree set` writes through this — the marker belongs to the checkout
    /// the agent runs in, not the one it points at.
    pub fn at(path: &Path) -> Result<Option<Self>> {
        let Some(layout) = Layout::read(path)? else {
            return Ok(None);
        };
        Self::assemble(layout)
    }

    fn assemble(layout: Layout) -> Result<Option<Self>> {
        let status = git(&layout.root, &["status", "--porcelain=v2", "--branch"])?;
        let (head, changed_files) = parse_status(&status)?;
        let worktree = (layout.git_dir != layout.common_dir).then(|| folder_name(&layout.root));
        Ok(Some(Self {
            name: repository_name(&layout.common_dir),
            root: layout.root,
            git_dir: layout.git_dir,
            worktree,
            declared: None,
            head,
            changed_files,
        }))
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

    /// `git` run in `cwd`, panicking on failure — the fixture owns these repos.
    fn run_git(cwd: &Path, args: &[&str]) {
        let output = Command::new("git").arg("-C").arg(cwd).args(args).output().unwrap();
        assert!(output.status.success(), "git {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr));
    }

    #[test]
    fn a_declared_worktree_redirects_only_the_session_that_set_it() {
        let temp = tempfile::tempdir().unwrap();
        let main = temp.path().join("main");
        run_git(temp.path(), &["init", "-q", "main"]);
        run_git(&main, &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-q", "--allow-empty", "-m", "init"]);
        let linked = temp.path().join("linked");
        run_git(&main, &["worktree", "add", "-q", "-b", "linked", linked.to_str().unwrap()]);
        let main = std::fs::canonicalize(&main).unwrap();
        let linked = std::fs::canonicalize(&linked).unwrap();

        let local = Repo::discover(&main, Some("first")).unwrap().unwrap();
        assert_eq!(local.worktree, None);
        assert_eq!(local.declared, None);

        worktree::set(&local.git_dir, Some("first"), &linked).unwrap();
        let followed = Repo::discover(&main, Some("first")).unwrap().unwrap();
        assert_eq!(followed.worktree.as_deref(), Some("linked"));
        assert_eq!(followed.declared.as_deref(), Some(linked.as_path()));

        // Other sessions — and sessions without an id — keep this checkout's own repo.
        assert_eq!(Repo::discover(&main, Some("second")).unwrap().unwrap().declared, None);
        assert_eq!(Repo::discover(&main, None).unwrap().unwrap().declared, None);

        worktree::clear(&local.git_dir, Some("first")).unwrap();
        assert_eq!(Repo::discover(&main, Some("first")).unwrap().unwrap().declared, None);

        // Removing the target leaves a stale marker; the checkout's own repo wins.
        worktree::set(&local.git_dir, Some("first"), &linked).unwrap();
        std::fs::remove_dir_all(&linked).unwrap();
        let fallen_back = Repo::discover(&main, Some("first")).unwrap().unwrap();
        assert_eq!(fallen_back.worktree, None);
        assert_eq!(fallen_back.declared, None);
    }
}
