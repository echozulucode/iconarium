# SVG Library Browser — Updated Phased Implementation Plan

## 1. Product Goal

Build a fast, modern Windows desktop application for browsing, searching, previewing, selecting, copying, and reusing SVG assets.

The application will use:

- Rust
- Tauri
- React
- TypeScript
- Tailwind CSS
- SQLite
- Native Windows integration where required

The application should work well for:

- Icon libraries
- Engineering diagrams
- Architecture diagrams
- UI assets
- Reference graphics
- Moderate-sized technical SVGs
- Large directory trees containing thousands or tens of thousands of SVG files

The primary design objective is that the application should feel immediate even when the underlying directory contains a large number of files.

The UI must never wait for the entire directory to be scanned, parsed, hashed, indexed, or rendered before becoming usable.

---

# 2. Confirmed Product Decisions

The following behaviors are now part of the intended design.

## 2.1 Copy behavior

`Ctrl+C` will be context-sensitive.

If an SVG region is selected:

```text
Ctrl+C
→ Copy selected region as SVG
```

If no region is selected:

```text
Ctrl+C
→ Copy entire selected SVG
```

Additional explicit commands will remain available:

```text
Copy SVG
Copy PNG
Copy Full Path
Copy Filename
Copy Selected Region
```

---

## 2.2 Region selection coordinates

Selection will be represented internally using SVG coordinates rather than screen coordinates.

For example:

```text
SVG coordinate space:

x = 1240
y = 680
width = 850
height = 430
```

Screen zoom and pan will only affect presentation.

This ensures that selections remain stable when:

- Zooming
- Panning
- Resizing the viewer
- Switching display scale
- Exporting/copying the selection

---

## 2.3 Cropped SVG normalization

When copying or exporting a selected region, the resulting SVG will use a normalized canvas.

For example, a source selection:

```text
x = 1200
y = 800
width = 900
height = 600
```

will produce a new document whose logical canvas begins at:

```text
0,0
```

rather than retaining the original document offset.

Conceptually:

```text
Original

viewBox="0 0 5000 3000"

Selected

1200,800 → 2100,1400

Result

viewBox="0 0 900 600"
```

The source content will be translated accordingly.

This should make pasted SVGs behave more naturally in PowerPoint, Visio, and draw.io.

---

## 2.4 Transparency

Transparency will be preserved by default.

Copy/export commands should provide:

```text
Copy as SVG
    transparent

Copy as PNG
    transparent

Copy as PNG with White Background
```

A configurable background should affect the viewer only unless the user explicitly requests that background during export.

---

## 2.5 SVG complexity

The application does not need to support arbitrarily massive CAD-like SVG documents.

It should instead establish reasonable limits and fail gracefully.

Suggested initial guardrails:

```text
Recommended SVG size:
< 10 MB

Soft warning:
10–25 MB

Default hard processing limit:
25 MB

Maximum parsed element count:
configurable, perhaps 50,000–100,000

Maximum embedded raster data:
configurable
```

These should be configuration values rather than constants baked throughout the implementation.

An oversized SVG should still appear in the catalog but may show:

```text
Preview unavailable
File exceeds rendering limit
```

The user should still be able to:

- Copy its path
- Reveal it in Explorer
- Open it externally

---

## 2.6 Multi-selection

The gallery will support selecting multiple SVG files.

Expected desktop behavior:

```text
Click
    Select one

Ctrl+Click
    Toggle item

Shift+Click
    Select range

Ctrl+A
    Select all current search results
```

Operations may then apply to multiple files.

Initially:

```text
Copy paths
Drag files
Reveal files
Open containing folders
```

Later:

```text
Copy multiple images
Create contact sheet
Export collection
Add to collection/favorites
```

Native multi-file drag should be supported so several SVGs can be dragged into another application simultaneously when the destination supports it.

---

## 2.7 Search inside SVG content

Search will eventually index more than filenames and paths.

Searchable fields should include:

```text
filename
relative path
directory names
<title>
<desc>
<text>
element IDs
class names
possibly metadata elements
```

The most important internal content is `<text>`.

This enables searches such as:

```text
ethernet
```

to find:

```text
ethernet-switch.svg

network/ethernet.svg

system-architecture.svg
    because the diagram contains a text label "Ethernet"
```

