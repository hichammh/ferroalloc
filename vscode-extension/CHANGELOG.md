# Changelog

## [0.1.4] — 2026-05-14

### Fixed
- Exclude demo GIF from VSIX package to reduce extension size (GIF served via GitHub URL)

## [0.1.3] — 2026-05-14

### Fixed
- Demo GIF not displaying on VS Code Marketplace: switched to absolute GitHub URL

## [0.1.2] — 2026-05-14

### Changed
- Replace static demo image with animated GIF in README

## [0.1.1] — 2026-05-14

### Fixed
- Crash in flush thread on macOS caused by reentrant backtrace calls during deallocation

## [0.1.0] — 2026-05-14

### Added
- **CodeLens**: displays live allocation count and total bytes per source line
- **Heatmap**: green-to-red background highlighting by allocation pressure (5 intensity levels)
- **Leak detection**: `ferroalloc: Show Live Leaks` lists lines with unreleased memory
- **Snapshot diff**: `ferroalloc: Save Memory Baseline` + `ferroalloc: Show Diff Since Baseline`
- **Status bar**: live byte counter with click-to-toggle tracking
- Auto-start / auto-stop when a VS Code debug session begins and ends
- Configurable analyzer port, refresh interval, and heatmap toggle
