# O3DSP

A dependency-free DSP library for Rust: oscillators, filters, effects, dynamics, envelopes,
spectral analysis, audio file I/O, and a static signal graph. Everything is written from scratch
against `std` — no external crates in the default build.

```toml
[dependencies]
o3dsp = { path = "../O3_DSP_crates-main" }
```

Stable Rust, edition 2021. No nightly features, no `std::simd`.

## What is in it

| Area | Components |
|---|---|
| Oscillators | PolyBLEP wavetable, multi-frame wavetable with octave mipmaps and unison, FM operator, Karplus-Strong, supersaw, granular, phase distortion, vector, physical modelling, noise (white/pink/brown/Perlin/fBm), additive (`HarmonicSpectrum`, `SpectrumMorph`, `AdditiveOscillator`), sine bank |
| Filters | Biquad, one-pole, state variable, comb, allpass, DC blocker, FIR, shelf, parametric EQ, biquad bank / graphic EQ, Sallen-Key, Moog ladder, diode ladder, ladder, formant, Linkwitz-Riley crossover, filter banks |
| Effects | Chorus, chorus/flanger, mono feedback delay, phaser, ring modulator, saturation, wavefolder, bitcrusher, Schroeder reverb, FDN reverb, stereo width |
| Dynamics | Drum compressor, vocal compressor, SSL-style glue compressor, gate/expander, peak limiter, lookahead limiter |
| Envelopes | ADSR, AR/AHR, DAHDSR, envelope follower, loopable, random, transient shaper |
| Modulation | LFO, audio-rate modulator, sample & hold, step sequencer, XY pad, chaos generators |
| Metering | Peak, RMS, VU, LUFS (ITU-R BS.1770 with gating) |
| Spectral | FFT, spectral analyser with partial extraction, additive resynthesis |
| Audio I/O | WAV and AIFF decoding (8/16/24/32-bit PCM, 32/64-bit float), `AudioBuffer`, offline editing (trim, cut, insert, fade, gain, normalise, reverse, crossfade, concatenate, silence detection, DC removal) |
| Routing | `ProcessorChain`, `ParallelProcessor`, `AudioGraph` (topological, stereo, zero-allocation after `compile`), `ProcessorIterator` |
| Infrastructure | Parameter smoother, cache-line padding, lock-free SPSC ring buffer, frame pool, Hermite interpolation and resampling, Bezier curves, SIMD channel mixing (AVX2 + scalar fallback) |

## Design rules the code follows

- **No dependencies.** `cargo tree` prints the crate and nothing else. `dasp` is optional behind
  `dasp_support`, for the `Signal` adapters.
- **No allocation after construction.** Buffers are sized in constructors and in
  `AudioGraph::compile`. This is measured, not asserted: `tests/no_alloc.rs` installs a counting
  allocator and fails if `LufsMeter`, `SineBank`, `AdditiveOscillator` or `AudioGraph` touch the
  heap while processing.
- **Real-time safe hot paths.** No locks, no file I/O, and no `Vec` growth in `process_sample`;
  the allocation test covers the components above, not every type in the crate.
- **Tests that can fail.** 280 test functions (~4,000 lines): 274 run in the library by default,
  5 more in the allocation integration test. No tautological assertions; filters, effects and
  meters are measured, not eyeballed.

## Building and testing

```
cargo build --release
cargo test
cargo test --features dasp_support
cargo doc --no-deps
```

The library has no binary or plugin target — `crate-type = ["lib"]`.

## Not in here

- No plugin framework, no VST3/CLAP/AU export, and no GUI. A plugin is a separate crate that
  depends on this one.
- No sample playback / voice allocation, no modulation matrix, no preset system.
- Sample-rate conversion exists only as `HermiteResampler`; `audio/edit.rs` deliberately does not
  resample.

## Licence

Proprietary. All rights reserved; no permission to use, copy, modify or distribute is granted. See
[LICENSE](LICENSE). Commercial use requires a separate written agreement.
