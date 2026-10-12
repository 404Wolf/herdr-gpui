use crate::controls::Command;

// Preserve the existing experimental Windows bindings. Keeping this policy
// separate lets native Windows defaults evolve independently of Linux/macOS.
pub(super) fn defaults(command: Command) -> &'static [&'static str] {
    super::macos::defaults(command)
}
