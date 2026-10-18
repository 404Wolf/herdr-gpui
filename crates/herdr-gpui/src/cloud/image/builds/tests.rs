#![allow(clippy::unwrap_used)]
use super::*;
use crate::cloud::image::catalog::{Origin, tests::isolated};
use std::{
    collections::HashSet,
    sync::{Arc, Barrier},
};

/// What the fake provider holds and did, shared by its plain functions.
static HELD: Mutex<Option<HashSet<String>>> = Mutex::new(None);
static BUILT: Mutex<Vec<String>> = Mutex::new(Vec::new());
static DELETED: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Held by a build until the test lets it finish.
static GATE: Mutex<Option<Arc<Barrier>>> = Mutex::new(None);

fn lock<T>(mutex: &'static Mutex<T>) -> MutexGuard<'static, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

const FAKE: Support = Support {
    platform: |_| Ok("test/arch".into()),
    name: |project, key| {
        format!(
            "img-{}-{}",
            project.id,
            crate::cloud::image::key::short(key)
        )
    },
    find: |name, _, _| {
        Ok(lock(&HELD)
            .get_or_insert_with(HashSet::new)
            .contains(name)
            .then(|| name.to_owned()))
    },
    build: |name, recipe, _, line| {
        line("#1 [1/2] FROM base");
        let gate = lock(&GATE).clone();
        if let Some(gate) = gate {
            gate.wait();
            gate.wait();
        }
        if recipe.dockerfile.contains("FAIL") {
            line("boom");
            return Err(Error::Truncated.into());
        }
        line("#2 [2/2] RUN true");
        lock(&BUILT).push(name.to_owned());
        lock(&HELD)
            .get_or_insert_with(HashSet::new)
            .insert(name.to_owned());
        Ok(name.to_owned())
    },
    delete: |reference, _| {
        lock(&DELETED).push(reference.to_owned());
        lock(&HELD)
            .get_or_insert_with(HashSet::new)
            .remove(reference);
        Ok(())
    },
    context: true,
};

fn recipe(dockerfile: &str) -> Recipe {
    Recipe {
        project: Project {
            id: "builds-test".into(),
            label: "app".into(),
        },
        origin: Origin {
            endpoint_id: "local".into(),
            path: "/src/app".into(),
        },
        dockerfile: dockerfile.into(),
        files: Vec::new(),
    }
}

fn reset_fake() {
    *lock(&HELD) = None;
    lock(&BUILT).clear();
    lock(&DELETED).clear();
    *lock(&GATE) = None;
}

const PROVIDER: CloudProvider = CloudProvider::Container;

// One test: the fake provider's state is process-wide.
#[test]
fn an_image_is_built_once_reused_rebuilt_and_reset() {
    let _catalog = isolated();
    reset_fake();
    let never = || false;
    let first = ensure(PROVIDER, &FAKE, &recipe("FROM a"), &never, &|_| {}).unwrap();
    assert_eq!(lock(&BUILT).len(), 1);
    // Same recipe: reused without a build.
    assert_eq!(
        ensure(PROVIDER, &FAKE, &recipe("FROM a"), &never, &|_| {}).unwrap(),
        first
    );
    assert_eq!(lock(&BUILT).len(), 1);
    // Deleted at the provider behind the app's back: built again.
    lock(&HELD).get_or_insert_with(HashSet::new).clear();
    ensure(PROVIDER, &FAKE, &recipe("FROM a"), &never, &|_| {}).unwrap();
    assert_eq!(lock(&BUILT).len(), 2);
    // A changed recipe builds a new image and deletes the one it replaces.
    let reported = Mutex::new(Vec::new());
    let second = ensure(PROVIDER, &FAKE, &recipe("FROM b"), &never, &|progress| {
        lock_local(&reported).push(progress.clone());
    })
    .unwrap();
    assert_ne!(first, second);
    assert_eq!(*lock(&DELETED), [first]);
    assert_eq!(
        lock_local(&reported).last().unwrap().step,
        Some((2, 2)),
        "progress follows the build's steps"
    );
    // A failed build is recorded with its log, and its error comes back.
    assert!(ensure(PROVIDER, &FAKE, &recipe("FROM b\nFAIL"), &never, &|_| {}).is_err());
    let entry = catalog::load().unwrap().remove(0);
    let Outcome::Failed { log } = &entry.images[0].outcome else {
        panic!("{entry:?}");
    };
    assert_eq!(log.last().unwrap(), &Error::Truncated.to_string());
    assert!(log.contains(&"boom".to_owned()));
    assert!(snapshot().1.is_empty(), "nothing is left running");
    // Reset deletes what is held and forgets it.
    ensure(PROVIDER, &FAKE, &recipe("FROM c"), &never, &|_| {}).unwrap();
    reset(PROVIDER, &FAKE, "builds-test").unwrap();
    assert!(catalog::load().unwrap().is_empty());
    assert_eq!(lock(&DELETED).len(), 2);

    // Two asking at once: one builds, the other waits and reuses it.
    reset_fake();
    let gate = Arc::new(Barrier::new(2));
    *lock(&GATE) = Some(gate.clone());
    let first = std::thread::spawn(|| {
        ensure(PROVIDER, &FAKE, &recipe("FROM d"), &|| false, &|_| {}).unwrap()
    });
    gate.wait();
    // The build is running and visible to views.
    let (revision, running) = snapshot();
    assert_eq!(running.len(), 1);
    assert_eq!(running[0].progress.step, Some((1, 2)));
    let second = std::thread::spawn(|| {
        ensure(PROVIDER, &FAKE, &recipe("FROM d"), &|| false, &|_| {}).unwrap()
    });
    assert!(matches!(
        reset(PROVIDER, &FAKE, "builds-test"),
        Err(crate::Error::SandboxImage(Error::Busy))
    ));
    *lock(&GATE) = None;
    gate.wait();
    let (first, second) = (first.join().unwrap(), second.join().unwrap());
    assert_eq!(first, second);
    assert_eq!(
        lock(&BUILT).len(),
        1,
        "the second waited instead of building"
    );
    assert!(watch(revision, Duration::from_secs(5)).0 > revision);
}

fn lock_local<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
