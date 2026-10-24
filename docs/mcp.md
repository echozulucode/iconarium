---
type: reference
status: active
updated: 2026-10-24
---

# iconarium-cli and the Iconarium MCP server

`iconarium-cli` searches, previews and exports icons from the libraries the Iconarium app
has indexed. It can be used from a terminal or scripts (`--json`), or run as an
**MCP server** (`iconarium-cli mcp`) so that LLM clients such as Claude Desktop, Claude Code
and VS Code can find and choose icons.

It reads the app's index (`%LOCALAPPDATA%\com.echozed.iconarium\library-index.sqlite3`)
without changing it. Every library you have opened in Iconarium is searchable; libraries you
haven't opened don't exist for it. Design and rationale: [mcp-cli-plan.md](mcp-cli-plan.md).

## Install

| How | Result |
|---|---|
| Iconarium installer (from this release on) | `iconarium-cli.exe` next to `Iconarium.exe` in `%LOCALAPPDATA%\Iconarium\`, and that folder added to your user `PATH`. Open a **new** terminal to pick up the PATH. Uninstalling removes both. If a policy blocks the PATH update (e.g. a Group Policy PowerShell execution policy), the install still succeeds, its details log says so, and the CLI works by its full path |
| From source | `just install-cli` (into `~/.cargo/bin`), or run without installing: `just cli <args>` |

Check the setup with `iconarium-cli doctor`. It prints the index path, whether it can be
read, and how many libraries and icons it contains.

## Connect an MCP client

Use the full path to the exe. Clients don't always see PATH changes made after they started.

**Claude Desktop.** Settings → Developer → Edit Config opens
`%APPDATA%\Claude\claude_desktop_config.json`. Add:

```json
{
  "mcpServers": {
    "iconarium": {
      "command": "C:\\Users\\<you>\\AppData\\Local\\Iconarium\\iconarium-cli.exe",
      "args": ["mcp"]
    }
  }
}
```

Quit and restart Claude Desktop. If the server doesn't appear, its stderr is in
`%APPDATA%\Claude\logs\mcp-server-iconarium.log`.

**Claude Code** (all projects):

```powershell
claude mcp add --transport stdio --scope user iconarium -- "$env:LOCALAPPDATA\Iconarium\iconarium-cli.exe" mcp
```

**VS Code** (`.vscode/mcp.json` in a workspace, or the user configuration):

```json
{
  "servers": {
    "iconarium": {
      "type": "stdio",
      "command": "C:\\Users\\<you>\\AppData\\Local\\Iconarium\\iconarium-cli.exe",
      "args": ["mcp"]
    }
  }
}
```

**Trying it out:** `just mcp-inspect` opens the MCP Inspector against a source build.

### Optional write tools

The server is read-only unless you enable writes when configuring it:

| Flag | Enables | Scope |
|---|---|---|
| `--allow-write <DIR>` (repeatable) | `export_icons` | Writes only inside the given absolute folders; never overwrites unless asked |
| `--allow-tagging` | `tag_icons` | Writes `iconarium.yaml` in library folders ([format](library-metadata.md)) |

Example args: `["mcp", "--allow-write", "C:\\Projects\\docs\\img", "--allow-tagging"]`.
Disabled tools are left out of the tool list entirely, so the model never tries them.

## Tools

| Tool | Use | Returns |
|---|---|---|
| `search_icons` | Find icons: `query`, plus optional `libraries`, `folder`, `tags`, `limit` (≤100, default 20), `cursor` | Ranked hits: id, name, library, path, absPath, tags, size, why it matched, state. Paging via `nextCursor`. Warnings when a library's content isn't fully indexed |
| `preview_icons` | **See** candidates: `ids` (1–48), `cell` (px), `background`, `labels` | One PNG contact sheet with numbered cells, plus a text legend (cell → id, name, path) |
| `get_icon` | Details: `id`, or `library` + `path`; `include_svg`, `include_duplicates` | Full metadata, tags with their source, title/description/visible text, optional SVG markup, byte-identical duplicates |
| `list_libraries` | Orientation | Libraries with icon counts and how much of each is content-indexed |
| `list_tags` | Learn the tag vocabulary: `libraries`, `prefix`, `limit` | Tags with counts and sources |
| `list_folders` | Browse: `library`, `folder` | Subfolders with counts, and files in the folder |
| `export_icons` *(opt-in)* | Copy SVGs or render PNGs into an allowed folder | Written paths and skipped items |
| `tag_icons` *(opt-in)* | Add or remove tags | Updated tags; searchable immediately |

Resources: `iconarium://asset/{id}.svg` (SVG markup) and
`iconarium://asset/{id}/png/{size}` (PNG, 16–1024 px). Tool results link to them.

