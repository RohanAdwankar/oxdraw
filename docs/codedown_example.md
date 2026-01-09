# Oxdraw Editor Frontend Architecture

This document outlines the architecture, key components, and data flow of the Oxdraw Editor frontend application. The editor is built with Next.js and React, providing an interactive interface for creating and managing Oxdraw diagrams, along with integrated code and markdown viewing capabilities.

## Overall Architecture

The Oxdraw Editor frontend leverages the Next.js framework for server-side rendering (SSR) and client-side navigation, providing a performant and modern web application. It follows a component-based architecture using React, with a central application component (`Home`) managing the global state and orchestrating interactions between various sub-components and a backend API. The application is designed to be highly interactive, supporting real-time diagram manipulation, source code editing, and dynamic UI adjustments.

Key architectural characteristics include:

*   **Client-Side Rich Application**: Heavy reliance on React hooks for state management and UI interactions, making it a client-intensive application.
*   **API-Driven**: All data persistence and complex diagram processing (e.g., parsing Oxdraw source, generating diagram data) are handled via a backend API (inferred from `../lib/api`).
*   **Modular Components**: Separation of concerns into dedicated components like `DiagramCanvas`, `CodePanel`, and `MarkdownViewer`.
*   **Responsive and Customizable UI**: Features like resizable panels and theme switching enhance user experience.

## Key Components

### 1. Next.js Framework Setup

The Next.js framework forms the foundation of the application, handling routing, layout, and build processes.

*   **Root Layout (`layout.tsx`)**:
    This component defines the base HTML structure for the application. It includes global metadata, imports necessary fonts, and applies global styles. The layout wraps the entire application, ensuring consistent styling and structure across pages.
    ```typescript
    import type { Metadata } from "next";
    import { Geist, Geist_Mono } from "next/font/google";
    import "./globals.css";
    const geistSans = Geist({
      variable: "--font-geist-sans",
      subsets: ["latin"],
    });
    const geistMono = Geist_Mono({
      variable: "--font-geist-mono",
      subsets: ["latin"],
    });
    export const metadata: Metadata = {
      title: "Oxdraw Editor",
      description: "Interactive Oxdraw diagram editor",
    };
    export default function RootLayout({
      children,
    }: Readonly<{
      children: React.ReactNode;
    }>) {
      return (
        <html lang="en">
          <body
            className={`${geistSans.variable} ${geistMono.variable} antialiased`}
          >
            {children}
          </body>
        </html>
      );
    }
    ```

*   **Main Page (`page.tsx`)**:
    This is the primary client-side component for the Oxdraw editor. Marked with `'use client'`, it utilizes various React hooks for managing a complex interactive state.
    ```typescript
    'use client';
    import { ... } from "react";
    import DiagramCanvas from "../components/DiagramCanvas";
    import MarkdownViewer from "../components/MarkdownViewer";
    import CodePanel from "../components/CodePanel";
    import { ... } from "../lib/api";
    import { ... } from "../lib/types";
    export default function Home() { /* ... */ }
    ```

### 2. Core Application Component (`Home` in `page.tsx`)

The `Home` component acts as the central orchestrator for the entire editor. It manages the application's state, handles user interactions, and coordinates data flow between child components and the backend API.

*   **State Management**:
    The component uses numerous `useState` hooks to manage the application's interactive state, including diagram data, source code, UI selections, and layout settings.
    ```typescript
    const [diagram, setDiagram] = useState<DiagramData | null>(null);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);
    const [saving, setSaving] = useState(false);
    const [source, setSource] = useState("");
    const [sourceDraft, setSourceDraft] = useState("");
    const [sourceSaving, setSourceSaving] = useState(false);
    const [sourceError, setSourceError] = useState<string | null>(null);
    const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
    const [selectedEdgeId, setSelectedEdgeId] = useState<string | null>(null);
    const [imagePaddingValue, setImagePaddingValue] = useState<string>("");
    const [dragging, setDragging] = useState(false);
    const [codeMapMapping, setCodeMapMapping] = useState<CodeMapMapping | null>(null);
    const [codeMapMode, setCodeMapMode] = useState(false);
    const [codedownMode, setCodedownMode] = useState(false);
    const [markdownContent, setMarkdownContent] = useState<string>("");
    const [selectedFile, setSelectedFile] = useState<{ path: string; content: string } | null>(null);
    const [highlightedLines, setHighlightedLines] = useState<{ start: number; end: number } | null>(null);
    const [theme, setTheme] = useState<"light" | "dark">("light");
    const [leftPanelWidth, setLeftPanelWidth] = useState(280);
    const [isLeftPanelResizing, setIsLeftPanelResizing] = useState(false);
    const [isLeftPanelCollapsed, setIsLeftPanelCollapsed] = useState(false);
    const [rightPanelWidth, setRightPanelWidth] = useState(380);
    const [isRightPanelResizing, setIsRightPanelResizing] = useState(false);
    const [isRightPanelCollapsed, setIsRightPanelCollapsed] = useState(false);
    ```

