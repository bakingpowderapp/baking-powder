# Baking Powder

A DAW in Rust, with its own synth, effects and factory presets. No outside crates, and no `unsafe` outside the macOS shell.

An MCP server, off until you turn it on, lets AI agents use the same features people do.

Alpha: anything can change, saved songs included.

## Run

Needs an Apple Silicon Mac and stable Rust (built with 1.98).

    cargo run --release -p mac

## Crates

- `sdk`: the toolkit
- `fx`: the effects
- `coresynth`: the synth
- `bp`: the DAW, built on the three above
- `mac`: the macOS shell, and the reference for any other

A shell only opens, forwards and paints. To write one, depend on `bp` by path.

## Contributing

Issues and pull requests are welcome. Read `CLAUDE.md` first; it holds the house rules. For anything big, open an issue before writing code.

Baking Powder LLC decides what goes into main. Contributions are taken under the MIT licence.

## Licence

MIT. See `LICENSE`.
