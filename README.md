# prism-fx

Audio effects written once, for two programs: **MultiGrainLive** (native Rust)
and **idMLab** in M Clone (Rust compiled to wasm32 in an AudioWorklet).

The design is in MultiGrainLive's `docs/SHARED-FX.md`. The effects come from
its `docs/HYPERPRISM.md` and `docs/EFFECT-INTAKE.md`.

- Every effect is a `Kernel`: stereo, in place, one frame at a time, with a
  block method that must give the same output.
- Every effect has a `Descriptor`: its parameters and its shared panel. Both
  hosts read that one table, so a range cannot drift between them.
- `contract` holds the rules every effect keeps, and the tests run them over
  the whole `CATALOG`: frames match blocks, block size does not matter, every
  control reaches the output, the output stays finite, reset is a fresh start,
  and nothing allocates after `new`.

No outside crates, no `unsafe`.

```
cargo test
cargo build --target wasm32-unknown-unknown --release
```

The catalogue is empty until the first effects land (Tremolo, Decimator, Echo).
