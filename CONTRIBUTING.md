# Contributing to MagineStudio

Thank you for helping improve MagineStudio.

## Before opening a change

- Search existing issues and pull requests.
- Keep changes focused and avoid committing model weights, generated videos,
  local settings, logs, download caches, or machine-specific paths.
- For user-visible behavior, describe the real backend capability; do not add
  sample-only buttons or fabricated runtime data to the desktop application.
- Preserve the local-only security boundary unless the change explicitly
  documents and reviews a broader network surface.

## Development setup

Requirements:

- Windows 10 or 11 x64;
- Node.js 24 or later;
- Rust stable with the MSVC target;
- Visual Studio C++ Build Tools and WebView2 Runtime.

```powershell
npm ci
npm run tauri dev
```

`npm run dev` opens a browser-only visual preview. Hardware information and
installation state in that preview are simulated; local generation requires
the Tauri desktop runtime.

## Required checks

Run these commands before submitting a pull request:

```powershell
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

Changes to installation, paths, ComfyUI communication, or generation should
also be tested against a real local ComfyUI instance where practical.

## Pull requests

Explain what changed, why it is needed, user impact, validation performed, and
any license or security implications. By submitting a contribution, you agree
that it is licensed under Apache License 2.0 as described in `LICENSE`.
