
https://github.com/user-attachments/assets/4967abab-794e-4449-9b7c-a4d8fa1f22cd

## Vision

Oxdraw is a tool for declarative diagram authoring. Charts are written in [Mermaid](https://mermaid.js.org/) syntax, while a web interface allows users to fine-tune positions connector paths, colors, and other styling components. Whenever a diagram is tweaked visually, the structural changes are persisted back to the source file as declarative code so that everything remains deterministic and versionable.
The changes are saved as comments in the mermaid file so it remains compatible with other Mermaid tools.

The long-term architecture comprises two layers:

- **Rust CLI** – compiles `.mmd` sources into reproducible assets (`.svg`, `.png`, later `.pdf`).
- **React web interface** – provides direct-manipulation editing (dragging nodes, reshaping edges) and writes compatible oxdraw metadata alongside the Mermaid definition.

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

While the editor is running you will see the interactive canvas on the left and a live `.mmd` preview on the right. The source view auto-saves with a gentle debounce and immediately refreshes the canvas, so typing new nodes or changing the graph header updates the diagram in place. Likewise, manipulating the canvas (dragging, adjusting routes, or deleting via the **Delete selected** button/keyboard shortcut) writes the structural update back into the source text. Each node shape now uses a distinct pastel colour to make flowchart semantics easier to scan at a glance.

Pass `--serve-host 0.0.0.0` or `--serve-port 6000` to change the bind address when collaborating across devices.

Passing `--background-color transparent` asks for a transparent background (currently SVG-only). Omit `--output` to default to `<input>.svg` or `out.svg` when reading from stdin.
Use `--quiet` to suppress the default success message when writing to a file.

## Next Steps
