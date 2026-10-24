---
type: reference
status: active
updated: 2026-10-24
---

# `iconarium.yaml`: library metadata format (version 1)

An optional YAML file in the **root folder of an icon library** that adds titles,
descriptions and tags to the SVGs below it. It belongs to the library, not to the app, so it
travels with the folder through copies, shares and git. Any tool can read or write it.

Today it is read by `iconarium-cli` and its MCP server (search, filters, names) and written
by `iconarium-cli tag` and the MCP `tag_icons` tool. The desktop app doesn't read it yet.

## Example

```yaml
# Iconarium library metadata (format 1)
iconarium: 1
library:
  name: Corporate Icons
  description: Approved icons for customer-facing material
  tags: [corporate, approved]          # every asset in the library
folders:
  network:                             # this folder and everything below it
    tags: [networking]
  network/switches:
    description: Layer-2/3 switches
    tags: [switch]
assets:
  network/router.svg:
    title: Cisco edge router           # display name
    description: Branch office WAN edge
    tags: [edge, wan]
  ui/home.svg:
    tags: [house, start]
```

## Fields

| Key | Type | Meaning |
|---|---|---|
| `iconarium` | integer | Format version. Writers must write `1`. Readers assume `1` when absent and **ignore files with a higher version** |
| `library.name` | string | Human name for the library (informational) |
| `library.description` | string | What the library is for |
| `library.tags` | list of strings | Tags for every asset in the library |
| `folders.<path>.description` | string | What the folder holds |
| `folders.<path>.tags` | list of strings | Tags for every asset in that folder **and its subfolders** |
| `assets.<path>.title` | string | Display name. It takes precedence over the SVG `<title>` and the filename |
| `assets.<path>.description` | string | Searchable description |
| `assets.<path>.tags` | list of strings | Tags for this asset |

All keys are optional. Unknown keys are ignored, so later versions can add fields without
breaking older readers.

## Paths

- Relative to the folder holding `iconarium.yaml`, with `/` separators (`\` is accepted and
  converted). No leading `/`, no `..`.
- Asset paths name the file including `.svg`. Folder paths name a folder; a folder entry
  applies to paths that start with `<folder>/`, so `network` doesn't match `networking/`.
- Matching tries an exact match first, then a case-insensitive one, because Windows file
  names are case-insensitive.

## Tags

- Normalized on read: trimmed, inner whitespace collapsed, lowercased. `Load  Balancer`
  becomes `load balancer`.
- 1–64 characters. No `,`, `;` or line breaks; those are list separators elsewhere.
  Invalid tags are ignored on read and rejected on write.
- An asset's effective tags combine, in this order and without duplicates: asset tags,
  folder tags (inherited), library tags. Tools may add tags from other sources;
  `iconarium-cli` also derives tags from `Keywords:` in the SVG's `<desc>` and from folder
  names.

## Writing

Writers should:

1. Read and parse the existing file. Refuse to write if it doesn't parse or has a newer version.
2. Change only the entries they mean to change, keeping unknown keys, and reusing existing
   keys (case-insensitive) so an edit doesn't fork an entry.
3. Drop asset entries left with no title, description or tags.
4. Write the whole file atomically (write a temporary file, then rename).

Comments and hand formatting can't survive a structured rewrite. Before rewriting,
`iconarium-cli` compares the file with its own canonical rendering of it. If they differ
(comments, ordering, quoting), it first copies the original to `iconarium.yaml.bak`. Unknown
keys are preserved. If you hand-edit a file that tools also update, keep notes in
`description` fields instead of comments.

Output from `iconarium-cli` is deterministic: keys are sorted and tags within an asset are sorted.

## Relationship to other sources

| Source | Scope | Who writes it |
|---|---|---|
| `iconarium.yaml` | Per library, travels with the folder | People, `iconarium-cli tag`, MCP `tag_icons` |
| `Keywords:` in `<desc>` | Per SVG, inside the file | Authoring tools, e.g. `tools/excalidraw-to-svg --meta` |
| Folder names | Implicit | The folder layout |
| `--meta` JSON for excalidraw-to-svg | Conversion input only | People or tools (e.g. `vision.json`) |
