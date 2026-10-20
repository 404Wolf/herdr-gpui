//! Making a Coder workspace on demand, as `cloud create` asks: the template
//! and preset are chosen by name instead of in the dialog, then the same job
//! as Add Device runs.

use super::{
    Error, Preset, Result, Template,
    setup::{self, AddRequest, Source},
};
use crate::cloud::{self, SavedDevice, Spec};

/// How many names an error lists before it stops.
const LISTED: usize = 20;

/// Make the workspace `spec` describes and save it as a device. Blocks.
pub(crate) fn provision(
    spec: &Spec,
    cancelled: &dyn Fn() -> bool,
    report: &dyn Fn(cloud::Step),
) -> crate::Result<SavedDevice> {
    let settings = crate::config::Config::load()?
        .coder
        .settings()?
        .ok_or(Error::Missing("url"))?;
    report(cloud::Step::Connecting);
    let template = template(
        setup::account(&settings)?.templates,
        spec.template.as_deref(),
    )?;
    let preset = preset(
        &template,
        setup::presets(&settings, &template)?,
        spec.preset.as_deref(),
    )?;
    let name = spec
        .name
        .clone()
        .unwrap_or_else(|| super::random_name(&settings.workspace_prefix));
    setup::add_device(
        AddRequest {
            settings,
            source: Source::New {
                name,
                template,
                preset,
            },
            label: String::new(),
            install: spec.install,
        },
        cancelled,
        report,
    )
}

fn listed<'a>(names: impl Iterator<Item = &'a str>) -> String {
    let names: Vec<&str> = names.collect();
    let mut text = names[..names.len().min(LISTED)].join(", ");
    if names.len() > LISTED {
        text.push_str(", …");
    }
    if text.is_empty() {
        text.push_str("none");
    }
    text
}

/// The template `name` names, by name, display name, or ID; with no name,
/// the deployment's only one.
pub(super) fn template(templates: Vec<Template>, name: Option<&str>) -> Result<Template> {
    let available = listed(templates.iter().map(|template| template.name.as_str()));
    let mut found = templates.into_iter().filter(|template| {
        name.is_none_or(|name| {
            template.id == name
                || template.name == name
                || template.display_name.eq_ignore_ascii_case(name)
        })
    });
    match (found.next(), found.next(), name) {
        (Some(template), None, _) | (Some(template), Some(_), Some(_)) => Ok(template),
        _ => Err(Error::Template {
            name: name.map(str::to_owned),
            available,
        }),
    }
}

/// The preset `name` names, by name or ID; with no name, the template's
/// default, if it has one.
pub(super) fn preset(
    template: &Template,
    presets: Vec<Preset>,
    name: Option<&str>,
) -> Result<Option<Preset>> {
    let Some(name) = name else {
        return Ok(presets.into_iter().find(|preset| preset.default));
    };
    let available = listed(presets.iter().map(|preset| preset.name.as_str()));
    presets
        .into_iter()
        .find(|preset| preset.id == name || preset.name.eq_ignore_ascii_case(name))
        .map(Some)
        .ok_or_else(|| Error::Preset {
            template: template.name.clone(),
            name: name.to_owned(),
            available,
        })
}

#[cfg(test)]
mod tests;
