
https://github.com/user-attachments/assets/de5222bb-9b65-43cf-a35b-5613d06343e8

## Overview

The goal of `oxdraw` is to make it easy to create and maintain high-quality diagrams using a declarative and reproducible syntax.
Charts are written in [Mermaid](https://mermaid.js.org/) syntax, while a web interface allows users to fine-tune positions connector paths, colors, and other styling components. Whenever a diagram is tweaked visually, the structural changes are persisted back to the source file as declarative code so that everything remains deterministic and versionable.
The changes are saved as comments in the mermaid file so it remains compatible with other Mermaid tools.
The repo is composed of the Rust CLI to compile `.mmd` files into images and the React based web interface to editing the files.

## Vision

The reason I started this project was I used Mermaid a lot in the past when making architecture diagrams or trying to understand large codebases through having AI tools generate .mmd files to visualize them. However what typically happened was since these diagrams couldn't be edited minutely for example cleaning up joints and chart organization, I would have to move over the diagrams I started to things like Lucidchart. So the big picture goal of this project is to unite the benefits of code generated diagramming like Mermaid with the customizability of diagram software like Lucidchart.

## Usage

### Install fom Cargo

```bash
cargo install oxdraw
```

### Render a diagram from a file

```bash
oxdraw --input flow.mmd  
```

### Launch the interactive editor

```bash
oxdraw --input flow.mmd --edit
```

### Have AI generate a codemap for the repo you're in 
#### This will also launch the interactive viewer mapping the nodes to files in the repo 

```bash
oxdraw --code-map ./
```

## Features

### CLI Flags

| Flag | Description |
| --- | --- |
| `-i, --input <PATH>` | Read a Mermaid source file; pass `-` to consume stdin instead. |
| `-o, --output <PATH>` | Write the rendered asset to a specific path; pass `-` to stream SVG to stdout. Defaults to `<input>.svg` (or `<input>.<format>` if an explicit format is chosen) and `out.svg` when reading from stdin. |
| `--png` | Shorthand for `--output-format png` |
| `--scale <FACTOR>` | Scale multiplier for PNG rasterization (default `10.0`); values must be greater than zero. Ignored for SVG output. |
| `--edit` | Launch the interactive editor pointing at the supplied diagram instead of emitting an asset once. |
| `--serve-host <ADDR>` | Override the bind address used while `--edit` is active (default `127.0.0.1`). |
| `--serve-port <PORT>` | Override the HTTP port while `--edit` is active (default `5151`). |
| `-b, --background-color <COLOR>` | Background fill passed to the renderer (currently SVG only). Applies to both one-off renders and the editor preview. |
| `-q, --quiet` | Suppress informational stdout such as the success message after rendering to disk. |
| `-n, --new` | Create new mermaid file and serves for editing. |
| `--code-map <PATH>` | Generate a code map from the given codebase path. |
| `--api-key <KEY>` | API Key for the LLM (optional, defaults to environment variable if not set). |
| `--model <MODEL>` | Model to use for code map generation. |
| `--api-url <URL>` | API URL for the LLM. |
| `--regen` | Force regeneration of the code map even if a cache exists. |
| `--prompt <PROMPT>` | Custom prompt to append to the LLM instructions. |

### Frontend Features

| Control | What it does |
| --- | --- |
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
- The source panel mirrors the Mermaid file, auto-saves after short idle periods, and surfaces pending/saving/error states alongside the current selection.
- Status text in the top toolbar signals loading, saving, and the currently edited file path.

### The Diagram Algorithm

https://github.com/user-attachments/assets/4430147a-83d8-4d83-aca6-7beec197c0e3

The path drawing algorithm is fun because there is a lot of ambiguity with what optimal behavior could be.
Some prefer smooth lines because there is less total line but I prefer strong edges to make the diagram a bit more clear. 
Some prefer no overlapping lines but I sometimes prefer an overlap rather than letting the lines get super long and string out of the diagram very far.
This is an example of using the delete key to remove one relationship and then using the arrow keys to move around one the nodes and seeing how the algorithm recomputes the positioning.
There's definitely some improvements to be made to this algorithm so I imagine this will keep getting better :)

## Community
If you do end up using oxdraw, please let me know! You can open issues or discussion posts on GitHub or reach out to me on one of the socials from my Github profile. I would love to hear how you are using it, any feedback you have, and/or add your project to this section!

Check out these projects using oxdraw:
- [Typst-Oxdraw](https://github.com/hongjr03/typst-oxdraw/) is a repo that integrates oxdraw diagrams into Typst documents. 
