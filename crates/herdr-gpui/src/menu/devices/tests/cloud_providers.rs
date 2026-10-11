use super::*;
use crate::cloud::CloudProvider;

/// Providers that need no account or URL, so the picker always offers them.
fn anonymous(provider: CloudProvider) -> bool {
    match provider {
        #[cfg(feature = "coder")]
        CloudProvider::Coder => false,
        #[cfg(feature = "daytona")]
        CloudProvider::Daytona => false,
        #[cfg(feature = "railway")]
        CloudProvider::Railway => true,
    }
}

/// The setting that makes `provider` suggested, or none for an account-free one.
fn setting(view: &mut HerdrWindow, provider: CloudProvider) -> Option<&mut Option<String>> {
    match provider {
        #[cfg(feature = "coder")]
        CloudProvider::Coder => Some(&mut view.config.coder.url),
        #[cfg(feature = "daytona")]
        CloudProvider::Daytona => Some(&mut view.config.daytona.api_url),
        #[cfg(feature = "railway")]
        CloudProvider::Railway => {
            let _ = view;
            None
        }
    }
}

/// An example account URL for `provider`, as `(environment variable, value)`.
fn account(provider: CloudProvider) -> Option<(&'static str, &'static str)> {
    match provider {
        #[cfg(feature = "coder")]
        CloudProvider::Coder => Some(("HERDR_CODER_URL", "https://coder.example.com")),
        #[cfg(feature = "daytona")]
        CloudProvider::Daytona => Some(("HERDR_DAYTONA_API_URL", "https://app.daytona.io/api")),
        #[cfg(feature = "railway")]
        CloudProvider::Railway => None,
    }
}

fn available() -> bool {
    crate::cloud::unavailable().is_none()
}

/// What the picker offers when only `configured` is set up, in picker order.
fn expected(configured: &[CloudProvider]) -> Vec<CloudProvider> {
    if !available() {
        return Vec::new();
    }
    CloudProvider::ALL
        .iter()
        .copied()
        .filter(|&provider| anonymous(provider) || configured.contains(&provider))
        .collect()
}

#[gpui::test]
fn provider_suggestions_follow_configuration_changes(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            for &provider in CloudProvider::ALL {
                let Some((_, url)) = account(provider) else {
                    // No configuration to change: it is offered wherever
                    // cloud devices can connect, and nowhere else.
                    assert_eq!(
                        view.cloud_providers_with_env(|_| None).contains(&provider),
                        available(),
                        "an account-free provider is offered without configuration",
                    );
                    assert!(setting(view, provider).is_none());
                    continue;
                };
                *setting(view, provider).unwrap() = None;
                assert!(
                    !view.cloud_providers_with_env(|_| None).contains(&provider),
                    "{provider:?} must not be suggested without configuration",
                );
                *setting(view, provider).unwrap() = Some(url.into());
                assert_eq!(
                    view.cloud_providers_with_env(|_| None).contains(&provider),
                    available(),
                );
                *setting(view, provider).unwrap() = None;
                assert!(
                    !view.cloud_providers_with_env(|_| None).contains(&provider),
                    "removing configuration must hide the suggestion again",
                );
                assert_eq!(view.cloud_provider_rows(), view.cloud_providers().len());
            }
        });
    });
}

#[gpui::test]
fn provider_suggestions_follow_environment_only_configuration(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            for &provider in CloudProvider::ALL {
                if let Some(url) = setting(view, provider) {
                    *url = None;
                }
            }
            let mut configured = Vec::new();
            for &provider in CloudProvider::ALL {
                // Nothing to configure, in the environment or the file.
                let Some((variable, url)) = account(provider) else {
                    continue;
                };
                configured.push((variable, url));
                assert_eq!(
                    view.cloud_providers_with_env(|name| (name == variable).then(|| url.into())),
                    expected(&[provider]),
                    "{variable} alone must suggest only {provider:?} besides account-free ones",
                );
                assert_eq!(
                    view.cloud_providers_with_env(|_| None),
                    expected(&[]),
                    "removing {variable} must hide its suggestion again",
                );
            }
            assert_eq!(
                view.cloud_providers_with_env(|name| configured
                    .iter()
                    .find(|(variable, _)| *variable == name)
                    .map(|(_, url)| (*url).into())),
                expected(CloudProvider::ALL),
                "environment-only providers must retain picker order",
            );
        });
    });
}
