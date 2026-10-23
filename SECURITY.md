# Security Policy

## Reporting a vulnerability

Do not open a public issue or pull request for a security problem.

Report it privately through GitHub: open the repository's **Security** tab, choose **Report a vulnerability**, and describe the problem there. Only the maintainers can see the report.

Include:

- The affected area (Tauri app, Rust core, IPC commands, sync, frontend).
- The version or commit hash you tested.
- Steps to reproduce, and the impact you expect.

Do not include real credentials, tokens or user data in the report. Redact them.

## Supported versions

LUNOR-X Desktop is in pre-release (`1.0.0-rc.x`). Only the latest release candidate and the `main` branch are maintained, and security fixes land there.

## Scope notes

- Provider credentials and tokens must not appear in IPC events or logs. They stay in OS-protected storage and are never sent to the backend.
- Tauri capabilities are restricted; native permissions are never exposed to remote content.
- The YouTube sign-in window loads Google pages only. Its navigation is limited to an allowlist of Google and YouTube hosts.
