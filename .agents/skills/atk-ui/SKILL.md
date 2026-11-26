---
name: atk-ui
description: Use atk-ui to author, inspect, render, or export UI mockups and navigation flows. Apply when the user invokes $atk-ui, requests work on an atk-ui workspace, or asks to use atk-ui-cli or atk-ui.
---

# atk-ui

Use `atk-ui-cli --project DIR --session ID` when choosing or switching targets during a task. Use MCP tools already attached to the intended project and session when available. Follow an explicit user preference for either transport. If atk-ui cannot perform a required operation, explain the limitation and ask before switching to external authoring.

## Select the project and session

- Check `atk-ui-cli --help` and `atk-ui --help`. If the tools are unavailable on PATH, run `cargo build --locked` in the atk-ui source checkout and use its `target/debug` binaries (`.exe` on Windows).
- Run the tools from the intended project directory or pass `--project DIR`. Use `project_list` to find saved sessions under `.ui/sessions/`.
- Pass `--session ID` for session commands. Use `session_start` with a fresh ID for a new mockup or `session_duplicate` to branch an existing design.
- When configuring a stdio MCP host, launch `atk-ui-cli --mcp --project DIR --session ID`. Use a separate process for each MCP target.

## Inspect and edit

- Send `{"command":"help"}` for the current command contract. Pass JSON via stdin or `--file FILE`. In PowerShell, use `'{"command":"project_list"}' | atk-ui-cli --project .`.
- Use `session_inspect` to read the saved design and revision before editing. Use `session_manifest` to locate screens, flow frames, transitions, and asset dependencies without loading asset payloads.
- Use `element_patch` for targeted edits and `component_element_patch` for changes shared by component instances. Geometry uses design pixels. Later elements paint above earlier ones.
- Put `expectedRevision` on writes that depend on inspected state. Use a `batch` command for related edits that must commit together and put the revision guard on the batch itself. Use MCP `run_batch` or a CLI JSON array for ordered commands with known inputs. Earlier successful steps remain applied if a later step fails. Check the replies before retrying.
- Use `session_validate` after authoring changes. Flow edges must match actions that reach their destination frames.

## Preview and deliver

- Use `session_render` with `format: "png"` and a fresh `path` for visual review. Select `screen` for an authored screen or `frame` to replay a saved flow path. Open the PNG when visual evidence matters.
- Use `preview_action` with a target from the default JSON render to exercise a transition or control. Use `preview_reset` to return the CLI preview to the entry state. Design edits invalidate the CLI preview.
- Use `session_export` with a fresh `.html` path when an offline preview is requested.
- For a browser preview, run `atk-ui --project DIR` and share the URL it prints. Use `--port PORT` if the default port `7881` is occupied. The read-only viewer reads saved sessions independently of the CLI preview cursor.
- Report the session ID and requested artifact paths.
