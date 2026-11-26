# CLAUDE.md — Baking Powder

A multi-platform DAW in Rust. Every rule here is 35 words at most.

## Work

1. Write code, then commit and push it. Nothing else unless the owner asks: no tests, proofs, renders, screenshots, health checks or notes.
2. Don't write tests. Don't run tests. Don't run proofs. Don't do anything besides writing and pushing code.
3. Before pushing, run only `cargo check`; push nothing that fails it, so parallel agents never inherit a broken tree.
4. Spawn subagents for tasks instead of doing the work yourself, one agent per task, so each stays focused, works faster, and the overseer keeps its context for long conversations.
5. Run the test suite only when the owner asks for changes or a suite fix. When asked for tests, write many that define the feature, run only those, and make every probe one of them.
6. Don't edit this file unless the owner asks; flag ideas instead.
7. Replies are short and plain, and end with a numbered list of next steps or questions, 25 words each.

## Code

8. No code comments; write code that explains itself.
9. Alpha, no users: no backwards-compatible code or versioning.
10. Crates: `sdk` toolkit, `fx` effects, `coresynth` synth, `bp` the DAW; safe stable Rust, no `unsafe`.
11. Everything platform-agnostic lives in those crates; a shell only opens, forwards, paints.
12. Agent tooling stays in `mcp/` and `serve.rs`; agents use the same features people do.
13. No outside plugins: no AU, VST or CLAP; a wanted sound is built natively.

## Look

14. Drawing is rect fills in five tones: white, black, and black at .25/.5/.75 (`Tone`).
15. Out of face room: add a view, three at most, A, B and C.

## Sound

18. Playback must be flawless; live playing and recording are low priority.
19. Core aims for warm analogue hardware sound; CPU cost is not a primary concern.

## Text

20. Preset descriptions: twelve words at most, what the sound is.

## Map

21. `fx` and `coresynth` reach only `sdk`; `bp` re-exports all three.
22. Nothing outside this tree is this repo's job: never check, read, edit or plan for it unless asked. [index.md](index.md) lists those folders.

## Songs

23. A saved song holds every sound it uses, so it opens on any platform without a sample library; disk files never change it.

## Behaviour

24. No sometimes behaviour: an action always works or is never offered; never refuse it in some cases and allow it in others.

## Repos

25. A new repo the owner asks you to push is always created private.