Filename/path indexing will ship before full SVG text indexing.

---

# 3. Target Architecture

```text
┌───────────────────────────────────────────────────────────────┐
│                    Tauri Desktop Application                  │
│                                                               │
│  React / TypeScript / Tailwind                               │
│  ┌─────────────────────────────────────────────────────────┐  │
│  │ Search                                                  │  │
│  │ Filters                                                 │  │
│  │ Virtualized SVG Gallery                                │  │
│  │ Multi-selection                                        │  │
│  │ Context menus                                           │  │
│  │ Detail viewer                                           │  │
│  │ Region selection                                        │  │
│  │ Pan / zoom / minimap                                    │  │
│  └────────────────────────┬────────────────────────────────┘  │
│                           │                                   │
│                     Tauri commands/events                     │
│                           │                                   │
│  ┌────────────────────────▼────────────────────────────────┐  │
│  │ Rust Backend                                            │  │
│  │                                                        │  │
│  │ Library Scanner                                        │  │
│  │ Persistent Index                                       │  │
│  │ SVG Metadata Parser                                    │  │
│  │ SVG Text Extractor                                     │  │
│  │ Search Engine                                          │  │
│  │ Thumbnail Queue                                        │  │
│  │ Thumbnail Renderer                                     │  │
│  │ Filesystem Watcher                                     │  │
│  │ SVG Crop / Region Export                               │  │
│  │ Clipboard Integration                                  │  │
│  │ Native Drag Integration                                │  │
│  └─────────┬──────────────────┬────────────────────┬───────┘  │
│            │                  │                    │          │
│         SQLite          Thumbnail Cache       Windows APIs    │
└────────────┼──────────────────┼────────────────────┼──────────┘
             │
        SVG libraries
```

---

# 4. Major Architectural Principle

Rust should own:

```text
filesystem traversal
parsing
hashing
indexing
search
preview rendering
cache management
SVG extraction/cropping
clipboard integration
native drag-and-drop
filesystem monitoring
```

React should own:

```text
visual presentation
virtualized scrolling
interaction
selection
viewer state
search input
keyboard handling
menus
zoom/pan controls
```

The React application should never recursively traverse directories itself.

It should also avoid parsing thousands of SVG files inside the WebView.

---

# 5. Phase 0 — Technical Risk Spikes

Perform these before building the complete application.

## 5.1 Drag SVG into Office

Create a minimal Tauri prototype containing one SVG.

Validate drag-out into:

```text
PowerPoint
Visio
draw.io desktop
draw.io browser
Windows Explorer
```

Determine whether passing the SVG as an ordinary native file drag preserves vector content.

Also test multiple files.

```text
Select:
icon1.svg
icon2.svg
icon3.svg

Drag

→ receiving application
```

This validates future gallery multi-select.

---

## 5.2 SVG clipboard

Test native clipboard publishing of:

```text
SVG
PNG
text
```

Validate pasting into:

```text
PowerPoint
Visio
draw.io
Paint
Word
```

Verify that PowerPoint and Visio receive vector content rather than silently choosing the PNG fallback.

---

## 5.3 Region-copy prototype

Build a tiny viewer containing one SVG.

Allow:

```text
zoom
pan
rectangle selection
Copy Selection
```

Generate a temporary cropped SVG.

Validate:

```text
Application
→ select region
→ Ctrl+C
→ PowerPoint
```

Confirm:

- Region only is pasted.
- It remains vector.
- Transparency works.
- Canvas is normalized.
- No outside content appears.

This is important enough to derisk early.

---

## 5.4 Gallery performance spike

Generate test libraries containing:

```text
1,000 SVGs
10,000 SVGs
50,000 SVGs
```

Validate:

- Progressive discovery
- Grid virtualization
- Scrolling during indexing
- Search while indexing
- Thumbnail generation while scrolling

---

# 6. Phase 1 — Application Foundation

Build:

```text
Tauri
Rust
React
TypeScript
Vite
Tailwind
```

Establish:

```text
logging
error handling
configuration
database migrations
Tauri command organization
application state
frontend state management
```

Suggested frontend state management:

```text
Zustand
```

or lightweight React stores.

Avoid complex Redux-style architecture unless the application grows considerably.

---

# 7. Phase 2 — Directory Selection and Library Model

