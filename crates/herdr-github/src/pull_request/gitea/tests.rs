#![allow(clippy::unwrap_used)]

use super::{
    super::{
        Input, Origin, ReviewDecision, State,
        batch::Batch,
        fetch::{TIMEOUT, run},
        model::MergeState,
    },
    build, claim_in, resolve,
};
use crate::Error;
use herdr_gitea::{
    CombinedStatus, Host, HostConfig, PullRequest as GiteaPull, Repository, Review, Status, User,
    testing::{Server, ok},
};
use serde_json::json;
use std::{process::Command, time::Instant};

fn host(name: &str, base: Option<&str>) -> Host {
    Host::from_config(&HostConfig {
        host: name.into(),
        url: base.map(Into::into),
        ..HostConfig::default()
    })
    .unwrap()
}

fn config<'a>(
    values: &'a [(&'a str, &'a str)],
) -> impl FnMut(&str) -> crate::Result<Option<String>> {
    move |key| {
        Ok(values
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| (*value).to_owned()))
    }
}

#[test]
fn origin_upstream_and_merge_refs_locate_a_checkout() {
    let hosts = [host("gitea.example.com", None)];
    let located = resolve(
        &hosts,
        "feature",
        config(&[(
            "remote.origin.url",
            "ssh://gitea@gitea.example.com:2222/instant-fill/monopoly.git",
        )]),
    )
    .unwrap()
    .unwrap();
    assert_eq!(located.repo.full_name(), "instant-fill/monopoly");
    assert_eq!(located.head_owner, "instant-fill");
    assert_eq!(located.branch, "feature");

    // A fork checkout: the base is upstream, the head is pushed to origin,
    // and the branch's remote name differs from its local one.
    let located = resolve(
        &hosts,
        "local-name",
        config(&[
            ("remote.origin.url", "git@gitea.example.com:me/monopoly.git"),
            (
                "remote.upstream.url",
                "https://gitea.example.com/instant-fill/monopoly",
            ),
            ("branch.local-name.merge", "refs/heads/remote-name"),
        ]),
    )
    .unwrap()
    .unwrap();
    assert_eq!(located.repo.full_name(), "instant-fill/monopoly");
    assert_eq!(located.head_owner, "me");
    assert_eq!(located.branch, "remote-name");

    // GitHub checkouts and unconfigured hosts stay the GitHub path's.
    for remote in [
        "git@github.com:owner/repo.git",
        "https://gitea.other.example/o/r",
    ] {
        let located = resolve(&hosts, "feature", config(&[("remote.origin.url", remote)]));
        assert_eq!(located.unwrap(), None, "{remote}");
    }
}

fn pull(state: &str, merged: bool) -> GiteaPull {
    serde_json::from_value(json!({
        "number": 7,
        "title": "Add the thing",
        "state": state,
        "merged": merged,
        "mergeable": true,
        "html_url": "https://gitea.example.com/o/r/pulls/7",
        "head": {"ref": "feature", "sha": "9f9dc2569221506c0593c978232dca284397c759",
                 "repo": {"name": "r", "full_name": "o/r", "owner": {"login": "o"}}},
        "base": {"ref": "main", "sha": "", "repo": null},
        "updated_at": "2026-10-24T01:22:58-04:00",
        "additions": 130, "deletions": 3, "changed_files": 4,
        "requested_reviewers": [{"login": "reviewer"}]
    }))
    .unwrap()
}

fn repo() -> herdr_gitea::Repo {
    herdr_gitea::repo_in(
        &[host("gitea.example.com", None)],
        "git@gitea.example.com:o/r.git",
    )
    .unwrap()
}

fn review(user: &str, state: &str) -> Review {
    Review {
        state: state.into(),
        user: Some(User { login: user.into() }),
        ..Review::default()
    }
}

