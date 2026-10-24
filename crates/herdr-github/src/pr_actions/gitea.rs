//! The review conversation, comments, and merges of pull requests on Gitea
//! hosts. A Gitea lookup gives its pull request an ID with the `gitea:`
//! prefix; targets made from one are answered here, over Gitea's REST API,
//! under the same rules as GitHub's: a comment or merge is sent once and never
//! retried, and a merge names the head commit the user saw.

use super::{
    COMMENT_LIMIT, Comment, CommentKind, Completion, Job, Outcome, PATH_LIMIT, Review,
    SOURCE_LIMIT, Target, bounded, text,
};
use crate::{
    Error,
    pull_request::{
        GITEA_ID_PREFIX as ID_PREFIX, MergeMethod, PullRequest, gitea_from as from_gitea,
    },
};
use herdr_gitea::{Client, Comment as GiteaComment, Repo, Review as GiteaReview, ReviewComment};
use serde_json::json;
use std::time::Duration;

#[cfg(test)]
mod tests;

const TIMEOUT: Duration = super::API_TIMEOUT;
/// Reviews whose inline comments are read, newest first.
const THREAD_REVIEWS: usize = 20;

/// Whether `target` is a Gitea pull request's.
pub(super) fn owns(target: &Target) -> bool {
    target.id.starts_with(ID_PREFIX)
}

/// A target for a Gitea lookup's pull request, checked against the host its
/// address names; `None` when `pr` is GitHub's.
pub(super) fn target(pr: &PullRequest, writable: bool) -> Option<crate::Result<Target>> {
    if !pr.id.starts_with(ID_PREFIX) {
        return None;
    }
    Some((|| {
        if pr.head_ref_oid.is_empty() {
            return Err(Error::PrActionTarget);
        }
        let (repo, number) = herdr_gitea::pull_url(&pr.url).ok_or(Error::PrActionTarget)?;
        if number != pr.number || pr.id != format!("{ID_PREFIX}{number}") {
            return Err(Error::PrActionTarget);
        }
        Ok(Target {
            owner: repo.owner,
            repo: repo.name,
            number,
            url: pr.url.clone(),
            id: pr.id.clone(),
            head: pr.head_ref_oid.clone(),
            writable,
        })
    })())
}

/// The repository `target` addresses, on the host its address names.
fn repo(target: &Target) -> crate::Result<Repo> {
    let (repo, number) = herdr_gitea::pull_url(&target.url).ok_or(Error::PrActionTarget)?;
    if number != target.number {
        return Err(Error::PrActionTarget);
    }
    Ok(repo)
}

pub(super) fn complete(target: &Target, job: Job, cancelled: impl Fn() -> bool) -> Completion {
    match job {
        Job::Conversation => Completion::Conversation(conversation(target, &cancelled)),
        Job::Run(action) => Completion::Action(perform(target, action)),
    }
}

fn conversation(target: &Target, cancelled: &impl Fn() -> bool) -> crate::Result<Vec<Comment>> {
    let repo = repo(target)?;
    let client = Client::new(&repo.host, TIMEOUT).map_err(from_gitea)?;
    let number = target.number;
    let (comments, _): (Vec<GiteaComment>, bool) = client
        .get_pages(
            &repo.api(&format!("/issues/{number}/comments")),
            2,
            cancelled,
        )
        .map_err(from_gitea)?;
    let (reviews, _): (Vec<GiteaReview>, bool) = client
        .get_pages(&repo.api(&format!("/pulls/{number}/reviews")), 2, cancelled)
        .map_err(from_gitea)?;
    let mut threads = Vec::new();
    for review in reviews
        .iter()
        .rev()
        .filter(|review| review.comments_count > 0)
        .take(THREAD_REVIEWS)
    {
        let inline: Vec<ReviewComment> = client
            .get_optional(
                &repo.api(&format!("/pulls/{number}/reviews/{}/comments", review.id)),
                cancelled,
            )
            .map_err(from_gitea)?
            .unwrap_or_default();
        threads.extend(inline);
    }
    Ok(assemble(&comments, &reviews, &threads))
}