Provide:

```text
Select Folder
Recent Folders
Reopen Last Folder
```

The selected directory becomes a library root.

Library record:

```text
Library {
    id
    path
    display_name
    created_at
    last_opened
    last_scan
}
```

Initially support one active library.

Design the schema so multiple library roots can be added later.

---

# 8. Phase 3 — Progressive Recursive Discovery

Directory scanning must be asynchronous.

Do not:

```text
scan everything
then return results
```

Instead:

```text
Select directory
      ↓
begin recursive traversal
      ↓
find SVGs
      ↓
send batches to UI
      ↓
continue traversal
```

Suggested batch size:

```text
100–500 records
```

An initial asset record requires only:

```text
path
filename
relative path
file size
mtime
asset ID
```

Everything else happens later.

---

# 9. Phase 4 — Background Processing Pipeline

Use bounded work queues.

```text
                 ┌→ metadata parser
Discovery queue ─┼→ SVG text extractor
                 ├→ thumbnail renderer
                 └→ content hash worker
```

Suggested priority classes:

```text
P0
Currently visible assets

P1
Assets just outside viewport

P2
Current search results

P3
Remaining library
```

Scrolling should reprioritize work dynamically.

---

# 10. Phase 5 — Persistent SQLite Index

SQLite should make subsequent launches nearly immediate.

Suggested tables:

```text
libraries

assets

svg_metadata

svg_text

thumbnails

settings
```

Example asset fields:

```text
id
library_id
relative_path
filename
file_size
mtime_ns
fast_fingerprint
content_hash
width
height
view_box
element_count
processing_state
parse_error
```

---

# 11. Phase 6 — Hashing and Change Detection

Do not hash every file before showing it.

Use staged change detection.

First:

```text
relative path
size
mtime
```

Then calculate a BLAKE3 hash when:

```text
new file
changed file
rename suspected
duplicate detection requested
cache validation requires it
```

This keeps initial scans fast.

---

# 12. Phase 7 — SVG Metadata and Content Extraction

Parse SVGs in Rust.

Extract:

```text
width
height
viewBox
<title>
<desc>
<text>
IDs
classes
element count
```

Store searchable content separately.

Example:

```text
svg_text

asset_id
title_text
description_text
visible_text
identifier_text
normalized_search_text
```

Normalize:

```text
case
whitespace
XML entities
repeated text
```

Potentially place a maximum amount of extracted searchable text per SVG.

For example:

```text
1 MB maximum extracted text
```

This avoids pathological documents.

---

# 13. Phase 8 — SVG Complexity Guardrails

Before expensive processing, calculate basic complexity information.

Possible limits:

```text
25 MB source file

100,000 XML/SVG nodes

50 MB decoded embedded raster images

10,000 characters per individual text node

1 MB searchable extracted text
```

If limits are exceeded:

```text
Asset state:
LIMIT_EXCEEDED
```

Display the asset but avoid expensive rendering.

Configuration should eventually allow advanced users to raise limits.

---

# 14. Phase 9 — Thumbnail Pipeline

Use Rust-based SVG rendering.

Pipeline:

```text
SVG
 ↓
parse
 ↓
render
 ↓
256×256 PNG/WebP thumbnail
 ↓
persistent cache
```

Start with one standard thumbnail resolution.

For example:

```text
256×256
```

Potential future sizes:

```text
128
256
512
```

Cache key should contain:

```text
asset hash/fingerprint
renderer version
thumbnail dimensions
```

---

# 15. Phase 10 — Virtualized Gallery

The UI should never render thousands of React cards.

Use a virtualized grid.

```text
50,000 results

↓ virtualization

~50–150 mounted cards
```

The grid should dynamically adapt to window width.

Suggested view modes:

```text
Small
Medium
Large
```

Cards should contain:

```text
thumbnail
filename
optional relative directory
```

Do not clutter cards with metadata.

---

# 16. Phase 11 — Search Version 1: Filename and Path

Initial search scope:

```text
filename
directory
relative path
```

Default syntax:

```text
ethernet switch
```

means:

```text
ethernet AND switch
```

Provide optional advanced syntax.

Examples:

```text
network switch

network/*switch*.svg

re:ethernet.*switch
```

Avoid exposing a complicated search form.

---

