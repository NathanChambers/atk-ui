# ATK UI

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

Run `atk-ui` from your project directory and open the [UI viewer](http://127.0.0.1:7881). Once your agent has saved a workspace, choose it with the folder button. From there, pan and zoom around the flow, then select a transition to focus its destination.

## Learn more

- [Usage and command reference](docs/usage.md) for layouts, components, flows, exports, and debug recordings.

## Related tools

- [ATK Mesh](https://github.com/NathanChambers/atk-mesh)
- [ATK Audio](https://github.com/NathanChambers/atk-audio)

## License

[MIT](LICENSE)
