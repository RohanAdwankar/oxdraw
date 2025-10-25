
https://github.com/user-attachments/assets/4967abab-794e-4449-9b7c-a4d8fa1f22cd

## Vision

The goal of `oxdraw` is to make it easy to create and maintain high-quality diagrams using a declaraitive and reproducible syntax.
Charts are written in [Mermaid](https://mermaid.js.org/) syntax, while a web interface allows users to fine-tune positions connector paths, colors, and other styling components. Whenever a diagram is tweaked visually, the structural changes are persisted back to the source file as declarative code so that everything remains deterministic and versionable.
The changes are saved as comments in the mermaid file so it remains compatible with other Mermaid tools.

The long-term architecture comprises two layers:

- **Rust CLI** – compiles `.mmd` sources into images 
- **React web interface** – provides direct-manipulation editing (dragging nodes, reshaping edges) and writes compatible oxdraw metadata alongside the Mermaid definition.

## Usage

### Build the CLI

```bash
cargo build --release
```

### Render a diagram from a file

```bash
./target/release/oxdraw --input tests/input/flow.mmd  
```

### Launch the interactive editor

Then start the editor against a specific diagram:

```bash
./target/release/oxdraw --input mermaid-cli/test-positive/flowchart1.mmd --edit
```


## Features

### CLI Flags

| Flag | Description |
| --- | --- |
| `-i, --input <PATH>` | Read a Mermaid source file; pass `-` to consume stdin instead. Required when using `--edit`. |
| `-o, --output <PATH>` | Write the rendered asset to a specific path; pass `-` to stream SVG to stdout. Defaults to `<input>.svg` (or `<input>.<format>` if an explicit format is chosen) and `out.svg` when reading from stdin. |
| `-e, --output-format <svg|png>` | Override format detection from the output file extension. Use when the destination name lacks a recognizable extension. |
| `--png` | Shorthand for `--output-format png`; keeps CLI usage terse when you only need a raster output. |
| `--scale <FACTOR>` | Scale multiplier for PNG rasterization (default `10.0`); values must be greater than zero. Ignored for SVG output. |
| `--edit` | Launch the interactive editor pointing at the supplied diagram instead of emitting an asset once. Conflicts with `--output`/`--output-format` and requires a file input. |
| `--serve-host <ADDR>` | Override the bind address used while `--edit` is active (default `127.0.0.1`). |
| `--serve-port <PORT>` | Override the HTTP port while `--edit` is active (default `5151`). |
| `-b, --background-color <COLOR>` | Background fill passed to the renderer (currently SVG only). Applies to both one-off renders and the editor preview. |
| `-q, --quiet` | Suppress informational stdout such as the success message after rendering to disk. |
| `oxdraw serve --input <PATH>` | Start the sync API server that powers the editor. Optional flags: `--host` (default `127.0.0.1`), `--port` (default `5151`), and `--background-color`. |

### Frontend Features

| Control | What it does |
| --- | --- |
| `Reset overrides` | Clears every stored node position and edge control-point override, snapping the diagram back to the auto layout. |
| `Delete selected` | Removes the currently selected node or edge; available via the Delete/Backspace keys as well. |
| Node Fill/Stroke/Text pickers | Apply per-node color overrides; double-clicking a node clears its override. |
| `Reset node style` | Remove all color overrides for the selected node. |
| Edge Color picker | Override the selected edge stroke color. |
| Edge Line selector | Toggle between solid and dashed stroke styles. |
| Edge Arrow selector | Choose arrow directions (forward/backward/both/none). |
| `Add control point` | Insert a new draggable waypoint on the selected edge to fine-tune routing. |
| `Reset edge style` | Drop edge-specific styling and revert to defaults; double-clicking an edge handle also clears its manual path. |

**Canvas and editor interactions**

- Drag nodes to update their stored positions with grid snapping and live alignment guides; Shift+Arrow nudges the selection in grid-sized jumps.
- Drag edge handles (or the label handle) to reshape routes; double-click an edge to insert a handle and double-click a handle to remove overrides.
- Drag an entire subgraph container to move all of its member nodes (and any edge overrides) together while maintaining separation from sibling groups.
- Right-click nodes or edges to open a context menu with a delete action.
- The source panel mirrors the Mermaid file, auto-saves after short idle periods, and surfaces pending/saving/error states alongside the current selection.
- Status text in the top toolbar signals loading, saving, and the currently edited file path.
