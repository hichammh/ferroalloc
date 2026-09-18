# Changelog

## [0.1.6] — 2026-09-18

Requires `ferroalloc-probe` 0.1.2 and `ferroalloc-analyzer` 0.1.1. Update the
analyzer too — `cargo install ferroalloc-analyzer --force` — otherwise the new
diagnostics stay silent.

### Fixed
- Sampling no longer reports correct programs as leaking. The decision is now
  derived from the block address, so a recorded allocation always has its free
  recorded too; previously `live_bytes` never came back down under
  `set_sample_rate(n)`
- A saturated event queue no longer turns freed blocks into permanent leaks:
  deallocations are kept past the allocation limit, and the number of discarded
  events is reported rather than silently changing the numbers
- A deallocation arriving after a reset no longer inserts an entry with an empty
  file and line 0 into the snapshot
- Memory deltas print correctly when negative: a 3 MB drop showed as `-3145728 B`

### Added
- The status bar explains an empty view instead of reporting "tracking": it now
  distinguishes a build without debug symbols from dropped events
- `/health` counts every event received, not only the attributable ones, and
  reports `events_dropped`

### Changed
- `[profile.release] debug = true` removed from the probe's manifest, where Cargo
  ignored it. It belongs in the Cargo.toml of the program under test — the README
  now says so, and the status bar says so when the symbols are missing
- Allocation counts differ from 0.1.5 when sampling is enabled

## [0.1.5] — 2026-05-15

### Fixed
- File path matching now prefers exact match before suffix to avoid mixing stats from files with identical names in different directories

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