*   **References (`useRef`)**:
    `useRef` hooks are used for persisting mutable values across renders without causing re-renders, such as timers for auto-saving and references to DOM elements.
    ```typescript
    const saveTimer = useRef<number | null>(null);
    const lastSubmittedSource = useRef<string | null>(null);
    const nodeImageInputRef = useRef<HTMLInputElement | null>(null);
    const imagePaddingValueRef = useRef(imagePaddingValue);
    ```

*   **UI Callbacks & Effects**:
    `useCallback` and `useEffect` are essential for optimizing performance and managing side effects, such as theme changes and dynamic panel resizing.
    ```typescript
    useEffect(() => {
      document.body.setAttribute("data-theme", theme);
    }, [theme]);
    const toggleTheme = useCallback(() => {
      setTheme((prev) => (prev === "light" ? "dark" : "light"));
    }, []);
    const startLeftPanelResizing = useCallback((e: React.MouseEvent) => {
      e.preventDefault();
      setIsLeftPanelResizing(true);
    }, []);
    const startRightPanelResizing = useCallback((e: React.MouseEvent) => {
      e.preventDefault();
      setIsRightPanelResizing(true);
    }, []);
    useEffect(() => {
      if (!isLeftPanelResizing && !isRightPanelResizing) return;
      const handleMouseMove = (e: MouseEvent) => {
        if (isLeftPanelResizing) {
          setLeftPanelWidth(Math.max(200, Math.min(e.clientX, 600)));
        } else if (isRightPanelResizing) {
          const newWidth = document.body.clientWidth - e.clientX;
          setRightPanelWidth(Math.max(300, Math.min(newWidth, 800)));
    ```

### 3. API Integration (`../lib/api.ts`)

The application interacts with a backend API for all data operations. These functions encapsulate the network requests for fetching and modifying diagram data.

*   **Diagram Management API**:
    ```typescript
    import { deleteEdge, deleteNode, fetchDiagram, updateLayout, updateNodeImage, updateSource, updateStyle, fetchCodeMapMapping, fetchCodeMapFile, openInEditor } from "../lib/api";
    ```
    These imports indicate API calls for:
    *   `fetchDiagram`: Retrieving the entire diagram structure.
    *   `updateSource`: Persisting changes to the Oxdraw source code.
    *   `updateLayout`, `updateNodeImage`, `updateStyle`: Modifying visual properties of diagram elements.
    *   `deleteNode`, `deleteEdge`: Removing elements from the diagram.
    *   `fetchCodeMapMapping`, `fetchCodeMapFile`, `openInEditor`: Functions related to code mapping and editor integration.

### 4. External UI Components (Inferred from imports)

The main `Home` component orchestrates these specialized child components, which are responsible for rendering specific parts of the editor's UI.

*   `DiagramCanvas`: Likely responsible for rendering the visual representation of the Oxdraw diagram and handling user interactions (e.g., drag-and-drop, selection).
    ```typescript
    import DiagramCanvas from "../components/DiagramCanvas";
    ```
*   `CodePanel`: Provides an interface for users to view and edit the raw Oxdraw source code. It also supports displaying mapped code lines.
    ```typescript
    import CodePanel from "../components/CodePanel";
    ```
*   `MarkdownViewer`: Renders and displays markdown content, possibly related to the diagram or linked code.
    ```typescript
    import MarkdownViewer from "../components/MarkdownViewer";
    ```

### 5. Utility Functions

The `page.tsx` file includes several utility functions for data manipulation, validation, and image processing.

*   **Diagram Override Check**:
    `hasOverrides`: Determines if the diagram contains manually overridden positions or points, which might affect auto-layout behavior.
    ```typescript
    function hasOverrides(diagram: DiagramData | null): boolean { /* ... */ }
    ```
*   **Image Processing**:
    Functions like `formatByteSize`, `blobToBase64`, `loadImageFromBlob`, `resizeImageToLimit`, and `ensureImageWithinLimit` handle client-side image operations, such as resizing and conversion, before uploading images for nodes.
    ```typescript
    const formatByteSize = (bytes: number): string => { /* ... */ };
    const blobToBase64 = (blob: Blob): Promise<string> => /* ... */ ;
    const loadImageFromBlob = (blob: Blob): Promise<HTMLImageElement> => /* ... */ ;
    const resizeImageToLimit = async ( /* ... */ ) => { /* ... */ };
    const ensureImageWithinLimit = async ( /* ... */ ) => { /* ... */ };
    ```
