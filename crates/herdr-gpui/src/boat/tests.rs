use super::*;
pub(crate) use super::{api::wait_ready, http::Request};

pub(crate) mod mock;

pub(crate) fn settings() -> Settings {
    Settings {
        base: "https://boat.example.com/api/v1".into(),
        organization: None,
        machine_type: None,
        environment: None,
        store: crate::github::Store::Environment,
    }
}
