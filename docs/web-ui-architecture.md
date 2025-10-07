# Oxdraw web editor contract

This document captures the interface between the Rust CLI and the upcoming React-based web editor so that interactive layout edits remain reproducible.

## Overview

Running `oxdraw serve --input <diagram.mmd>` starts an embedded HTTP server (default `http://127.0.0.1:5151`) that exposes the diagram definition, the auto-generated layout, and any stored manual overrides. The server speaks a small JSON API used by the web editor to read, tweak, and persist node positions and edge routes. SVG exports produced by the CLI automatically incorporate the latest overrides.

During a session the CLI watches two files:

- `<diagram>.mmd` – the raw Mermaid-compatible source that defines nodes, edges, labels, and flow direction.
- `<diagram>.oxdraw.json` – layout overrides captured by the web editor (node coordinates and optional edge waypoints). The file is created on-demand when the editor saves.

The CLI keeps both artefacts in sync:

1. Parse the `.mmd` file to build the semantic model used across both the server and the existing render command.
2. Load overrides (if any) and merge them with the auto layout that the CLI generates.
3. Persist any updates pushed by the web editor back to `<diagram>.oxdraw.json`.

## HTTP API

All endpoints live under the `/api` prefix and currently exchange JSON payloads encoded as UTF-8.

### `GET /api/diagram`

Returns the latest diagram model, including auto layout suggestions and any saved overrides.

**Response 200**

```json
{
  "sourcePath": "/absolute/path/to/diagram.mmd",
  "background": "white",
  "nodes": [
    {
      "id": "A",
      "label": "Start",
      "shape": "rectangle",
      "autoPosition": { "x": 240.0, "y": 120.0 },
      "position": { "x": 200.0, "y": 150.0 }
    }
  ],
  "edges": [
    {
      "id": "A-->B",
      "from": "A",
      "to": "B",
      "label": "Next",
      "kind": "solid",
      "autoPoints": [
        { "x": 240.0, "y": 180.0 },
        { "x": 240.0, "y": 280.0 }
      ],
      "points": [
        { "x": 220.0, "y": 210.0 },
        { "x": 260.0, "y": 260.0 }
      ]
    }
  ]
}
```

- `position` / `points` represent the persisted overrides. They may be omitted when no manual edit exists.
- `autoPosition` / `autoPoints` provide the CLI’s deterministic fallback so the editor can display the baseline layout.

### `PUT /api/diagram/layout`

Stores layout overrides coming from the web editor. The request body only needs to include the elements that changed.

**Request body**

```json
{
  "nodes": {
    "A": { "x": 210.0, "y": 150.0 },
    "B": { "x": 210.0, "y": 310.0 }
  },
  "edges": {
    "A-->B": {
      "points": [
        { "x": 210.0, "y": 200.0 },
        { "x": 270.0, "y": 260.0 }
      ]
    }
  }
}
```

**Response 204** – The overrides were persisted successfully.

### `GET /api/diagram/svg`

Returns the current SVG render (with overrides applied) as `image/svg+xml`. Editors can use this to preview the final visualisation or diff changes as they drag elements around.

## Edge identifiers

Edges are keyed by `<from>--> <to>` (including the arrow token and single space). Dashed edges use `-.->`. The identifier is guaranteed to match the raw syntax emitted by the parser, making it stable between refreshes.

## Layout persistence format

`<diagram>.oxdraw.json` mirrors the payload used by `PUT /api/diagram/layout` with the same `nodes`/`edges` maps. The CLI writes it atomically on save to avoid partial writes.

## Future considerations

- Broadcast updates via Server Sent Events (`GET /api/events`) so multiple editor windows stay in sync.
- Allow editing the raw diagram definition from the browser (via `PUT /api/diagram/source`).
- Persist per-edge label offsets for finer control.
- Introduce authentication flags for remote collaboration sessions.
