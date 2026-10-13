//! `cloud list`, read here from the app's saved cloud machines, whether or
//! not Herdr UI runs. Nothing here creates, starts, or destroys a machine.
use super::Output;
#[cfg(feature = "cloud")]
use serde::Serialize;

/// One saved cloud machine, as the command prints it.
#[cfg(feature = "cloud")]
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(super) struct Machine {
    pub provider: String,
    pub id: String,
    pub label: String,
    pub account: String,
    pub session: String,
    pub enabled: bool,
}

#[cfg(feature = "cloud")]
impl From<&crate::cloud::SavedDevice> for Machine {
    fn from(device: &crate::cloud::SavedDevice) -> Self {
        Self {
            provider: crate::cloud::name(device.provider).to_owned(),
            id: device.id.clone(),
            label: device.label.clone(),
            account: device.account.clone(),
            session: device.session.clone(),
            enabled: device.enabled,
        }
    }
}

#[cfg(feature = "cloud")]
pub(super) fn text(machines: &[Machine]) -> String {
    if machines.is_empty() {
        return "No cloud machine is saved; `herdr-gpui cloud add` opens the settings that add one"
            .into();
    }
    machines
        .iter()
        .map(|machine| {
            format!(
                "{} {} ({}, {}) session {}{}",
                machine.provider,
                machine.label,
                machine.id,
                machine.account,
                machine.session,
                if machine.enabled { "" } else { ", turned off" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Blocking file read, which the command line may do.
#[cfg(feature = "cloud")]
pub(super) fn list() -> crate::Result<Output> {
    let machines: Vec<Machine> = crate::cloud::load()?.iter().map(Machine::from).collect();
    let output = Output::data(text(&machines), &machines)?;
    Ok(if machines.is_empty() {
        output.nothing()
    } else {
        output
    })
}

#[cfg(not(feature = "cloud"))]
pub(super) fn list() -> crate::Result<Output> {
    Err(crate::Error::ControlRejected {
        code: crate::control::ErrorCode::Unsupported,
        message: "This build has no cloud providers".into(),
    })
}