# 17. Phase 12 — Search Version 2: SVG Internal Content

Extend search across:

```text
filename
path
title
description
visible SVG text
IDs
classes
```

Ranking should favor:

```text
1. Exact filename
2. Filename token
3. Path
4. SVG title
5. SVG visible text
6. Description
7. IDs/classes
```

Example:

```text
Search:
motor controller
```

Could return:

```text
motor-controller.svg

power-system.svg
  because it contains "Motor Controller"

supervisory-controller.svg
  because it contains labels matching both tokens
```

Show why an asset matched when useful.

For example:

```text
system-overview.svg

Matched text:
"Ethernet Control Interface"
```

---

# 18. Phase 13 — Gallery Multi-Selection

Implement conventional Windows behavior.

```text
Click
single selection

Ctrl+Click
toggle selection

Shift+Click
range selection

Ctrl+A
select all filtered results

Esc
clear selection
```

Status bar:

```text
12 selected
8,462 results
```

Selection should survive scrolling because only visible DOM nodes exist.

Selection state therefore belongs outside individual cards.

---

# 19. Phase 14 — Native Multi-File Drag

Support:

```text
one selected SVG
multiple selected SVGs
```

Dragging any selected item should drag the complete selection.

Example:

```text
Selection:

motor.svg
pump.svg
converter.svg

drag
 ↓

PowerPoint / Visio / Explorer
```

The default operation must be:

```text
COPY
```

Never move source assets.

---

# 20. Phase 15 — Clipboard Operations

Implement clear actions.

## Entire asset

```text
Copy SVG
Copy PNG
Copy Path
Copy Filename
```

## Multi-selection

Initially:

```text
Copy Paths
```

Potential later feature:

```text
Copy multiple graphics
```

That capability can be investigated separately because receiving applications differ considerably.

---

# 21. Phase 16 — Detailed Viewer

Open using:

```text
Double click
Enter
Context Menu → View
```

Viewer layout:

```text
┌─────────────────────────────────────────────────────────────┐
│ icon.svg        Pan | Select       - 100% +   Fit   Reset  │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│                                                             │
│                       SVG Canvas                            │
│                                                             │
│                                                             │
├─────────────────────────────────────────────────────────────┤
│ 1200 × 800   32 KB   architecture/network/icon.svg          │
└─────────────────────────────────────────────────────────────┘
```

---

# 22. Phase 17 — Zoom and Pan

Support:

```text
Mouse wheel
zoom

Click-drag
pan

Space + drag
temporary pan

+
zoom in

-
zoom out

0
100%

F
fit document
```

Zoom should center around the mouse pointer.

Persist viewer state only during the current viewing session.

---

# 23. Phase 18 — Minimap

Add a minimap for medium/large diagrams.

```text
┌───────────────────────────┐
│ Main SVG                  │
│                           │
│        zoomed view        │
│                           │
└───────────────────────────┘

                    ┌───────┐
                    │       │
                    │ ┌───┐ │
                    │ │   │ │
                    │ └───┘ │
                    │       │
                    └───────┘
```

The minimap viewport rectangle should update while panning.

Clicking the minimap may recenter the main view.

---

# 24. Phase 19 — Region Selection

Viewer modes:

```text
Pan
Select Region
```

Selection is stored in SVG coordinate space.

Example:

```rust
SelectionRegion {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}
```

Visual selection:

```text
┌──────────────────────────────┐
│                              │
│    ┌─────────────────┐       │
│    │                 │       │
│    │ selected region │       │
│    │                 │       │
│    └─────────────────┘       │
│                              │
└──────────────────────────────┘
```

Show dimensions while dragging.

For example:

```text
850 × 420 SVG units
```

---

# 25. Phase 20 — Copy Region as SVG

This becomes a core feature.

Input:

```text
original.svg

selection:
x=1200
y=600
w=800
h=500
```

Output:

```text
temporary/copy.svg

viewBox="0 0 800 500"
```

Source SVG content should be:

```text
translated by -1200,-600
clipped to 800×500
```

Preserve:

```text
vectors
text
gradients
patterns
symbols
defs
clip paths
transparency
```

Do not attempt element-level editing.

This is viewport cropping rather than object extraction.

---

# 26. Phase 21 — Copy Region as PNG

Use the same selection coordinates.

