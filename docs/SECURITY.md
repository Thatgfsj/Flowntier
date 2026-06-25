# Security

**Status (v0.4):** initial entry. A full threat model is added in
v0.5 alongside the code-signing work.

## Reporting a vulnerability

**Do not** file a public GitHub issue for security vulnerabilities.

Email `security@flowntier.dev` (TODO: confirm address before v0.4.0
release). Encrypt sensitive reports with the maintainer's PGP key
(TBD — will be added here once keypair is generated).

Please include:

* A clear description of the vulnerability
* Steps to reproduce (PoC preferred)
* The version(s) affected
* Your assessment of severity

You should receive an acknowledgement within 72 hours.

## Threat model (v0.4)

**In scope:**

* **Local secrets.** API keys are encrypted at rest using the OS
  keystore (DPAPI on Windows, Keychain on macOS, libsecret on
  Linux). The plaintext key never touches disk.
* **Tauri webview sandbox.** The webview has no direct filesystem,
  network, or shell permissions; only the permissions listed in
  `apps/desktop/src-tauri/capabilities/default.json` are granted.
* **Auto-update signatures.** Update artifacts are signed with an
  ed25519 keypair held by the maintainer; the public key is
  compiled into the shell binary so signatures are verified before
  installation.

**Out of scope (v0.4):**

* **Code signing on Windows.** The installer is not EV-signed.
  SmartScreen will show "Unknown publisher" — users must click
  "More info → Run anyway". Code signing lands in v0.5.
* **Notarization on macOS.** The `.dmg` is not notarized. Users
  will need to right-click → Open the first time.
* **Sandbox escapes.** The Tauri shell spawns a Rust sidecar binary
  (`flowntier-runtime`) with the user's ambient permissions.
  Malicious input that escapes the agent loop can run arbitrary
  commands.

## Cryptographic posture

| Use                  | Algorithm     | Key location                |
|----------------------|---------------|------------------------------|
| Secret encryption    | AES-GCM-256   | OS keystore (DPAPI/Keychain/libsecret) |
| Auto-update signing  | ed25519       | `TAURI_SIGNING_PRIVATE_KEY` GitHub secret |
| TLS to LLM providers | TLS 1.2+      | System trust store           |

## Known limitations

* No telemetry, no analytics, no phone-home. The only outbound
  network traffic is the user's LLM API calls.
* Logs (`%APPDATA%/flowntier/logs/`) may contain API URLs and
  request IDs but never request bodies or response bodies.