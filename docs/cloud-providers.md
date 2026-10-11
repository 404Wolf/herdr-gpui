# Cloud provider notes

## Railway free VMs (`ssh railway.new`)

Observed live on 2026-10-11. No account or token: the SSH key is the identity,
and the same key always returns the same VM.

- **First use:** creating the VM must be an interactive (non-headless) SSH
  session. A headless attempt is refused (exit 13, JSON on stdout).
- **Reconnect:** once the VM exists, headless `ssh -T railway.new CMD` works and
  returns the same VM. Reuse the stored key; never generate another to get a VM.
- **Manifest:** each headless connection writes JSON to stderr. The deadline is
  `build_expires_at` (UTC, creation + 60 minutes). The claim and signup links in
  it are credentials: never log or open them.
- **Limits:** 60-minute build window, then 24 hours to claim before the VM and
  its files are deleted. 3 new VMs per IP per day; reconnecting does not use one.
- **Verified:** the standard Herdr installer (`curl … install.sh | sh`) works;
  the client's probe and bridge scripts run unchanged and report a compatible
  Herdr; raw binary over headless stdin/stdout is intact.
- **Not verified:** a full GUI session, the bridge starting the server, and SSH
  behavior after expiry.
