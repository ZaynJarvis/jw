# jw-rs (JSON Walker) - AI Agent Dev Guide

This folder contains a Rust rewrite of the `jw` Python script, designed to be heavily optimized for speed and large JSON payload rendering without the stuttering experienced in Python.

## Architecture
- **Dependencies**: `crossterm` for direct, lightweight terminal manipulation, and `serde_json` for robust parsing. No heavy framework (like `ratatui`) is used to maintain maximal performance and exact control over the render loop.
- **UI Layout**: Split-screen (`w / 2` threshold).
  - **Left Panel**: The interactive JSON tree structure, tracking expanded paths and cursor state.
  - **Right Panel**: The real-time interactive preview, spawning a `std::process::Command` to evaluate the real `jq` path filter dynamically against the loaded JSON string.
- **Rendering**: The entire virtual DOM is structurally cached into a vector of `Row` structs. It is only rebuilt when nodes expand or collapse (`need_rebuild` flag), preventing constant tree walking on every frame.

## Development Context & Future Work
- **Clipboard Output**: Current clipboard logic uses macOS `pbcopy` directly via `Command::new("pbcopy")`.
- **Planned Enhancements**:
  - Implement full scrolling for the right-hand preview panel (it currently uses `saturating_sub` bounds and truncates the remaining lines).
  - Add native rust syntax highlighting (currently just colored brackets and values).
  - Add integration tests around the exact `jq` path string formatting compilation (`path_to_jq` and `path_to_jq_all_arrays`) because it's a critical logic point that can easily drop periods or improperly escape quotes.
  - Provide fallback to `xclip` or `xsel` for Linux environments to handle CI/CD or other platforms without hard-failing `pbcopy`.
