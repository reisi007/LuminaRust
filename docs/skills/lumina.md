---
name: lumina
description: Use the LuminaRust MCP server to load images, edit them non-destructively via recipes, preview renders, and export — all as MCP tools for AI agents.
---

# LuminaRust MCP Skill

LuminaRust is a **non-destructive** RAW/image processor. The MCP server
(`lumina-mcp`, started via `lumina mcp` or the `lumina-mcp` binary) exposes it
to AI agents as a small set of JSON-RPC tools over stdio. This skill explains
how an agent should think about and drive it.

## Core philosophy: sidecar-first, recipe-based

- **The original file is never modified.** Every edit lives in a portable
  sidecar next to the original: `<image>.<ext>.lumina.json`.
- An edit is a **recipe** — a declarative list of adjustments (exposure,
  contrast, white balance, …) plus virtual copies. Rendering applies the recipe
  to the original pixels on demand; it never bakes anything into the source.
- A **virtual copy** is an independent editing variant of the same source. It
  has a stable id, a name, and its own full recipe. The default copy is
  `vc-original` (name `Original`).
- Persistence is the sidecar. Re-opening a project re-reads the sidecar — no
  re-inference or re-render is required for existing adjustments.

Because of this, the agent's job is to **load → inspect → edit (write-through
to the sidecar) → preview (look) → adjust → save**. Edits are cheap and
permanent (until the sidecar is deleted).

## Transport & protocol

- Start the server; it speaks newline-delimited JSON-RPC 2.0 on stdin/stdout.
- Always `initialize` first (returns `protocolVersion`, `capabilities.tools`,
  `serverInfo`). Then `tools/list` to discover tools.
- Each tool call is `tools/call` with `{ "name": "<tool>", "arguments": {…} }`.
- Results come back as `{ "content": [{ "type": "text", "text": "<json>" }],
  "isError": false, "structuredContent": <json> }`. Prefer
  `structuredContent`; the `text` block is the same JSON as a string.
- `notifications/initialized` is a notification (no id) and expects no reply.
- Error responses carry `error.code` (JSON-RPC) and `error.data.error` — a
  stable machine name like `FileNotFound`, `UnsupportedFormat`,
  `NoImageLoaded`, `InvalidAdjustment`, `DecodeError`, `UnsupportedFormat`,
  `UnknownCopy`, `MethodNotFound`. Branch on `data.error`, not on the message
  text.

## Tools

All tools take `image_id` (returned by `lumina_load`) except `lumina_load`.

### `lumina_load`
`{ "path": "/abs/path/to/image.ARW" }` →
`{ "image_id", "width", "height", "format", "virtual_copies": [names], "sidecar_status": "loaded"|"created" }`.
Accepts RAW (arw/cr2/cr3/dng/nef/…), PNG, JPEG, WebP. Loads an existing sidecar
or creates a default one. A new `lumina_load` discards any previously loaded
image (single-image session).

```json
{"name":"lumina_load","arguments":{"path":"/photos/portrait.ARW"}}
```

### `lumina_edit`
`{ "image_id", "virtual_copy"? (name or id, default = standard copy),
"adjustments": { "exposure": -10..=10, "contrast"|"highlights"|"shadows"|"whites"|"blacks"|"wb_tint"|"vibrance"|"saturation": -1..=1,
"wb_temperature": 1500..=12000 } }` → `{ "ok": true, "recipe_hash" }`.
Only provided adjustment keys are overwritten; others are left untouched.
Out-of-range or unknown keys are rejected with `InvalidAdjustment` (never
silently clipped). The sidecar is written **atomically** (write-through). The
call is idempotent: identical input → identical `recipe_hash`.

```json
{"name":"lumina_edit","arguments":{"image_id":"a1b2c3d4","adjustments":{"exposure":1.2,"highlights":-0.3}}}
```

### `lumina_get_recipe`
`{ "image_id", "virtual_copy"? }` → `{ "recipe": <EditRecipe>, "recipe_hash" }`.
Returns the **full** recipe (adjustments nested under `recipe.adjustments`,
plus curves/hsl/etc. if present) and its hash so you can detect changes.

