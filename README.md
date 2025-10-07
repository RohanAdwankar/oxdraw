
https://github.com/user-attachments/assets/4967abab-794e-4449-9b7c-a4d8fa1f22cd

## Vision

Oxdraw is a toolchain for declarative diagram authoring. Charts are written in [Mermaid](https://mermaid.js.org/) syntax, while an upcoming web studio will let users fine-tune positions and connector paths. Whenever a diagram is tweaked visually, the structural changes are persisted back to the source file as declarative code so that everything remains deterministic and versionable.

The long-term architecture comprises two layers:

- **Rust CLI** – compiles `.mmd` sources into reproducible assets (`.svg`, `.png`, later `.pdf`).
- **React web interface** – provides direct-manipulation editing (dragging nodes, reshaping edges) and writes compatible oxdraw metadata alongside the Mermaid definition.

The CLI is the foundation for the full experience, so we are building it first using TDD and by staying compatible with the existing `mermaid-cli` fixtures shipped in this repository.

## Current status: Rust CLI

- Implemented in `src/main.rs` with [clap](https://docs.rs/clap) for argument parsing and a lightweight native renderer.
- Parses a deterministic subset of Mermaid flowchart syntax (the `graph` directive with basic shapes and labels).
- Supports input from files or `stdin`, and writes to a file or `stdout`.
- Emits SVG today; PNG/PDF return an explicit "not yet supported" error while we build out the raster pipeline.
- Tested via `cargo test` using the upstream fixture `mermaid-cli/test-positive/flowchart1.mmd`.

### Caveats

- The current parser recognises a small slice of Mermaid flowchart syntax: directional `graph` headers (`TD`, `BT`, `LR`, `RL`), rectangular/stadium/circle/diamond node declarations, and labelled edges (`-->`, `-.->`).
- Layout is a simple linear pass (no automatic graph balancing or routing); diagrams are arranged along the primary axis declared by the header.
- Markdown sources and artefact generation from the original mermaid-cli are not implemented yet.
- PNG/PDF output remains a TODO for the local renderer; only SVGs are produced right now.

## Usage

### Build the CLI

```bash
cargo build --release
```

### Render a diagram from a file

```bash
./target/release/oxdraw \
	--input mermaid-cli/test-positive/flowchart1.mmd \
	--output ./flowchart1.svg \
	--output-format svg
```

### Render from stdin to stdout

```bash
cat mermaid-cli/test-positive/flowchart1.mmd | ./target/release/oxdraw --output -
```

### Launch the interactive editor

Build the Next.js bundle once (rerun after UI changes):

```bash
cd frontend
npm install
npm run build
cd ..
```

Then start the editor against a specific diagram:

```bash
./target/release/oxdraw --input mermaid-cli/test-positive/flowchart1.mmd --edit
```

This boots the Axum HTTP server on <http://127.0.0.1:5151>, serves the exported Next.js UI from `frontend/out/`, and streams all layout changes back into `flowchart1.oxdraw.json`. Stop the session with `Ctrl+C`. Set the `OXDRAW_WEB_DIST` environment variable if you keep the built assets elsewhere.

While the editor is running you will see the interactive canvas on the left and a live `.mmd` preview on the right. The source view auto-saves with a gentle debounce and immediately refreshes the canvas, so typing new nodes or changing the graph header updates the diagram in place. Likewise, manipulating the canvas (dragging, adjusting routes, or deleting via the **Delete selected** button/keyboard shortcut) writes the structural update back into the source text. Each node shape now uses a distinct pastel colour to make flowchart semantics easier to scan at a glance.

Pass `--serve-host 0.0.0.0` or `--serve-port 6000` to change the bind address when collaborating across devices.

Passing `--background-color transparent` asks for a transparent background (currently SVG-only). Omit `--output` to default to `<input>.svg` or `out.svg` when reading from stdin.
Use `--quiet` to suppress the default success message when writing to a file.

## Tests & TDD workflow

The CLI ships with integration coverage that mirrors the mermaid-cli examples:

```bash
cargo test
```

Add new fixtures under `mermaid-cli/test-positive/` and extend `tests/` as functionality grows. Keep the red/green cycle tight when extending the feature set.

## Roadmap

- Add PNG/PDF export by mirroring the mermaid-cli raster flow.
- Expand test coverage to every positive/negative fixture included with mermaid-cli.
- Implement Markdown-aware rendering and artefact management.
- Begin the React-based editor that can read/write oxdraw-enhanced Mermaid documents.
