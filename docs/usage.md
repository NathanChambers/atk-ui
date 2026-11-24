# ATK UI usage guide

[Installation and quickstart](../README.md)

## Quickstart

In a fresh project directory, create and render a first screen:

```powershell
'{"command":"session_start","name":"Hello UI"}' | atk-ui-cli --session hello-ui
'{"command":"screen_set","id":"home","width":720,"height":440,"background":"#17202b"}' | atk-ui-cli --session hello-ui
'{"command":"element_set","screen":"home","id":"title","type":"text","x":48,"y":48,"width":480,"height":72,"text":"Hello UI","appearance":{"fontSize":40,"ink":[239,245,252,255]}}' | atk-ui-cli --session hello-ui
'{"command":"session_render","screen":"home","format":"png","path":"hello-ui.png"}' | atk-ui-cli --session hello-ui
atk-ui
```

Open the [UI viewer](http://127.0.0.1:7881) and choose **Hello UI** with the folder button. The CLI saves the design under `.ui/sessions/`.

## Author a screen

- `screen_set` saves the design width and height. `fixed_aspect` preserves those dimensions. `content_height` uses the declared height as a minimum and extends to the bottom of visible elements.
- `element_set` places panels, text, sprites, controls, or navigation hotspots by ID. `appearance` accepts RGBA colors, numeric dimensions, corner rounding or cuts, font roles, and text alignment.
- `sprite_import` imports a PNG image. Use the sprite ID on a positioned element.
- `flow_set` arranges views and links. A frame path starts at the entry screen. Set `screen` instead of `path` to place an authored screen alongside the flow as a reference. Standalone screen frames have no flow connections. Each link between navigation frames must match an authored action that reaches its destination.
- The CLI saves projects under `.ui/sessions/`. Use `expectedRevision` to reject stale writes, and `session_inspect` to read the authored document.

```json
{"command":"element_set","id":"open-details","screen":"overview","type":"hotspot","x":40,"y":80,"width":200,"height":64,"action":{"type":"navigate","screen":"details"}}
```

## Iterate on a project

- `element_patch` changes selected fields. `element_duplicate`, `element_move`, and `element_reorder` work across the screen's zero-based paint order. Later elements draw on top.
- `component_element_patch` edits a child definition. `flow_frame_*` and `flow_edge_*` add, patch, or remove board entries. Identify an existing edge by its source frame, destination frame, and action.
- `session_duplicate` creates a new workspace from the saved design, and `session_rename` changes its display name. Screen duplicate, rename, and remove commands use screen IDs. Renaming a screen updates incoming navigation. Removal checks that references are valid after the edit.
- Use `session_validate` to check structure, references, native scene construction, and flow replay. Use the `batch` command for related design changes that must commit together. A successful batch increments the revision once. Put `expectedRevision` on the batch itself.

```json
{"command":"batch","expectedRevision":12,"commands":[{"command":"element_patch","screen":"home","id":"deploy","changes":{"x":900}},{"command":"flow_frame_patch","id":"home","changes":{"x":80}}]}
{"command":"session_render","screen":"dashboard","format":"png","path":"dashboard.png"}
{"command":"element_inspect","frame":"home","id":"site-header/brand"}
```

The optional `screen` or `frame` selector renders directly from the saved project. A frame replays its authored path for that request. These selectors leave the CLI preview cursor unchanged.

## Discover views and assets

Use `session_manifest` for the entry screen, authored screens, resolved flow frames, transitions, and their dependencies without asset payloads. Authored screens list potential dependencies, and each flow frame lists the dependencies of its visible state.

```json
{"command":"session_manifest"}
{"command":"asset_inspect","kind":"sprite","id":"button-art"}
{"command":"asset_read","kind":"sprite","id":"button-art","path":"button-copy.png"}
{"command":"asset_inspect","kind":"font","id":"neutral"}
{"command":"asset_inspect","kind":"nine_patch","id":"button-primary-patch"}
```

Use `asset_read` for original PNGs and embedded fonts. Omit the output path to receive Base64. The CLI requires an unused output path when writing a file. Inspect a local font to get the exact installed file selected by the renderer. Omit the font ID to inspect the renderer fallback.

## Reuse visual components

Define a component with `component_set` and a list of ordinary visual elements. Place it with `element_set` using type `component`, a component ID, and a position. Child coordinates are relative to the instance. Optional `overrides` keyed by child ID change text or appearance for one instance. Update the definition to change every instance. Author transition zones on the screen so they can point at different destinations.

```json
{"command":"component_set","id":"header","elements":[{"id":"title","type":"text","x":0,"y":0,"width":240,"height":40,"text":"Overview"}]}
{"command":"element_set","type":"component","component":"header","id":"page-header","screen":"home","x":32,"y":24,"overrides":{"title":{"text":"Home"}}}
```

## Preview and export

Run commands with JSON via stdin, `--file`, or `--command`. Use `help` to see the current command contract. Arrays execute in order and retain successful commands if a later command fails.

Use the `hello-ui` session from the [quickstart](../README.md#quickstart) to try these commands.

```powershell
'{"command":"session_render"}' | atk-ui-cli --session hello-ui
'{"command":"session_render","format":"scene","path":"hello-ui-scene.json"}' | atk-ui-cli --session hello-ui
'{"command":"session_render","format":"png","path":"hello-ui-frame.png"}' | atk-ui-cli --session hello-ui
'{"command":"preview_reset"}' | atk-ui-cli --session hello-ui
```

To try an action on a screen with controls or hotspots, pass a target from the JSON render to `preview_action`.

The CLI stores its temporary preview under `.ui/preview/`. Use `preview_inspect` to inspect it. Preview actions and resets leave the saved design revision intact. Design edits invalidate the preview, and the viewer reads the authored entry and flow independently.

JSON output describes the visible screen and available actions. Scene output contains the renderer's primitives. PNG and HTML export the current frame. HTML wraps the rendered image as an offline preview.

The **Grayscale** and **Gaze** viewer modes inspect the same native frame. A gaze study is authored with `gaze_set`, using rectangles in design pixels, scores from 0 to 100, and an optional ordered path. It applies to a particular preview state.

## Workspace appearance

Import a PNG, register its fixed edges in left, top, right, bottom order, then reference the patch from an element or a shared style. The renderer preserves the corners and stretches the center and edges as needed.

```json
{"command":"sprite_import","id":"button-art","path":"button.png"}
{"command":"nine_patch_set","id":"button-patch","patch":{"sprite":"button-art","insets":[12,12,12,16],"edgeScale":1}}
{"command":"style_set","id":"primary","style":{"appearance":{"patch":"button-patch","ink":[255,255,255,255],"fontSize":24,"centered":true},"defaultSize":[200,64]}}
{"command":"element_set","screen":"home","id":"continue","type":"button","x":32,"y":48,"appearanceRef":"primary","text":"Continue"}
```

Explicit element dimensions override a shared style's creation defaults. Inline appearance overrides individual style fields. Editing a shared style updates its users' appearance without changing their authored bounds.

Use `font_set` to register a font ID in the workspace. Set `path` to embed an OTF or TTF file you can distribute, or give `font.localNames` as an ordered list of installed font filenames. Set `font.weight` for variable fonts. Reference the font ID in an element or shared style's `appearance.font`.


### Implementation handoff

```powershell
'{"command":"session_handoff","path":"handoff.json"}' | atk-ui-cli --project . --session hello-ui
```

The command writes a new, engine-neutral JSON file from one saved revision
without using the live preview cursor. Pass `expectedRevision` to require a
specific revision. The bundle contains:

- `format` and `version`, the export format and schema version.
- `manifest`, the revision, view inventory, flow transitions, and dependencies.
- `design`, the authored screens, component definitions and overrides, styles,
  visibility rules, actions, sprites, nine-patches, and font descriptors.
- `references.screens` and `references.frames`, keyed by ID. Each entry has
  resolved screen elements in paint order, available actions, and a PNG reference
  in `image.dataBase64`. Frame references replay the saved flow path.

Geometry uses design pixels from the top-left. Asset payloads are stored once
in `design`. Embed fonts in the session for a portable bundle, or supply the
local font dependencies when reconstructing the UI. Reference PNGs use the
font resolved by the exporting machine. The export preserves authored
relationships, so a standalone state example remains a standalone reference
unless the design explicitly connects it.


### Guided image comparison

```powershell
'{"command":"image_compare","reference":"mockup.png","capture":"capture.png","path":"comparison.html","focus":["Header alignment"],"expectedDifferences":["Placeholder text"]}' | atk-ui-cli --project .
```

Use two PNGs from the same screen and interaction state. Paths may be absolute
or relative to the project. No saved session is required. The command writes an
offline HTML sheet with the two images side by side at equal display widths,
preserving their aspect ratios and showing their original dimensions. It also
returns the input paths, dimensions, and review steps as JSON so an agent can
open the originals directly.

`focus` and `expectedDifferences` are optional lists of notes for the reviewer.
The reviewer compares the images and reports evidence, priorities, and
uncertainties. The command does not generate a pixel diff, similarity score,
or pass/fail verdict.

## CLI command conventions

Send a JSON object through stdin, pass it with `--command JSON`, or read it with `--file FILE` or `--input FILE`. Send `{"command":"help"}` for command discovery.

Send a JSON array of 1 to 1,024 commands to run a batch. Use `--batch` to require array input. The CLI checks the command shape before it starts. Commands then run in order, with one JSON reply per attempted command. Execution stops at the first failure and earlier edits remain applied. Check the exit status and JSON replies before retrying.

Use `{"command":"help","all":true}` to list documentation topics. Read a topic with `{"command":"help","topic":"command-api"}` and pass the returned `next_offset` as `offset` until it is null.

Pass `--output FILE` for a single export. Relative paths use the CLI working directory. For a JSON request's `path` field, relative paths use the project directory. Set `path` on individual commands in a batch. With `session_render` and `--output`, the CLI infers PNG, HTML, or JSON from the filename when `format` is omitted.

Successful command replies include `ok: true` and a `result`. Failures exit with status 1 and report JSON on stdout. Standard export replies include `written.path` and `written.bytes` for requested output files.

Run commands locally through the CLI. Use `expectedRevision` on edits based on inspected state. Use the JSON `batch` command for an atomic group of design edits.

## Debug recordings and replay

Add `--debug` to a CLI command or batch to record it. The CLI prints the recording path to stderr and keeps command replies on stdout.

```powershell
'{"command":"session_inspect"}' | atk-ui-cli --session my-draft --debug
atk-ui-cli --replay PATH_TO_RECORDING.jsonl
```

Each invocation creates a JSONL file under `PROJECT/.ui/recordings/`. The file contains the initial saved state and a request, reply, and saved-state snapshot for each attempted command. A request without a reply means the outcome was not recorded. Inspect the saved project before retrying it.

Replay reads the recorded timeline without executing commands or restoring state. Add `--frame N` to print a single saved snapshot. Frame 0 is the initial state. Snapshots reflect saved data observed by the CLI, so another CLI writing concurrently can affect what a snapshot contains. Recording files contain the full request and saved-state data.
