# Bevy 0.19 companion fork

Branch `bevy-0.19` updates the Vello backend to 0.10 (wgpu 29), removes its
self-referential scene builder, and aligns HarfBuzz with the pinned Rive C++
runtime (`rive_10.1.0`). It is used by [G28XYZ/rive-bevy](https://github.com/G28XYZ/rive-bevy/tree/bevy-0.19).

Clone with `--recursive`, or run `git submodule update --init --recursive`.
Build this package with `cargo check -p rive-rs --no-default-features --features vello,text`.
The upstream workspace demos still use the old Vello API and are not ported.
Text is enabled by default; layout is no longer a default feature because the
upstream Yoga submodule is missing. Newer Rive features are not added by this port.
The C++ runtime revision is unchanged.

![CI](https://github.com/rive-app/rive-rs/actions/workflows/ci.yml/badge.svg)
![Discord badge](https://img.shields.io/discord/532365473602600965)
![Twitter handle](https://img.shields.io/twitter/follow/rive_app.svg?style=social&label=Follow)

# rive-rs

![Rive hero image](https://cdn.rive.app/rive_logo_dark_bg.png)

A Rust runtime library for [Rive](https://rive.app).

> [!NOTE]  
> This runtime uses [Vello](https://github.com/linebender/vello) as a render back-end, which has certain limitations. Refer to [Known Issues](#known-issues) for details. Efforts are underway to incorporate the [Rive Renderer](https://rive.app/renderer) as another back-end.

## Table of contents

- ⭐️ [Rive Overview](#rive-overview)
- 🚀 [Getting Started](#getting-started)
- 👨‍💻 [Contributing](#contributing)
- ❓ [Issues](#issues)

## Rive overview

[Rive](https://rive.app) is a real-time interactive design and animation tool that helps teams
create and run interactive animations anywhere. Designers and developers use our collaborative
editor to create motion graphics that respond to different states and user inputs. Our lightweight
open-source runtime libraries allow them to load their animations into apps, games, and websites.

🏡 [Homepage](https://rive.app/)

📘 [Rive Documentation](https://rive.app/community/doc)

🛠 [Rive Forums](https://rive.app/community/forums/home)

## Getting started

You will need a Rust toolchain and a C compiler to build. You can can install
the Rust toolchain using [rustup].

### Get submodules

To be able to compile the repo, you need to also clone/update the submodules:

```bash
$ git submodule update --init
```
...or, when cloning:

```bash
$ git clone --recurse-submodules git://github.com/rive-app/rive-rs.git
```

### Running the viewer

To open the included viewer, run:

```bash
$ cargo run --release
```

Then, drop any `.riv` file into the window to open it. Scroll to control the size of
the grid of copies.

[rustup]: https://rustup.rs

### Awesome Rive

For even more examples and resources on using Rive at runtime or in other tools, checkout the [awesome-rive](https://github.com/rive-app/awesome-rive) repo.

See the [rive-bevy repository](https://github.com/rive-app/rive-bevy) that makes use of this runtime.

## Contributing

We love contributions! Check out our [contributing docs](./CONTRIBUTING.md) to get more details into
how to run this project, the examples, and more all locally.

## Issues

Have an issue with using the runtime, or want to suggest a feature/API to help make your development
life better? Log an issue in our [issues](https://github.com/rive-app/rive-rs/issues) tab! You
can also browse older issues and discussion threads there to see solutions that may have worked for
common problems.

### Known issues

The existing [Vello](https://github.com/linebender/vello) render back-end may lead to some inconsistencies in comparison to the original design:

- Image meshes: They can exhibit small inconsistencies at triangle borders, there can be gaps between triangles, transparent meshes will overdraw at triangle borders.
- Very high number of clips: Vello is currently rendering very high numbers of clips incorrectly.
- All strokes will have round joins and caps.

Efforts are being made to make the [Rive Renderer](https://rive.app/renderer) available. You'll then have the choice to select your preferred renderer.