### `lumina_preview`
`{ "image_id", "virtual_copy"?`, "max_width"? (default 1024) }` →
`{ "ok", "preview_path", "width", "height", "size_bytes" }`.
Renders the full recipe, then bilinearly downscales to `max_width` (aspect
preserved, never upscaled) and writes a **PNG** to the preview dir
(`$LUMINA_MCP_PREVIEW_DIR`, default `$TMPDIR/lumina-previews/`) as
`<image_id>.png`. It is **cache-free** and **deterministic** — same recipe +
source → byte-identical file — so you can hand `preview_path` to a vision
model and compare successive previews.

### `lumina_save`
`{ "image_id", "output_path", "virtual_copy"?`, "format"? ("png"|"jpeg"|"webp",
default "png"), "quality"? (1..=100, default 90, JPEG/WebP only) }` →
`{ "ok", "bytes_written", "path" }`. Full-resolution render via the shared
entry point. Refuses to overwrite the original.

### `lumina_list_virtual_copies`
`{ "image_id" }` → `{ "copies": [ { "id", "name", "recipe_hash" } ] }`.
At least one copy (the default) is always present.

### `lumina_inspect`
`{ "image_id" }` → sidecar status/metadata **without decoding pixels**:
`source_path`, `sidecar_path`, `schema_version`, `recipe_version`,
`pipeline_version`, `virtual_copies` (count), `ai_masks` (list of
`{ layer, copy, status }`). Use this to check what masks/sidecars already
exist before deciding to edit.

### `lumina_analyze` (vision-less agents)
`{ "image_id", "virtual_copy"? }` → structured JSON: `exposure_estimate`
(`ev` to reach mid-gray, `median_luminance`, `mean_luminance`), `luminance`
(mean/median/p01/p99/stddev), `per_channel` (mean/stddev/min/max for r/g/b),
`histogram` (256-bin luminance + per-channel), and `dominant_colors`
(`[{ rgb, frequency }]`). Use this instead of (or before) a vision preview to
judge brightness, contrast and color balance programmatically.

## Typical workflow

```
load    → image_id, metadata, copy list
inspect → what masks/sidecars already exist
edit    → set exposure/contrast/wb (write-through to sidecar)
preview → render <image_id>.png, feed to vision (or analyze)
edit    → refine based on what you saw
preview → confirm
save    → export final to PNG/JPEG/WebP
```
This load→edit→preview→save loop is the key agent feedback cycle: the preview
is the fast, deterministic "what does it look like now?" signal.

## Best practices

- **Track `recipe_hash`.** After each `lumina_edit`, compare the new hash to
  the previous one to confirm the change landed (and to detect idempotency).
- **Don't re-load to re-render.** `lumina_preview`/`lumina_save`/`lumina_analyze`
  always render the *current* sidecar recipe, so just call them again after an
  edit.
- **Small, incremental edits.** Set one or a few adjustments per `lumina_edit`;
  it's cheap and atomic.
- **Use `lumina_analyze` for fast judgement** when a vision model isn't
  available, or as a cheap first pass before spending a vision call on the
  preview.
- **Identity is per-session.** `image_id` is stable only for the running
  server. After a restart, `lumina_load` again to get a fresh id.
- **Pick the right copy.** Pass `virtual_copy` by name or id when you want a
  specific variant; omit it for the default copy.

## Error handling

| `data.error` | Meaning | Recovery |
| --- | --- | --- |
| `FileNotFound` | path missing or unreadable | check the path exists and is readable |
| `UnsupportedFormat` | extension not png/jpeg/webp/raw | use a supported format |
| `DecodeError` | file present but unreadable/corrupt | the file may be damaged |
| `NoImageLoaded` | tool called before `lumina_load` (or wrong session) | `lumina_load` first |
| `UnknownImage` | `image_id` doesn't match the loaded image | re-`lumina_load`, use its `image_id` |
| `UnknownCopy` | `virtual_copy` name/id not found | list copies via `lumina_list_virtual_copies` |
| `InvalidAdjustment` | value out of range or unknown key | respect the documented ranges |
| `UnsupportedFormat` (save) | `format` not png/jpeg/webp | use a supported output format |
| `MethodNotFound` | unknown tool name | check `tools/list` |

Never assume a tool succeeded: inspect `isError` / `error` in every response.
