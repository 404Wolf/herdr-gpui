//! The blocking jobs behind the Container tab: make a container, install Herdr
//! in it on approval, and save the device; read what the tab shows; and
//! delete a container with its home volume.

use super::{Engine, Result, Settings, machine};
pub(crate) use super::{
    machine::State,
    settings::{DEFAULT_CPUS, DEFAULT_IMAGE, DEFAULT_MEMORY},
};
use crate::cloud::{self, CloudProvider, SavedDevice, install};

/// The session every container device attaches to.
const SESSION: &str = "default";
/// Container names stay short enough to read in the sidebar.
const NAME_LIMIT: usize = 40;

/// What adding a container needs.
pub(crate) struct AddRequest {
    pub(crate) settings: Settings,
    pub(crate) name: String,
    /// The user approved running Herdr's installer in the container.
    pub(crate) install: bool,
}

/// A default name for a new container, which is also its device ID.
pub(crate) fn random_name() -> String {
    cloud::names::random("herdr", NAME_LIMIT)
}

/// The whole job for `cloud::Jobs`: make the container, install Herdr if
/// approved, and save the device. A failure after the container exists
/// deletes it again.
pub(crate) fn add_device(
    request: AddRequest,
    cancelled: &dyn Fn() -> bool,
    report: &dyn Fn(cloud::Step),
) -> crate::Result<SavedDevice> {
    let AddRequest {
        settings,
        name,
        install,
    } = request;
    let engine = Engine::pick(settings.engine)?;
    report(cloud::Step::Creating);
    machine::create(engine, &name, &settings, cancelled, &|| {
        report(cloud::Step::Building("pulling image".into()));
    })?;
    let installed = install_herdr(engine, &name, install, cancelled, report);
    if let Err(error) = installed {
        if let Err(cleanup) = machine::delete(engine, &name, &|| false) {
            tracing::warn!(category = "container", id = name, error = %cleanup, "Could not remove a failed container");
        }
        return Err(error);
    }
    // Commands such as Teleport's need a running server before any window
    // connects; a failure here still leaves a device a connection can start.
    if let Err(error) = machine::start_server(engine, &name, cancelled) {
        tracing::warn!(category = "container", id = name, %error, "Herdr's server did not start");
    }
    let device = SavedDevice {
        provider: CloudProvider::Container,
        id: name.clone(),
        label: name.clone(),
        account: engine.key().to_owned(),
        machine: name,
        session: SESSION.to_owned(),
        enabled: true,
    };
    cloud::save(device.clone())?;
    Ok(device)
}

fn install_herdr(
    engine: Engine,
    id: &str,
    approved: bool,
    cancelled: &dyn Fn() -> bool,
    report: &dyn Fn(cloud::Step),
) -> crate::Result<()> {
    report(cloud::Step::CheckingHerdr);
    if install::installed(machine::exec(engine, id)?, &cancelled)? {
        return Ok(());
    }
    if !approved {
        return Err(cloud::Error::Install(format!(
            "Herdr is not installed in {id}; allow installing it and try again"
        ))
        .into());
    }
    report(cloud::Step::InstallingHerdr);
    install::install(machine::exec(engine, id)?, &cancelled)?;
    // The installer's own success is not proof the bridge will find it.
    if !install::installed(machine::exec(engine, id)?, &cancelled)? {
        return Err(cloud::Error::Install(
            "the installer finished but Herdr is not on the expected paths".into(),
        )
        .into());
    }
    Ok(())
}

/// Make a ready container with the configured settings and a generated
/// name, installing Herdr, for work that needs a new machine of its own, such
/// as a worktree dispatched to a new container. Blocks: run it on a worker.
pub(crate) fn provision(
    cancelled: &dyn Fn() -> bool,
    report: &dyn Fn(cloud::Step),
) -> crate::Result<SavedDevice> {
    let settings = crate::config::Config::load()?.container.settings()?;
    add_device(
        AddRequest {
            settings,
            name: random_name(),
            install: true,
        },
        cancelled,
        report,
    )
}

/// A saved container and what its engine says about it now.
#[derive(Clone, Debug)]
pub(crate) struct Device {
    pub(crate) saved: SavedDevice,
    /// `None` when the engine could not be asked.
    pub(crate) state: Option<State>,
}

/// What the Container tab shows.
#[derive(Clone, Debug)]
pub(crate) struct Overview {
    /// The engine new containers use and its server version, or why there is
    /// none to use.
    pub(crate) engine: std::result::Result<(Engine, String), String>,
    pub(crate) devices: Vec<Device>,
}

pub(crate) fn overview(settings: &Settings, cancelled: &dyn Fn() -> bool) -> Result<Overview> {
    let quick = std::time::Duration::from_secs(10);
    let engine = Engine::pick(settings.engine)
        .and_then(|engine| {
            let version = engine.run(
                "version",
                ["version", "--format", "{{.Server.Version}}"],
                quick,
                cancelled,
            )?;
            Ok((engine, version))
        })
        .map_err(|error| error.to_string());
    let devices = cloud::load()?
        .into_iter()
        .filter(|saved| saved.provider == CloudProvider::Container)
        .map(|saved| {
            let state = Engine::from_key(&saved.account)
                .and_then(|engine| machine::state(engine, &saved.id, cancelled).ok());
            Device { saved, state }
        })
        .collect();
    Ok(Overview { engine, devices })
}

/// Delete the saved container `id` with its home volume, then forget it. A
/// device no longer saved has nothing left to do. Blocks: run it on a worker.
pub(crate) fn destroy_saved(id: &str) -> crate::Result<()> {
    let Some(device) = cloud::load()?
        .into_iter()
        .find(|device| device.provider == CloudProvider::Container && device.id == id)
    else {
        return Ok(());
    };
    let engine =
        Engine::from_key(&device.account).ok_or(super::Error::Field("container engine"))?;
    machine::delete(engine, id, &|| false)?;
    Ok(cloud::remove(CloudProvider::Container, id)?)
}

/// Stop the saved container `id`, keeping its files for the next connection.
pub(crate) fn stop_saved(id: &str) -> crate::Result<()> {
    let Some(device) = cloud::load()?
        .into_iter()
        .find(|device| device.provider == CloudProvider::Container && device.id == id)
    else {
        return Ok(());
    };
    let engine =
        Engine::from_key(&device.account).ok_or(super::Error::Field("container engine"))?;
    Ok(machine::stop(engine, id, &|| false)?)
}
