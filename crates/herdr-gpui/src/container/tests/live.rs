//! Opt-in: makes a real container on the engine named by
//! `HERDR_TEST_CONTAINER_ENGINE` (`docker` or `podman`), installs Herdr in it
//! from the network, attaches the stdio bridge, and deletes it again. Run with
//! `cargo test -p herdr-gpui container::tests::live -- --ignored`.

use super::*;
use std::sync::atomic::AtomicBool;

#[test]
#[ignore = "needs a running Docker or Podman engine and network access"]
fn a_container_is_made_reached_and_deleted() {
    let engine = std::env::var("HERDR_TEST_CONTAINER_ENGINE")
        .ok()
        .and_then(|key| Engine::from_key(&key))
        .unwrap_or_else(|| panic!("set HERDR_TEST_CONTAINER_ENGINE to docker or podman"));
    let settings = Settings {
        engine: Some(engine),
        ..Settings::resolve(&ContainerConfig::default()).unwrap()
    };
    let id = setup::random_name();
    let never = || false;
    let result = (|| -> crate::Result<()> {
        machine::create(engine, &id, &settings, &never, &|| {})?;
        assert_eq!(
            machine::state(engine, &id, &never)?,
            machine::State::Running
        );
        // The agents' user owns its home.
        let owner = engine.run(
            "exec",
            [
                "exec",
                "--user",
                machine::USER,
                &id,
                "stat",
                "-c",
                "%U",
                machine::HOME,
            ],
            std::time::Duration::from_secs(30),
            &never,
        )?;
        assert_eq!(owner, machine::USER);
        assert!(!crate::cloud::install::installed(
            machine::exec(engine, &id)?,
            &never
        )?);
        crate::cloud::install::install(machine::exec(engine, &id)?, &never)?;
        assert!(crate::cloud::install::installed(
            machine::exec(engine, &id)?,
            &never
        )?);
        // The bridge answers discovery, so a connection would work.
        let bridge = herdr_client::connect_command(
            machine::exec(engine, &id)?,
            "default",
            &AtomicBool::new(false),
        )
        .map_err(Error::Bridge)?;
        drop(bridge);
        // A stopped container starts again on the next connection.
        machine::stop(engine, &id, &never)?;
        assert_eq!(
            machine::state(engine, &id, &never)?,
            machine::State::Stopped
        );
        machine::ensure_running(engine, &id, &never)?;
        assert_eq!(
            machine::state(engine, &id, &never)?,
            machine::State::Running
        );
        Ok(())
    })();
    machine::delete(engine, &id, &never).unwrap();
    assert_eq!(
        machine::state(engine, &id, &never).unwrap(),
        machine::State::Missing
    );
    result.unwrap();
}