Render only the requested region.

Provide:

```text
Copy PNG
Copy PNG 2x
Copy PNG with White Background
```

A future context menu could provide:

```text
1x
2x
4x
```

but 1x/2x is sufficient initially.

---

# 27. Phase 22 — Save Region

Allow:

```text
Save Selection as SVG
Save Selection as PNG
```

Default suggested filename:

```text
original-name-crop.svg
```

Do not modify the source file.

---

# 28. Phase 23 — Drag Selected Region

When a region exists, support:

```text
Drag Selection
```

The backend will create a temporary cropped SVG.

```text
region
 ↓
temporary SVG
 ↓
native drag
 ↓
PowerPoint / Visio / draw.io
```

Temporary files should live in the application's cache/temp directory.

Clean them:

```text
after drag if safe

or

on application shutdown/startup cleanup
```

Do not assume the receiver is finished reading the file immediately when the drag action returns.

---

# 29. Phase 24 — Context-Sensitive Ctrl+C

Final behavior:

```text
Viewer with region selected:

Ctrl+C
→ selected region as SVG


Viewer without region:

Ctrl+C
→ entire SVG


Gallery:

Ctrl+C
→ entire selected SVG
```

Potential multi-selection behavior:

```text
multiple gallery items selected

Ctrl+C
→ copy file paths
```

Avoid attempting multi-image clipboard composition in V1.

---

# 30. Phase 25 — Viewer Background Options

Viewer-only backgrounds:

```text
Transparent checkerboard
White
Dark
```

Default:

```text
Checkerboard
```

This makes transparent and white SVG content immediately visible.

Clipboard/export continues to preserve transparency unless explicitly overridden.

---

# 31. Phase 26 — File Watcher

Monitor:

```text
create
modify
rename
delete
```

Debounce bursts.

For example:

```text
Git checkout
→ 600 file events

filesystem watcher
→ coalesce

single reconciliation pass
```

Update:

```text
database
search index
thumbnail cache
gallery
```

incrementally.

---

# 32. Phase 27 — Startup Optimization

After the first indexing pass:

```text
Launch app
 ↓
open SQLite
 ↓
show previous gallery immediately
 ↓
background filesystem reconciliation
```

Do not perform a blocking full rescan.

Desired behavior:

```text
App opens
→ icons visible

then

"Checking for changes..."
```

rather than:

```text
"Scanning 28,000 files..."
```

before showing anything.

---

# 33. Phase 28 — Performance and Stress Tests

Create deterministic datasets.

## Dataset A

```text
1,000 SVGs
```

## Dataset B

```text
10,000 SVGs
```

## Dataset C

```text
50,000 SVGs
```

## Dataset D

```text
10,000 SVGs

with significant <text> content
```

## Dataset E — pathological files

Include:

```text
zero-byte SVG
malformed XML
25 MB file
100 MB file
many nested groups
very large path
embedded PNG
external resource links
scripts
animations
massive <text> content
unusual Unicode
```

---

# 34. Performance Targets

Suggested targets:

| Operation                 |          Target |
| ------------------------- | --------------: |
| Application shell visible |         <500 ms |
| Cached gallery visible    |         <500 ms |
| First uncached assets     |          <1 sec |
| Search filename/path      |          <50 ms |
| Search full content       |         <100 ms |
| Grid scrolling            |         ~60 FPS |
| Normal viewer open        |         <200 ms |
| Thumbnail cache hit       |          <30 ms |
| Region-copy initiation    | <200 ms typical |
| Noticeable UI stalls      |     none >50 ms |

These should be engineering targets rather than hard contractual guarantees.

---

# 35. Phase 29 — UI Polish

The product should resemble:

```text
Raycast
+
modern asset browser
+
VS Code quick search
```

rather than a traditional file explorer.

Main UI:

```text
┌──────────────────────────────────────────────────────────────┐
│ [Folder ▾]   🔍 Search 12,842 SVGs...        ◫ size   ⋯      │
├──────────────────────────────────────────────────────────────┤
│                                                              │
│   SVG       SVG       SVG       SVG       SVG                 │
│                                                              │
│   SVG       SVG       SVG       SVG       SVG                 │
│                                                              │
│                     virtual grid                             │
│                                                              │
├──────────────────────────────────────────────────────────────┤
│ 12,842 assets                 Indexed ✓        3 selected     │
└──────────────────────────────────────────────────────────────┘
```