/// The conversation as the shared view shows it: comments, verdicts, and the
/// first inline comment on each file, oldest first and bounded.
pub(super) fn assemble(
    comments: &[GiteaComment],
    reviews: &[GiteaReview],
    threads: &[ReviewComment],
) -> Vec<Comment> {
    let mut entries = Vec::new();
    for comment in comments {
        entries.push(entry(
            comment.user.as_ref().map(|user| user.login.as_str()),
            &comment.body,
            &comment.created_at,
            CommentKind::Conversation,
        ));
    }
    for review in reviews {
        let verdict = match review.state.as_str() {
            _ if review.dismissed => Review::Dismissed,
            "APPROVED" => Review::Approved,
            "REQUEST_CHANGES" => Review::ChangesRequested,
            "COMMENT" => Review::Commented,
            // A pending review is its author's unsubmitted draft, and a
            // review request is not a review.
            _ => continue,
        };
        // An empty comment review only wraps inline comments, listed on their own.
        if verdict == Review::Commented && text(&review.body.clone().into()).is_empty() {
            continue;
        }
        entries.push(entry(
            review.user.as_ref().map(|user| user.login.as_str()),
            &review.body,
            &review.submitted_at,
            CommentKind::Review(verdict),
        ));
    }
    // One entry per file, as GitHub's view lists one per thread.
    let mut paths: Vec<(&str, usize)> = Vec::new();
    for thread in threads {
        match paths.iter_mut().find(|(path, _)| *path == thread.path) {
            Some((_, replies)) => *replies += 1,
            None => paths.push((thread.path.as_str(), 0)),
        }
    }
    for (path, replies) in paths {
        let Some(first) = threads.iter().find(|thread| thread.path == path) else {
            continue;
        };
        entries.push(entry(
            first.user.as_ref().map(|user| user.login.as_str()),
            &first.body,
            &first.created_at,
            CommentKind::Thread {
                path: bounded(&text(&path.into()), PATH_LIMIT),
                resolved: first.resolver.is_some(),
                replies: replies as u64,
            },
        ));
    }
    entries.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    let excess = entries.len().saturating_sub(COMMENT_LIMIT);
    entries.drain(..excess);
    entries
}

fn entry(author: Option<&str>, body: &str, time: &str, kind: CommentKind) -> Comment {
    let author = bounded(
        &text(&author.unwrap_or_default().into()),
        super::AUTHOR_LIMIT,
    );
    Comment {
        // A deleted account is Gitea's "Ghost".
        author: if author.is_empty() {
            "ghost".into()
        } else {
            author
        },
        kind,
        body: text(&body.into()),
        created_at: bounded(&text(&time.into()), 32),
        source: bounded(body, SOURCE_LIMIT),
        avatar: None,
        bot: false,
    }
}

fn perform(target: &Target, action: super::Action) -> crate::Result<Outcome> {
    if !target.writable {
        return Err(Error::PrActionTarget);
    }
    let repo = repo(target)?;
    let client = Client::new(&repo.host, TIMEOUT).map_err(from_gitea)?;
    let number = target.number;
    match action {
        super::Action::Comment(body) => {
            let reply = client
                .post(
                    &repo.api(&format!("/issues/{number}/comments")),
                    &json!({"body": body}),
                )
                .map_err(from_gitea)?;
            Ok(commented(&reply, target))
        }
        super::Action::Merge(method) => {
            client
                .post(
                    &repo.api(&format!("/pulls/{number}/merge")),
                    &json!({"Do": style(method), "head_commit_id": target.head}),
                )
                .map_err(from_gitea)?;
            Ok(Outcome {
                message: format!("Merged pull request #{number}"),
                url: Some(target.url.clone()),
            })
        }
    }
}

/// Gitea's `Do` for a merge method.
pub(super) fn style(method: MergeMethod) -> &'static str {
    match method {
        MergeMethod::Merge => "merge",
        MergeMethod::Squash => "squash",
        MergeMethod::Rebase => "rebase",
    }
}

/// The comment exists once Gitea answered, so an odd reply is still a
/// success. Only a link on this pull request is offered.
pub(super) fn commented(reply: &serde_json::Value, target: &Target) -> Outcome {
    let url = reply["html_url"]
        .as_str()
        .filter(|url| {
            url.strip_prefix(target.url.as_str())
                .and_then(|anchor| anchor.strip_prefix("#issuecomment-"))
                .is_some_and(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
        })
        .map(str::to_owned);
    Outcome {
        message: format!("Commented on pull request #{}", target.number),
        url,
    }
}
