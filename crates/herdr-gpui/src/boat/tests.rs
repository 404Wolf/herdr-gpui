use super::*;
pub(crate) use super::{api::wait_ready, http::Request};

pub(crate) mod mock;

pub(crate) fn settings() -> Settings {
    Settings {
        base: "https://boat.example.com/api/v1".into(),
        organization: None,
        machine_type: None,
        environment: None,
        ttl_seconds: None,
        store: crate::github::Store::Environment,
    }
}

impl Request<'_> {
    pub(crate) fn create_for_test(
        &self,
        name: &str,
        cancelled: impl Fn() -> bool,
        remember: impl FnOnce(&api::Sandbox) -> Result<()>,
    ) -> Result<api::Sandbox> {
        let creation = pending::Creation::new(self.settings, name, 0);
        self.create(&creation, cancelled, remember)
    }
}
