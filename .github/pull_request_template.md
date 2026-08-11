## What changed

Describe the implementation and affected user flow.

## Why

Explain the problem or requirement this change addresses.

## Validation

- [ ] `npm test`
- [ ] `npm run build`
- [ ] `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml`
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
- [ ] Real ComfyUI test where applicable

## Safety and licensing

- [ ] No model weights, generated videos, credentials, logs, or private paths were committed.
- [ ] Download, filesystem, and network changes preserve the documented security boundary.
- [ ] Third-party license implications are documented.