#[test]
fn reviews_add_up_to_a_decision() {
    let pr = pull("open", false);
    let decide = |reviews: &[Review]| build::review_decision(&pr, reviews);
    assert_eq!(decide(&[]), Some("REVIEW_REQUIRED"));
    assert_eq!(decide(&[review("a", "APPROVED")]), Some("APPROVED"));
    assert_eq!(
        decide(&[review("a", "APPROVED"), review("b", "REQUEST_CHANGES")]),
        Some("CHANGES_REQUESTED")
    );
    // A reviewer's later approval replaces their requested changes, and a
    // comment does not replace a verdict.
    assert_eq!(
        decide(&[
            review("a", "REQUEST_CHANGES"),
            review("a", "APPROVED"),
            review("a", "COMMENT"),
        ]),
        Some("APPROVED")
    );
    let dismissed = Review {
        dismissed: true,
        ..review("a", "REQUEST_CHANGES")
    };
    assert_eq!(decide(&[dismissed]), Some("REVIEW_REQUIRED"));
    let unrequested = GiteaPull {
        requested_reviewers: Some(Vec::new()),
        ..pr.clone()
    };
    assert_eq!(build::review_decision(&unrequested, &[]), None);
}

#[test]
fn a_gitea_pull_request_reads_like_a_github_one() {
    let settings = Repository {
        allow_merge_commits: true,
        allow_squash_merge: true,
        ..Repository::default()
    };
    let status = CombinedStatus {
        state: "failure".into(),
        total_count: 3,
        statuses: ["success", "failure", "pending"]
            .into_iter()
            .enumerate()
            .map(|(index, status)| Status {
                context: format!("ci / job {index}"),
                status: status.into(),
                target_url: String::new(),
            })
            .collect(),
    };
    let pr = build::pull_request(
        &repo(),
        &pull("open", false),
        &settings,
        &status,
        &[review("a", "APPROVED")],
    )
    .unwrap();
    assert_eq!(pr.number, 7);
    assert_eq!(pr.url, "https://gitea.example.com/o/r/pulls/7");
    assert_eq!(pr.state, State::Open);
    assert_eq!(pr.merge_state_status, MergeState::Clean);
    assert_eq!(pr.review_decision, ReviewDecision::Approved);
    assert_eq!(pr.checks_summary, "1 passed / 1 failed / 1 pending");
    assert_eq!((pr.additions, pr.deletions, pr.changed_files), (130, 3, 4));
    assert_eq!(pr.id, "gitea:7");
    assert_eq!(pr.head_ref_oid, "9f9dc2569221506c0593c978232dca284397c759");
    assert_eq!(
        pr.merge_methods,
        [
            super::super::MergeMethod::Merge,
            super::super::MergeMethod::Squash
        ]
    );
    // No statuses reported is no checks, though Gitea calls it pending.
    let quiet = CombinedStatus {
        state: "pending".into(),
        ..CombinedStatus::default()
    };
    let pr = build::pull_request(&repo(), &pull("closed", true), &settings, &quiet, &[]).unwrap();
    assert_eq!(pr.state, State::Merged);
    assert_eq!(pr.checks_summary, "No checks reported");
    let closed =
        build::pull_request(&repo(), &pull("closed", false), &settings, &quiet, &[]).unwrap();
    assert_eq!(closed.state, State::Closed);
}

#[test]
fn an_answer_for_another_repository_is_refused() {
    let mut other = pull("open", false);
    other.html_url = "https://gitea.example.com/evil/r/pulls/7".into();
    let result = build::pull_request(
        &repo(),
        &other,
        &Repository::default(),
        &CombinedStatus::default(),
        &[],
    );
    assert!(matches!(result, Err(Error::PrIdentity)));
    let mut unknown = pull("open", false);
    unknown.state = "reopened-somehow".into();
    let result = build::pull_request(
        &repo(),
        &unknown,
        &Repository::default(),
        &CombinedStatus::default(),
        &[],
    );
    assert!(matches!(result, Err(Error::PrIdentity)));
}

#[test]
fn drafts_and_conflicts_set_the_merge_state() {
    let mut pr = pull("open", false);
    assert_eq!(build::merge_state(&pr), "CLEAN");
    pr.mergeable = false;
    assert_eq!(build::merge_state(&pr), "DIRTY");
    pr.title = "WIP: not yet".into();
    assert_eq!(build::merge_state(&pr), "DRAFT");
}

/// A Git checkout of `branch` whose origin is `remote`, removed on drop.
struct Checkout(std::path::PathBuf);

impl Drop for Checkout {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Checkout {
    fn new(remote: &str, branch: &str) -> Self {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "herdr-gitea-{}-{suffix}-{branch}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        let checkout = Self(path);
        checkout.git(&["init", "--quiet", "--template=", "-b", branch]);
        checkout.git(&["config", "--local", "remote.origin.url", remote]);
        checkout
    }

