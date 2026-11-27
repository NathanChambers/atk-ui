# atk-ui

Build UI mockups and navigation flows with an AI agent. Review the saved design on a local browser canvas.

## What you can do

- Author screens with text, images, styles, reusable components, and navigation hotspots.
- Review flows on a zoomable canvas and inspect layouts in color, grayscale, or an authored gaze overlay.
- Export PNG or HTML previews and hand off the design, assets, and reference frames as JSON.

## Install

Install a stable Rust toolchain. From the atk-ui source checkout, run:

```sh
cargo install --path . --locked
```

Cargo installs `atk-ui-cli` for your agent and `atk-ui` for the browser viewer. Both go in Cargo's bin directory, usually `~/.cargo/bin`.

## Use with an agent

Add the [`atk-ui` skill](.agents/skills/atk-ui/SKILL.md) using your agent's skill installation instructions. Open the agent in your project directory and give it a task.

> Mock up a settings screen and a confirmation dialog, connect them, and export a PNG preview.

## Open the viewer

Run `atk-ui` from your project directory. Open the printed URL to see a workspace picker that updates as workspaces are saved. Choose one to explore its flow, or use the folder button to switch.

## Learn more

- [Usage and command reference](docs/usage.md) for layouts, components, flows, exports, and debug recordings.

## Related tools

- [atk-mesh](https://github.com/NathanChambers/atk-mesh)
- [atk-audio](https://github.com/NathanChambers/atk-audio)

## License

[MIT](LICENSE)