Avoid sidebars unless a future feature truly requires one.

---

# 36. Context Menu

Gallery item:

```text
View

Copy SVG
Copy PNG
Copy Full Path
Copy Filename

Reveal in Explorer
Open Externally

────────────────

Properties
```

Multiple selection:

```text
Drag Selected

Copy Paths

Reveal in Explorer
```

Viewer with selected region:

```text
Copy Selection as SVG
Copy Selection as PNG
Save Selection as SVG
Save Selection as PNG
Drag Selection

Clear Selection
```

---

# 37. Keyboard Model

Suggested shortcuts:

```text
Ctrl+F
Focus search

Ctrl+A
Select all results

Ctrl+C
Context-aware copy

Ctrl+Shift+C
Copy full path

Enter
Open viewer

Esc
Clear region / close viewer / clear selection

Arrow keys
Move gallery selection

Space
Temporary pan

S
Region selection mode

H
Pan/hand mode

+
Zoom in

-
Zoom out

0
100%

F
Fit SVG
```

---

# 38. Backend Module Structure

```text
src-tauri/src/

├── lib.rs

├── commands/
│   ├── library.rs
│   ├── assets.rs
│   ├── search.rs
│   ├── preview.rs
│   ├── clipboard.rs
│   ├── drag.rs
│   └── viewer.rs

├── library/
│   ├── scanner.rs
│   ├── watcher.rs
│   ├── fingerprint.rs
│   ├── limits.rs
│   └── models.rs

├── index/
│   ├── database.rs
│   ├── migrations.rs
│   ├── metadata.rs
│   └── text.rs

├── search/
│   ├── query.rs
│   ├── parser.rs
│   ├── matcher.rs
│   └── ranking.rs

├── svg/
│   ├── parser.rs
│   ├── metadata.rs
│   ├── text_extract.rs
│   ├── renderer.rs
│   ├── crop.rs
│   └── limits.rs

├── preview/
│   ├── queue.rs
│   ├── cache.rs
│   └── worker.rs

├── native/
│   ├── clipboard.rs
│   ├── drag.rs
│   └── explorer.rs

└── config/
    └── settings.rs
```

---

# 39. Frontend Structure

```text
src/

├── app/

├── components/
│   ├── SearchBar.tsx
│   ├── AssetGrid.tsx
│   ├── AssetCard.tsx
│   ├── AssetContextMenu.tsx
│   ├── MultiSelectionToolbar.tsx
│   ├── StatusBar.tsx
│   └── EmptyState.tsx

├── viewer/
│   ├── SvgViewer.tsx
│   ├── ViewerToolbar.tsx
│   ├── SelectionOverlay.tsx
│   ├── MiniMap.tsx
│   └── ViewerContextMenu.tsx

├── hooks/
│   ├── useLibrary.ts
│   ├── useSearch.ts
│   ├── useVirtualGrid.ts
│   ├── useSelection.ts
│   ├── useViewer.ts
│   └── useKeyboard.ts

├── stores/
│   ├── libraryStore.ts
│   ├── searchStore.ts
│   ├── selectionStore.ts
│   └── viewerStore.ts

└── api/
    └── tauri.ts
```

---

# 40. Revised Work Package Order

## WP-01 — Project foundation

Deliver:

```text
Tauri
React
TypeScript
Tailwind
basic shell
logging
configuration
```

---

## WP-02 — Native integration spike

Validate:

```text
drag to PowerPoint
drag to Visio
drag to draw.io
SVG clipboard
multi-file drag
```

---

## WP-03 — Region-copy spike

Validate:

```text
viewer
selection rectangle
SVG-coordinate conversion
cropped SVG generation
paste into Office
```

---

## WP-04 — Directory library

Implement:

```text
directory picker
recent library
recursive scanning
progressive results
```

---

## WP-05 — Persistent catalog

Implement:

```text
SQLite
asset records
change detection
startup restore
```

---

## WP-06 — SVG metadata processing

Implement:

```text
dimensions
viewBox
title
description
text
IDs/classes
complexity measurement
```

---

## WP-07 — Preview pipeline

Implement:

```text
thumbnail queue
priority processing
render cache
error states
```