    fn git(&self, args: &[&str]) {
        let mut command = Command::new("git");
        command.arg("-C").arg(&self.0).args(args);
        let (ok, text) = run(&mut command, Instant::now() + TIMEOUT, &|| false).unwrap();
        assert!(ok, "fixture git failed: {text}");
    }

    fn input(&self, branch: &str) -> Input {
        Input {
            checkout: Some(self.0.to_str().unwrap().into()),
            repo_key: Some(self.0.join(".git").to_str().unwrap().into()),
            branch: branch.into(),
        }
    }
}

#[test]
fn a_batch_answers_gitea_checkouts_and_passes_the_rest_on() {
    let sha = "9f9dc2569221506c0593c978232dca284397c759";
    let detail = |number: u64, branch: &str, base: &str| {
        json!({
            "number": number, "title": format!("PR {number}"), "state": "open",
            "merged": false, "mergeable": true,
            "html_url": format!("{base}/o/r/pulls/{number}"),
            "head": {"ref": branch, "sha": sha,
                     "repo": {"name": "r", "full_name": "o/r", "owner": {"login": "o"}}},
            "base": {"ref": "main"}, "updated_at": "2026-10-24T00:00:00Z",
            "additions": 1, "deletions": 2, "changed_files": 3
        })
    };
    let server = Server::start_with(|base| {
        vec![
            ok(
                "GET",
                "/api/v1/repos/o/r",
                json!({"default_branch": "main", "allow_merge_commits": true}),
            ),
            ok(
                "GET",
                "/api/v1/repos/o/r/pulls?state=open&limit=50&page=1",
                json!([detail(7, "feature", base), detail(8, "other", base)]),
            ),
            ok(
                "GET",
                "/api/v1/repos/o/r/pulls?state=open&limit=50&page=2",
                json!([]),
            ),
            ok(
                "GET",
                "/api/v1/repos/o/r/pulls/7",
                detail(7, "feature", base),
            ),
            ok(
                "GET",
                &format!("/api/v1/repos/o/r/commits/{sha}/status"),
                json!({"state": "success", "total_count": 1,
                       "statuses": [{"context": "ci", "status": "success"}]}),
            ),
            ok(
                "GET",
                "/api/v1/repos/o/r/pulls/7/reviews?limit=50&page=1",
                json!([{"state": "APPROVED", "user": {"login": "a"}}]),
            ),
            ok(
                "GET",
                "/api/v1/repos/o/r/pulls/7/reviews?limit=50&page=2",
                json!([]),
            ),
        ]
    })
    .unwrap();
    let base = server.base.clone();
    let host = host("127.0.0.1", Some(&base));

    let gitea = Checkout::new(&format!("{base}/o/r.git"), "feature");
    let unopened = Checkout::new(&format!("{base}/o/r.git"), "none");
    let github = Checkout::new("https://github.com/example/project.git", "feature");
    let mut batch = Batch::default();
    let rest = claim_in(
        std::slice::from_ref(&host),
        vec![
            gitea.input("feature"),
            unopened.input("none"),
            github.input("feature"),
        ],
        &Origin::Local,
        &mut batch,
        &|| false,
    );
    assert_eq!(rest, vec![github.input("feature")]);
    assert!(batch.failed.is_empty(), "{:?}", batch.failed);
    let found = |input: &Input| {
        batch
            .found
            .iter()
            .find(|(asked, _)| asked == input)
            .map(|(_, result)| result.as_ref().ok().cloned())
    };
    let pr = found(&gitea.input("feature")).unwrap().unwrap().unwrap();
    assert_eq!(pr.number, 7);
    assert_eq!(pr.url, format!("{base}/o/r/pulls/7"));
    assert_eq!(pr.review_decision, ReviewDecision::Approved);
    assert_eq!(pr.checks_summary, "1 passed");
    assert!(found(&unopened.input("none")).unwrap().unwrap().is_none());
    // Both checkouts share one listing of the repository's open pull requests.
    let listings = server
        .requests()
        .iter()
        .filter(|(_, path, ..)| path == "/api/v1/repos/o/r/pulls?state=open&limit=50&page=1")
        .count();
    assert_eq!(listings, 1);
}
