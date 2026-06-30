//! Integration test for `Git2Client` against real local repositories: clone,
//! fetch/sync, branch, commit, and push (exercises `src/git.rs`).

use pmp_iq::git::{CloneRequest, CommitRequest, Git2Client, GitClient, PushRequest};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

fn unique_dir(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("pi-git-{prefix}-{}", Uuid::new_v4()))
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().to_string()
}

/// Create a bare "origin" repo seeded with one commit on `main`.
fn seed_origin() -> PathBuf {
    let origin = unique_dir("origin");
    git2::Repository::init_bare(&origin).unwrap();

    let seed = unique_dir("seed");
    let repo = git2::Repository::init(&seed).unwrap();
    fs::write(seed.join("README.md"), "hello").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("README.md")).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = git2::Signature::now("seed", "seed@example.com").unwrap();
    // Commit straight onto refs/heads/main so the branch exists regardless of
    // the local `init.defaultBranch`.
    repo.commit(Some("refs/heads/main"), &sig, &sig, "init", &tree, &[]).unwrap();
    repo.set_head("refs/heads/main").unwrap();

    let mut remote = repo.remote("origin", &path_str(&origin)).unwrap();
    remote.push(&["refs/heads/main:refs/heads/main"], None).unwrap();
    origin
}

fn request(origin: &Path, work: &Path) -> CloneRequest {
    CloneRequest {
        clone_url: path_str(origin),
        dest: path_str(work),
        branch: Some("main".into()),
        token: None,
    }
}

#[tokio::test]
async fn clone_branch_commit_push_roundtrip() {
    let origin = seed_origin();
    let work = unique_dir("work");
    let git = Git2Client;
    let req = request(&origin, &work);

    // Fresh clone via sync_branch.
    let info = git.sync_branch(req.clone()).await.unwrap();
    assert!(Path::new(&info.path).join("README.md").exists());

    // sync_branch again over the existing checkout (fetch + hard reset).
    let info2 = git.sync_branch(req.clone()).await.unwrap();
    assert_eq!(info2.commit_sha, info.commit_sha);

    // Create the agent branch, make a change, commit it.
    git.create_branch(path_str(&work), "agent/x".into()).await.unwrap();
    fs::write(work.join("NEW.txt"), "a change").unwrap();
    let committed = git
        .commit_all(CommitRequest {
            checkout: path_str(&work),
            message: "agent change".into(),
            author_name: "agent".into(),
            author_email: "agent@example.com".into(),
        })
        .await
        .unwrap();
    assert!(committed, "a real change is committed");

    // A second commit with nothing to stage reports no commit.
    let again = git
        .commit_all(CommitRequest {
            checkout: path_str(&work),
            message: "noop".into(),
            author_name: "agent".into(),
            author_email: "agent@example.com".into(),
        })
        .await
        .unwrap();
    assert!(!again, "no changes → no commit");

    // Push the agent branch to origin.
    git.push_branch(PushRequest {
        checkout: path_str(&work),
        branch: "agent/x".into(),
        token: None,
    })
    .await
    .unwrap();

    // Origin now has the pushed branch.
    let origin_repo = git2::Repository::open_bare(&origin).unwrap();
    assert!(origin_repo.find_reference("refs/heads/agent/x").is_ok());
}

#[tokio::test]
async fn clone_or_update_handles_fresh_and_existing() {
    let origin = seed_origin();
    let work = unique_dir("work2");
    let git = Git2Client;
    let req = request(&origin, &work);

    let first = git.clone_or_update(req.clone()).await.unwrap();
    // Second call opens the existing checkout and fetches.
    let second = git.clone_or_update(req.clone()).await.unwrap();
    assert_eq!(first.commit_sha, second.commit_sha);
}

#[tokio::test]
async fn sync_unknown_branch_errors() {
    let origin = seed_origin();
    let work = unique_dir("work3");
    let git = Git2Client;
    // Clone main first so the checkout exists, then ask for a missing branch.
    git.sync_branch(request(&origin, &work)).await.unwrap();
    let bad = CloneRequest {
        clone_url: path_str(&origin),
        dest: path_str(&work),
        branch: Some("does-not-exist".into()),
        token: None,
    };
    assert!(git.sync_branch(bad).await.is_err());
}
