# Security policy

## Supported versions

Security fixes are applied to the latest version on the `main` branch. Older
development snapshots are not supported.

## Reporting a vulnerability

Do not publish an exploitable vulnerability in a public issue. Use this
repository's [private vulnerability reporting form](https://github.com/Magine1921/MagineStudio/security/advisories/new).
Include:

- the affected version or commit;
- reproduction steps and expected impact;
- relevant logs with tokens, usernames, and local paths removed;
- a proposed mitigation, if available.

Please allow a reasonable period for validation and remediation before public
disclosure.

## Security boundaries

- The managed ComfyUI service is intended to listen on `127.0.0.1` only.
- MagineStudio validates managed download hosts, download hashes, target
  directories, and locally opened URLs before use.
- Model weights and ComfyUI are downloaded from third-party services and are
  subject to their availability, integrity metadata, and licenses.
- Generated media, prompts, local paths, and settings remain on the user's
  computer unless the user explicitly uses another service.
- A user-installed custom ComfyUI node executes code with the user's local
  permissions and is outside MagineStudio's trust boundary.

If a build changes any of these boundaries, document the change in the pull
request and update this policy.
