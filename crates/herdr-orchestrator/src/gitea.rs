//! A Gitea or Forgejo repository's open issues, read through Gitea's REST
//! API for a remote on a host configured under `[gitea]`: `native_id` is the
//! issue's number. Every sync is a complete listing, so it may replace the
//! cache, as GitLab's does.

use super::{Activity, Checkpoint, Item, ItemKey, SourceKey, gitlab::Listing};
use chrono::{DateTime, Utc};
use herdr_gitea::{Client, Issue, Repo};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(60);
/// The most pages one sync reads: 40 pages of 50, the GitLab listing's cap.
const MAX_PAGES: usize = super::gitlab::MAX_OPEN / herdr_gitea::PAGE;

/// Whether `host` is a configured Gitea instance, which decides a remote's
/// provider before any guess from its name.
pub fn is_host(host: &str) -> bool {
    herdr_gitea::hosts()
        .iter()
        .any(|instance| instance.names_host(host))
}

/// The configured repository a source names.
pub fn repo(source: &SourceKey) -> Option<Repo> {
    herdr_gitea::repo(&format!("https://{}/{}", source.host, source.repository))
}

/// Lists `source`'s open issues. `previous` keeps agent-launcher's markers,
/// which herdr-gpui never adds.
pub fn sync(
    source: &SourceKey,
    previous: Option<&Checkpoint>,
    cancelled: &impl Fn() -> bool,
) -> super::Result<Listing> {
    let repo = repo(source).ok_or_else(|| super::Error::RemoteUrl(source.repository.clone()))?;
    let started = Utc::now();
    let client = Client::new(&repo.host, TIMEOUT).map_err(error)?;
    let (issues, complete): (Vec<Issue>, bool) = client
        .get_pages(
            &repo.api("/issues?type=issues&state=open"),
            MAX_PAGES,
            cancelled,
        )
        .map_err(error)?;
    let previous = previous.cloned().unwrap_or_default();
    Ok(Listing {
        items: items(source, issues),
        checkpoint: Checkpoint {
            updated_at: Some(started),
            etag: None,
            last_full_at: Some(Utc::now()),
            pr_details: previous.pr_details,
            pr_cursor: previous.pr_cursor,
        },
        complete,
    })
}

/// Each issue as the orchestrator's item. Pull requests are listed by Gitea's
/// issues API too unless asked not to; any that slip through are left out.
pub fn items(source: &SourceKey, issues: Vec<Issue>) -> Vec<Item> {
    let mut seen = std::collections::HashSet::new();
    issues
        .into_iter()
        .filter(|issue| issue.pull_request.is_none() && issue.number > 0)
        .filter(|issue| seen.insert(issue.number))
        .map(|issue| Item {
            key: ItemKey {
                source: source.clone(),
                native_id: issue.number.to_string(),
            },
            identifier: format!("#{}", issue.number),
            title: issue.title,
            description: Some(issue.body).filter(|body| !body.is_empty()),
            state: issue.state.to_ascii_lowercase(),
            url: Some(issue.html_url).filter(|url| !url.is_empty()),
            author: issue.user.map(|user| user.login),
            labels: issue.labels.into_iter().map(|label| label.name).collect(),
            parent_id: None,
            blocked_by: Vec::new(),
            priority: None,
            created_at: time(&issue.created_at),
            updated_at: time(&issue.updated_at),
            pull_request: None,
            activity: Some(Activity {
                comments: Some(issue.comments),
                ..Activity::default()
            }),
        })
        .collect()
}

fn time(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|time| time.with_timezone(&Utc))
}

fn error(error: herdr_gitea::Error) -> super::Error {
    match error {
        herdr_gitea::Error::Cancelled => super::Error::Cancelled,
        error => super::Error::Gitea(error),
    }
}

#[cfg(test)]
mod tests {
    use super::{super::Provider, items};
    use crate::SourceKey;
    use herdr_gitea::Issue;

    #[test]
    fn open_issues_become_items_and_pull_requests_are_left_out() -> Result<(), serde_json::Error> {
        let source = SourceKey {
            provider: Provider::Gitea,
            host: "gitea.example.com".into(),
            repository: "o/r".into(),
        };
        let issues: Vec<Issue> = serde_json::from_value(serde_json::json!([
            {"number": 2, "title": "Crash", "body": "Steps", "state": "open",
             "html_url": "https://gitea.example.com/o/r/issues/2",
             "user": {"login": "a"}, "labels": [{"name": "bug"}], "comments": 3,
             "created_at": "2026-10-01T00:00:00Z", "updated_at": "2026-10-02T00:00:00-04:00"},
            {"number": 3, "title": "A pull request", "pull_request": {}},
            {"number": 2, "title": "Crash, read twice"}
        ]))?;
        let items = items(&source, issues);
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.key.native_id, "2");
        assert_eq!(item.identifier, "#2");
        assert_eq!(item.description.as_deref(), Some("Steps"));
        assert_eq!(item.labels, ["bug"]);
        assert_eq!(item.author.as_deref(), Some("a"));
        assert_eq!(item.activity.as_ref().and_then(|a| a.comments), Some(3));
        assert_eq!(
            item.updated_at.map(|time| time.to_rfc3339()),
            Some("2026-10-02T04:00:00+00:00".to_owned())
        );
        assert_eq!(
            Provider::Gitea.as_str().parse::<Provider>().ok(),
            Some(Provider::Gitea)
        );
        Ok(())
    }
}
