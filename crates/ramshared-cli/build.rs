use std::collections::BTreeSet;
use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    let Some(manifest_dir) = env::var_os("CARGO_MANIFEST_DIR") else {
        println!("cargo:rustc-env=RAMSHARED_BUILD_GIT_SHA=");
        println!("cargo:rustc-env=RAMSHARED_BUILD_TREE_STATE=unavailable");
        return;
    };
    let manifest_dir = Path::new(&manifest_dir);
    let workspace_root = manifest_dir.join("../..");
    for path in [
        workspace_root.join("Cargo.toml"),
        workspace_root.join("Cargo.lock"),
    ] {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    watch_worktree_paths(&workspace_root);
    watch_git_metadata(&workspace_root);

    let (commit, tree_state) = read_source_identity(manifest_dir);
    println!(
        "cargo:rustc-env=RAMSHARED_BUILD_GIT_SHA={}",
        commit.unwrap_or_default()
    );
    println!("cargo:rustc-env=RAMSHARED_BUILD_TREE_STATE={tree_state}");
}

fn watch_worktree_paths(workspace_root: &Path) {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])
        .output();
    let Ok(output) = output else {
        return;
    };
    if !output.status.success() {
        return;
    }

    let mut directories = BTreeSet::new();
    for entry in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
    {
        let Ok(relative) = std::str::from_utf8(entry) else {
            continue;
        };
        let path = workspace_root.join(relative);
        println!("cargo:rerun-if-changed={}", path.display());
        let mut parent = path.parent();
        while let Some(directory) = parent {
            if !directory.starts_with(workspace_root) {
                break;
            }
            directories.insert(directory.to_path_buf());
            parent = directory.parent();
        }
    }
    for directory in directories {
        println!("cargo:rerun-if-changed={}", directory.display());
    }
}

fn watch_git_metadata(workspace_root: &Path) {
    for path in ["HEAD", "index", "packed-refs"] {
        watch_git_path(workspace_root, path);
    }
    if let Some(reference) = git_output_with_args(workspace_root, &["symbolic-ref", "-q", "HEAD"]) {
        watch_git_path(workspace_root, &reference);
    }
}

fn watch_git_path(workspace_root: &Path, path: &str) {
    let Some(relative) = git_output_with_args(workspace_root, &["rev-parse", "--git-path", path])
    else {
        return;
    };
    let path = Path::new(&relative);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(path)
    };
    println!("cargo:rerun-if-changed={}", path.display());
}

fn read_source_identity(manifest_dir: &Path) -> (Option<String>, &'static str) {
    let commit = git_output_with_args(manifest_dir, &["rev-parse", "--verify", "HEAD^{commit}"])
        .filter(|value| is_full_commit(value));
    let Some(commit) = commit else {
        return (None, "unavailable");
    };

    let Some(status) = git_output_with_args(
        manifest_dir,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    ) else {
        return (Some(commit), "unavailable");
    };
    let tree_state = if status.is_empty() { "clean" } else { "dirty" };
    (Some(commit), tree_state)
}

fn git_output_with_args(manifest_dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(manifest_dir)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|value| value.trim().to_string())
}

fn is_full_commit(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