*   **Styling & Validation Helpers**:
    `formatPaddingValue`, `normalizePadding`, `resolveColor`, `normalizeColorInput`, and `HEX_COLOR_RE` provide consistent handling and validation for style-related inputs.
    ```typescript
    const HEX_COLOR_RE = /^#([0-9a-f]{6})$/i;
    const formatPaddingValue = (value: number): string => { /* ... */ };
    const normalizePadding = (value: number): number => { /* ... */ };
    const resolveColor = (value: string | null | undefined, fallback: string): string => { /* ... */ };
    const normalizeColorInput = (value: string): string => value.trim().toLowerCase();
    ```

### 6. Data Models (`../lib/types.ts`)

The application relies on a set of TypeScript interfaces and types (imported from `../lib/types.ts`) to define the structure of the data it handles. These include `DiagramData` for the entire diagram, `NodeData` for individual nodes, `EdgeData` for connections, and `CodeMapMapping` for linking diagrams to code.

```typescript
import {
  DiagramData,
  EdgeArrowDirection,
  EdgeKind,
  LayoutUpdate,
  EdgeStyleUpdate,
  NodeStyleUpdate,
  NodeData,
  Point,
  CodeMapMapping,
  CodeLocation,
} from "../lib/types";
```

## Data Flow

### 1. Application Initialization

Upon loading, the `RootLayout` component renders the basic page structure.
The `Home` component initializes its extensive state variables.
An initial API call to `fetchDiagram` (not explicitly shown but implied by the `diagram` state) retrieves the current Oxdraw diagram data and source code from the backend. This data populates the `diagram` and `source` states, which are then passed to `DiagramCanvas` and `CodePanel` for rendering.

### 2. Diagram Editing

*   **Visual Interactions**: When a user interacts with the `DiagramCanvas` (e.g., moving a node, resizing, creating/deleting edges, changing styles), the `DiagramCanvas` dispatches events or calls callbacks provided by the `Home` component.
*   **State Update**: The `Home` component updates its internal `diagram` state based on these interactions and updates `selectedNodeId` or `selectedEdgeId`.
*   **API Persistence**: For changes requiring persistence, the `Home` component invokes relevant API functions from `../lib/api`, such as `updateLayout`, `updateStyle`, `deleteNode`, or `deleteEdge`. These calls send the updated data to the backend.

### 3. Source Code Editing

*   **Drafting Changes**: Users can directly edit the Oxdraw source code in the `CodePanel`. Changes are initially stored in the `sourceDraft` state.
*   **Auto-saving/Manual Save**: A `saveTimer` (managed via `useRef`) and the `lastSubmittedSource` ref facilitate an auto-save mechanism. When the `sourceDraft` stabilizes or a manual save is triggered, the `updateSource` API call sends the `sourceDraft` content to the backend.
*   **Diagram Re-render**: Upon successful update, the `source` state is updated, which may trigger the backend to re-process the diagram and send updated `DiagramData` back to the frontend, leading to a re-render of the `DiagramCanvas`.

### 4. Image Handling

*   **User Selection**: When a user selects an image file (e.g., to embed in a node via `nodeImageInputRef`), the file input change event is handled.
*   **Client-side Processing**: The selected image `File` undergoes client-side processing by utility functions like `ensureImageWithinLimit` which may call `resizeImageToLimit`, `loadImageFromBlob`, and `blobToBase64`. This ensures the image adheres to size limits and is converted into a Base64 string.
*   **API Upload**: The Base64 encoded image data is then sent to the backend via the `updateNodeImage` API call.

### 5. Code Mapping & Markdown Display

*   **Mode Switching**: The `codeMapMode` and `codedownMode` states control the visibility and functionality of the code mapping and markdown viewing features.
*   **Fetching Data**: Based on user interaction (e.g., selecting a diagram element or activating a mode), `fetchCodeMapMapping` or `fetchCodeMapFile` API calls retrieve relevant code locations, file contents, or markdown content.
*   **UI Update**: The fetched data updates states like `codeMapMapping`, `selectedFile`, `highlightedLines`, and `markdownContent`, causing the `CodePanel` and `MarkdownViewer` to display the information accordingly.
*   **External Editor**: The `openInEditor` API call allows the user to directly open the referenced code file in an external editor, bridging the web application with local development tools.

### 6. UI Interactions (Theming & Resizing)

*   **Theme Toggle**: The `toggleTheme` function updates the `theme` state. This change is reflected by a `useEffect` hook that sets the `data-theme` attribute on the `document.body`, which in turn triggers CSS rules to apply light or dark mode styles.
*   **Panel Resizing**: When a user initiates resizing by dragging (`startLeftPanelResizing`, `startRightPanelResizing`), global mouse event listeners (`handleMouseMove`, `handleMouseUp`) are activated. These listeners update `leftPanelWidth` or `rightPanelWidth` states based on mouse movement. The updated state values dynamically adjust the width of the `CodePanel` and other adjacent panels, providing a flexible layout.

