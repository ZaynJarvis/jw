# jw (JSON Walker)

**A fast, interactive terminal user interface (TUI) for exploring JSON files and automatically generating `jq` filters.**

`jw` allows you to open large JSON payloads, browse them as a collapsible tree, select specific nodes, and instantly see both the equivalent `jq` filter path and the live JSON output in a split-screen preview. Once you find exactly what you need, simply press `Enter`. The exact `jq` command is dumped to standard output and automatically copied to your clipboard.

![jw demo](https://via.placeholder.com/800x400.png?text=jw+demo)

## Features
- 🚀 **Blazing Fast**: Written in Rust, it handles massive JSON files without UI stuttering or lag.
- 🌳 **Interactive Tree**: Easily collapse and expand nested objects and arrays.
- ✂️ **Visual Selection**: Multi-select fields and automatically construct complex object projections in `jq`.
- 🔍 **Live Preview**: Split-screen design shows the generated `jq` query and real-time evaluated JSON output side-by-side.
- 📋 **Clipboard Sync**: Instantly pipes the generated command to `pbcopy` on macOS for quick pasting into your shell.

## Installation

### From Source
Ensure you have Rust and Cargo installed, then run:

```bash
git clone https://github.com/yourusername/jw.git
cd jw
cargo build --release
# Move the binary to your PATH
mv target/release/jw /usr/local/bin/
```

## Usage

You can pass a file path directly or pipe JSON data via `stdin`:

```bash
jw data.json
# or
cat data.json | jw
# or
curl -s https://api.github.com/repos/rust-lang/rust | jw
```

### Keyboard Controls

| Key(s) | Action |
| --- | --- |
| **`j` / `k`** or **`Up` / `Down`** | Navigate up and down. |
| **`h` / `l`** or **`Left` / `Right`** | Collapse / Expand nodes. |
| **`.`** (dot) | Toggle collapse/expand on the current node. |
| **`Space`** | Toggle selection of the current node. |
| **`v`** | Toggle visual selection mode (select a range of nodes). |
| **`Tab`** | Select current node and move down. |
| **`/`** | Search node values and keys. |
| **`n`** | Jump to the next search result. |
| **`q`** / **`Esc`** | Quit without extracting. |
| **`Enter`** | Generate `jq` command, output to stdout, and copy to clipboard. |

## Future Work
- Better syntax highlighting for the tree view using a robust tokenizer.
- Search optimizations and reverse search (e.g. `N`).
- Native support for Linux/Windows clipboard fallbacks (`xclip`, `wl-copy`, `clip.exe`).
- Scrollbar implementation for extremely tall preview outputs in the right panel.
- Further mapping of complex `jq` iterator behaviors for deeply nested multi-array selections.

## License
MIT License. See [LICENSE](LICENSE) for more details.
