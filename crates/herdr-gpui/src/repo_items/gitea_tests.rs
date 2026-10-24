use super::{Origin, items};
use crate::{pull_request::Remote, repo_items::Kind};
use herdr_gitea::{Host, HostConfig, Issue, PullRequest};
use serde_json::json;

fn repo() -> Option<herdr_gitea::Repo> {
    let host = Host::from_config(&HostConfig {
        host: "gitea.example.com".into(),
        ..HostConfig::default()
    })?;
    herdr_gitea::repo_in(&[host], "git@gitea.example.com:o/r.git")
}

#[test]
fn gitea_rows_read_like_github_ones_with_gitea_addresses() -> anyhow::Result<()> {
    let repo = repo().ok_or_else(|| anyhow::anyhow!("no repository"))?;
    let pulls: Vec<PullRequest> = serde_json::from_value(json!([
        {"number": 3, "title": "Own branch", "head": {"ref": "feature",
            "repo": {"name": "r", "owner": {"login": "o"}}}, "user": {"login": "me"}},
        {"number": 4, "title": "From a fork", "draft": true, "head": {"ref": "main",
            "repo": {"name": "r", "owner": {"login": "someone"}}}, "user": {"login": "someone"}},
        {"number": 5, "title": "No usable head", "head": {"ref": "--bad"}}
    ]))?;
    let issues: Vec<Issue> = serde_json::from_value(json!([
        {"number": 9, "title": "Bug\nreport", "user": {"login": "reporter"}},
        {"number": 10, "title": "A pull request", "pull_request": {"merged": false}}
    ]))?;
    let origin = Origin {
        owner: "o".into(),
        repo: "r".into(),
        remote: Remote::Origin,
        fork_owner: None,
    };
    let rows = items(&repo, &origin, &pulls, &issues)?;
    let summary: Vec<(Kind, u64, String, Option<String>, bool)> = rows
        .iter()
        .map(|item| {
            (
                item.kind,
                item.number,
                item.url.clone(),
                item.fork_owner.clone(),
                item.draft,
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (
                Kind::PullRequest,
                3,
                "https://gitea.example.com/o/r/pulls/3".to_owned(),
                None,
                false
            ),
            (
                Kind::PullRequest,
                4,
                "https://gitea.example.com/o/r/pulls/4".to_owned(),
                Some("someone".to_owned()),
                true
            ),
            (
                Kind::Issue,
                9,
                "https://gitea.example.com/o/r/issues/9".to_owned(),
                None,
                false
            ),
        ]
    );
    assert_eq!(rows[2].title, "Bug report");
    // A fork's head is fetched through Gitea's pull request ref.
    assert_eq!(
        rows[1].fetch_refspec(),
        "+refs/pull/4/head:refs/herdr/pull/4/head"
    );
    Ok(())
}