<!-- OXDRAW MAPPING
{
  "nodes": {
    "line_18": {
      "end_line": 3,
      "file": "layout.tsx",
      "start_line": 1,
      "symbol": null
    },
    "line_19": {
      "end_line": 8,
      "file": "layout.tsx",
      "start_line": 5,
      "symbol": null
    },
    "line_20": {
      "end_line": 13,
      "file": "layout.tsx",
      "start_line": 10,
      "symbol": null
    },
    "line_21": {
      "end_line": 18,
      "file": "layout.tsx",
      "start_line": 15,
      "symbol": null
    },
    "line_22": {
      "end_line": 31,
      "file": "layout.tsx",
      "start_line": 20,
      "symbol": null
    },
    "line_28": {
      "end_line": 14,
      "file": "page.tsx",
      "start_line": 1,
      "symbol": null
    },
    "line_29": {
      "end_line": 17,
      "file": "page.tsx",
      "start_line": 15,
      "symbol": null
    },
    "line_30": {
      "end_line": 27,
      "file": "page.tsx",
      "start_line": 18,
      "symbol": null
    },
    "line_31": {
      "end_line": 29,
      "file": "page.tsx",
      "start_line": 29,
      "symbol": null
    },
    "line_38": {
      "end_line": 128,
      "file": "page.tsx",
      "start_line": 128,
      "symbol": "Home"
    },
    "line_43": {
      "end_line": 160,
      "file": "page.tsx",
      "start_line": 129,
      "symbol": null
    },
    "line_49": {
      "end_line": 165,
      "file": "page.tsx",
      "start_line": 162,
      "symbol": null
    },
    "line_55": {
      "end_line": 169,
      "file": "page.tsx",
      "start_line": 167,
      "symbol": "useEffect"
    },
    "line_56": {
      "end_line": 173,
      "file": "page.tsx",
      "start_line": 171,
      "symbol": "toggleTheme"
    },
    "line_57": {
      "end_line": 178,
      "file": "page.tsx",
      "start_line": 175,
      "symbol": "startLeftPanelResizing"
    },
    "line_58": {
      "end_line": 183,
      "file": "page.tsx",
      "start_line": 180,
      "symbol": "startRightPanelResizing"
    },
    "line_59": {
      "end_line": 196,
      "file": "page.tsx",
      "start_line": 185,
      "symbol": "useEffect"
    },
    "line_65": {
      "end_line": 27,
      "file": "page.tsx",
      "start_line": 18,
      "symbol": null
    },
    "line_69": {
      "end_line": 37,
      "file": "page.tsx",
      "start_line": 31,
      "symbol": "hasOverrides"
    },
    "line_73": {
      "end_line": 72,
      "file": "page.tsx",
      "start_line": 64,
      "symbol": "formatByteSize"
    },
    "line_74": {
      "end_line": 89,
      "file": "page.tsx",
      "start_line": 74,
      "symbol": "blobToBase64"
    },
    "line_75": {
      "end_line": 103,
      "file": "page.tsx",
      "start_line": 91,
      "symbol": "loadImageFromBlob"
    },
    "line_76": {
      "end_line": 156,
      "file": "page.tsx",
      "start_line": 105,
      "symbol": "resizeImageToLimit"
    },
    "line_77": {
      "end_line": 196,
      "file": "page.tsx",
      "start_line": 158,
      "symbol": "ensureImageWithinLimit"
    },
    "line_81": {
      "end_line": 56,
      "file": "page.tsx",
      "start_line": 56,
      "symbol": "HEX_COLOR_RE"
    },
    "line_82": {
      "end_line": 204,
      "file": "page.tsx",
      "start_line": 198,
      "symbol": "formatPaddingValue"
    },
    "line_83": {
      "end_line": 211,
      "file": "page.tsx",
      "start_line": 206,
      "symbol": "normalizePadding"
    },
    "line_84": {
      "end_line": 219,
      "file": "page.tsx",
      "start_line": 213,
      "symbol": "resolveColor"
    },
    "line_85": {
      "end_line": 221,
      "file": "page.tsx",
      "start_line": 221,
      "symbol": "normalizeColorInput"
    },
    "line_91": {
      "end_line": 40,
      "file": "page.tsx",
      "start_line": 29,
      "symbol": null
    }
  }
}
-->
<!-- OXDRAW META path: commit:c7695a4c16065f8fa291ad08b8819aa299ca1059 diff_hash:13646096770106105413 -->