---

## WP-08 — Virtualized gallery

Implement:

```text
fast grid
viewport priority
smooth scrolling
thumbnail lazy loading
```

---

## WP-09 — Filename/path search

Implement:

```text
token search
glob
optional regex
ranking
```

---

## WP-10 — Multi-selection

Implement:

```text
Ctrl+Click
Shift+Click
Ctrl+A
keyboard navigation
persistent virtual-grid selection
```

---

## WP-11 — SVG-content search

Implement:

```text
title
description
text
IDs
classes
ranking
match explanations
```

---

## WP-12 — Native drag productionization

Implement:

```text
single drag
multi-file drag
drag image
copy semantics
Office compatibility
```

---

## WP-13 — Clipboard productionization

Implement:

```text
SVG
PNG
path
filename
native format ordering
```

---

## WP-14 — Viewer

Implement:

```text
zoom
pan
fit
100%
background modes
metadata
```

---

## WP-15 — Region selection

Implement:

```text
SVG coordinate mapping
selection overlay
selection manipulation
keyboard mode switching
```

---

## WP-16 — Region output

Implement:

```text
copy SVG
copy PNG
save SVG
save PNG
normalize origin
transparent background
```

---

## WP-17 — Region drag-out

Implement:

```text
temporary cropped SVG
native drag
cleanup lifecycle
```

---

## WP-18 — Minimap

Implement:

```text
overview
viewport indicator
click navigation
```

---

## WP-19 — Filesystem synchronization

Implement:

```text
watcher
event coalescing
incremental database updates
thumbnail invalidation
```

---

## WP-20 — Performance hardening

Test:

```text
50k assets
large directory trees
full-text search
rapid scrolling
rapid filesystem changes
bad SVG inputs
```

---

## WP-21 — UI polish

Finalize:

```text
spacing
icons
keyboard cues
context menus
loading states
empty states
error states
subtle animation
```

---

## WP-22 — Installer and release

Produce:

```text
Windows x64 release

offline operation

signed installer if infrastructure allows
```

---

# 41. Recommended MVP

I would define the first genuinely useful release as:

```text
Directory selection

Recursive indexing

Persistent SQLite index

Fast thumbnails

Virtualized grid

Filename/path search

SVG-text search

Single and multi-selection

Copy path

Copy full SVG

Native SVG drag

Multi-file drag

Viewer

Zoom/pan

Region selection

Copy region as SVG

Copy region as PNG

Transparent background

Basic SVG complexity limits
```

That is a substantially stronger MVP than merely building a thumbnail browser.

The distinguishing workflow becomes:

```text
Find
→ Preview
→ Inspect
→ Select
→ Crop if needed
→ Copy/drag directly into engineering documentation
```

---

# 42. Post-MVP Opportunities

Do not implement these initially, but leave architectural room for them.

## Favorites

```text
Star commonly used SVGs
```

## Collections

```text
Electrical
Network
Controls
C4
Navy Symbols
Presentation Assets
```

without moving source files.

## Duplicate detection

Use the content hash to identify exact duplicates.

Eventually perceptual similarity could also be investigated.

## SVG text highlighting

If search matched:

```text
Ethernet
```

the detailed viewer could eventually pan/zoom to the matching text.

This could become particularly useful for engineering diagrams.

## Search result snippets

Example:

```text
power-architecture.svg

Matched:
"... Ethernet interface connects to supervisory controller ..."
```

## Recently used

Track assets recently:

```text
opened
copied
dragged
```

## Frequently used

Surface commonly reused icons.

## Multiple libraries

For example:

```text
Corporate Icons

Project Diagrams

Architecture Assets

Personal Library
```

## Collections independent of filesystem

A collection would contain references to indexed assets rather than copies.

---

# 43. Architectural Principle to Preserve

The most important rule throughout the implementation should be:

> The filesystem and SVG complexity must never determine whether the UI is responsive.

Every potentially expensive action should therefore be:

```text
incremental
asynchronous
bounded
cancelable where practical
prioritized toward visible content
cached whenever useful
```

The frontend should always be able to:

```text
scroll
search
select
open menus
navigate
```

while Rust continues indexing, parsing, hashing, or rendering in the background.

That principle will matter more to the perceived quality of the application than almost any individual UI component.
