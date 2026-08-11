# Changelog

All notable changes to MagineStudio are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.0] - 2026-08-11

### Added

- Windows hardware, NVIDIA GPU, VRAM, driver, disk, and environment detection.
- Existing ComfyUI and MiniMax H3 model discovery.
- Configurable ComfyUI, model, cache, and output paths.
- Resumable verified downloads and ComfyUI model-path configuration.
- Local ComfyUI process lifecycle and API health checks.
- Text, image, first/last-frame, and reference generation interfaces.
- Real-time task progress, estimated duration, cancellation, and video preview.
- Persistent creation settings and task selection.

### Security

- Restricted the managed ComfyUI service to the loopback interface.
- Restricted external URLs and cache deletion targets.
- Added download integrity validation and path boundary checks.

[Unreleased]: https://github.com/Magine1921/MagineStudio/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/Magine1921/MagineStudio/releases/tag/v1.0.0
