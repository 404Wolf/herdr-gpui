//! `security`: the private vulnerability reports and Dependabot alerts the
//! Security section last read. It starts no read of its own; a window that
//! has not read yet says so. Reports carry their titles and metadata only.

use crate::{
    HerdrWindow,
    control::protocol::{
        Response,
        hosts::{Reply, SecurityAlertInfo, SecurityRepoInfo, SecurityReportInfo},
    },
    security::{Alerts, Repo, Reports},
};

/// The most alerts one answer carries, so it stays well inside the control
/// socket's message bound; each repository's `total` still counts them all.
pub(super) const MAX_ALERTS: usize = 120;
/// The most reports one answer carries.
pub(super) const MAX_REPORTS: usize = 50;

pub(super) fn alerts(view: &HerdrWindow) -> Response {
    let security = &view.security;
    let (repositories, cut) = listing(security.repos());
    let note = if cut {
        Some(format!(
            "Showing the first {MAX_ALERTS} alerts and {MAX_REPORTS} reports; the Security section and GitHub list the rest"
        ))
    } else if security.read_at().is_none() {
        Some(if view.main_tokens().is_none() {
            "Sign in to GitHub in Herdr UI to read Dependabot alerts".to_owned()
        } else {
            "Herdr UI has not read the alerts yet".to_owned()
        })
    } else if repositories.is_empty() {
        Some("No github.com repository is open in this window's workspaces".to_owned())
    } else {
        None
    };
    Response::Host {
        reply: Reply::Security { repositories, note },
    }
}

/// Each repository with its alerts and reports, up to [`MAX_ALERTS`] and
/// [`MAX_REPORTS`] in all, and whether any were left out.
pub(super) fn listing(repos: &[Repo]) -> (Vec<SecurityRepoInfo>, bool) {
    let mut budget = MAX_ALERTS;
    let mut reports_budget = MAX_REPORTS;
    let mut cut = false;
    let repositories = repos
        .iter()
        .map(|repo| {
            let (state, total, error) = match &repo.alerts {
                Ok(Alerts::Open { total, .. }) => ("open", *total, None),
                Ok(Alerts::Off) => ("off", 0, None),
                Err(error) => ("error", 0, Some(error.to_string())),
            };
            let open = repo.open();
            let taken = open.len().min(budget);
            cut |= taken < open.len();
            budget -= taken;
            let (reporting, reports_error) = match &repo.reports {
                Ok(Reports::Open(_)) => ("on", None),
                Ok(Reports::Off) => ("off", None),
                Ok(Reports::Hidden) => ("hidden", None),
                Err(error) => ("error", Some(error.to_string())),
            };
            let reported = repo.reported();
            let shown = reported.len().min(reports_budget);
            cut |= shown < reported.len();
            reports_budget -= shown;
            SecurityRepoInfo {
                name: repo.name.clone(),
                host: repo.host.clone(),
                url: repo.alerts_url(),
                state: state.to_owned(),
                total,
                alerts: open[..taken]
                    .iter()
                    .map(|alert| SecurityAlertInfo {
                        number: alert.number,
                        severity: alert.severity.name().to_owned(),
                        package: alert.package.clone(),
                        ecosystem: alert.ecosystem.clone(),
                        summary: alert.summary.clone(),
                        advisory: alert.advisory.clone(),
                        advisory_url: alert.advisory_url(),
                        vulnerable: alert.vulnerable.clone(),
                        patched: alert.patched.clone(),
                        manifest: alert.manifest.clone(),
                        url: repo.alert_url(alert),
                    })
                    .collect(),
                error,
                reporting: reporting.to_owned(),
                reports: reported[..shown]
                    .iter()
                    .map(|report| SecurityReportInfo {
                        advisory: report.advisory.clone(),
                        summary: report.summary.clone(),
                        severity: report.severity.map(|severity| severity.name().to_owned()),
                        reporter: report.reporter.clone(),
                        created_at: report.created_at.map(|at| at.to_rfc3339()),
                        url: repo.report_url(report),
                    })
                    .collect(),
                reports_error,
            }
        })
        .collect();
    (repositories, cut)
}