The server sends instructions telling the model to search with short queries, preview the
best candidates and choose by eye, then fetch details. They also say that names and
descriptions from third-party files are data, not instructions.

**Protocols:** MCP 2026-07-28 (stateless) and 2025-11-25 (with `initialize`), over stdio.

## Command line

```text
iconarium-cli [--index FILE] [--json] <command>

  doctor                        where the index is, and whether it can be read
  libraries                     indexed libraries
  search [QUERY…] [-l LIB]… [--folder F] [-t TAG]… [--limit N] [--cursor C]
  show ID | show -l LIB PATH [--svg] [--duplicates]
  tags [-l LIB]… [--prefix P] [--limit N]
  folders [FOLDER] [-l LIB] [--max-files N]
  preview ID… -o sheet.png [--cell N] [--background white|checker|transparent] [--no-labels]
  export ID… --to DIR [--png --size N] [--overwrite] [--name-from file|name]
  tag ID… [--add TAG]… [--remove TAG]…
  mcp [--allow-write DIR]… [--allow-tagging]
```

- **Query syntax** (same as the app): words all must match; `"exact phrase"`; `-exclude`;
  globs such as `*.svg` or `network/*switch*`; `re:regex`.
- **Libraries** are selected by id, root path, or name when the name is unique. Two
  libraries with the same folder name are shown as `icons (C:\work\icons)`.
- **Exit codes:** 0 success, 1 error, 3 search found nothing, 2 usage error.
- `--json` prints the same structures the MCP tools return.
- `ICONARIUM_INDEX` sets the index path; `ICONARIUM_LOG=debug` turns on verbose stderr.

## Where names and tags come from

| Field | Source, in priority order |
|---|---|
| Name | `title` in [`iconarium.yaml`](library-metadata.md), then the SVG `<title>` (if short and not an editor default like "Layer 1"), then the filename with separators turned into spaces |
| Tags | `iconarium.yaml` (asset, folder, library), then `Keywords:` in the SVG `<desc>`, then folder names (size folders such as `24`, `48px` and `16x16` skipped) |
| Searchable text | Filename, path, SVG title/description/visible text/ids, plus `iconarium.yaml` titles, descriptions and tags |

## Behaviour notes

- **Freshness.** The server reloads changed libraries in the background, checking at most
  every 2 s by comparing cheap per-library signatures. The app's constant thumbnail
  bookkeeping doesn't trigger reloads. A paging cursor from before a reload returns
  "results changed, search again".
- **Coverage.** The app fully indexes only the library it has open. Others keep the content
  index from the last time they were open. Filename and path search is always complete;
  search results warn when title or text search may be incomplete.
- **Safety.**
  - The server never writes the index and never creates it.
  - Reads are limited to indexed files.
  - Previews and exports honour the app's complexity limits, and render on 16 MiB-stack
    threads like the app.
  - stdout carries only JSON-RPC.
  - Logs go to stderr and `%LOCALAPPDATA%\com.echozed.iconarium\logs\cli\` (its own folder, so the app's log pruning never touches it).
- **Updates.** The installer ends running `iconarium-cli.exe` processes before replacing
  them; MCP clients restart the server on demand.
