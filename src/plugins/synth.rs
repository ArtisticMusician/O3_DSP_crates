//! An instrument plugin: one band-limited oscillator through a one-pole lowpass and an ADSR.
//!
//! Notes arrive as MIDI, the gate drives the envelope, and the output is written to every channel
//! of the buffer.

use nih_plug::prelude::*;
use nih_plug_egui::{
    create_egui_editor, egui::Vec2, resizable_window::ResizableWindow, widgets, EguiState,
};
use std::sync::Arc;

use crate::synthesis::envelopes::AdsrEnvelope;
use crate::synthesis::filters::OnePoleFilter;
use crate::synthesis::oscillators::polyblep::{TableOscillator, Waveform};

const WAVEFORMS: [Waveform; 4] = [
    Waveform::Sine,
    Waveform::Sawtooth,
    Waveform::Square,
    Waveform::Triangle,
];

#[cfg(feature = "diag")]
fn diag(message: &str) {
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("D:/O3DSP_diag.log")
    {
        let _ = writeln!(file, "{message}");
    }
}

#[cfg(not(feature = "diag"))]
fn diag(_message: &str) {}

static BLOCKS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub struct O3Synth {
    params: Arc<O3SynthParams>,
    oscillator: TableOscillator,
    filter: OnePoleFilter,
    envelope: AdsrEnvelope,
    frequency: f32,
    velocity: f32,
    sample_rate: f32,
}

#[derive(Params)]
pub struct O3SynthParams {
    /// Window size and DPI scale, saved with the plugin state.
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,

    #[id = "gain"]
    pub gain: FloatParam,
    #[id = "cutoff"]
    pub cutoff: FloatParam,
    #[id = "attack"]
    pub attack: FloatParam,
    #[id = "decay"]
    pub decay: FloatParam,
    #[id = "sustain"]
    pub sustain: FloatParam,
    #[id = "release"]
    pub release: FloatParam,
    #[id = "wave"]
    pub wave: IntParam,
}

impl Default for O3SynthParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(360, 400),
            gain: FloatParam::new("Gain", 0.5, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(20.0))
                .with_unit(" gain"),
            cutoff: FloatParam::new(
                "Cutoff",
                2_000.0,
                FloatRange::Skewed {
                    min: 20.0,
                    max: 20_000.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" Hz"),
            attack: FloatParam::new("Attack", 0.01, FloatRange::Linear { min: 0.0, max: 2.0 })
                .with_unit(" s"),
            decay: FloatParam::new("Decay", 0.2, FloatRange::Linear { min: 0.0, max: 2.0 })
                .with_unit(" s"),
            sustain: FloatParam::new("Sustain", 0.7, FloatRange::Linear { min: 0.0, max: 1.0 }),
            release: FloatParam::new("Release", 0.4, FloatRange::Linear { min: 0.0, max: 5.0 })
                .with_unit(" s"),
            wave: IntParam::new("Wave", 1, IntRange::Linear { min: 0, max: 3 })
                .with_value_to_string(Arc::new(|index: i32| {
                    match index {
                        0 => "Sine",
                        1 => "Saw",
                        2 => "Square",
                        _ => "Triangle",
                    }
                    .to_string()
                })),
        }
    }
}

impl Default for O3Synth {
    fn default() -> Self {
        Self::with_sample_rate(48_000.0)
    }
}

impl O3Synth {
    fn with_sample_rate(sample_rate: f32) -> Self {
        let params = Arc::new(O3SynthParams::default());
        Self {
            params,
            oscillator: TableOscillator::new(sample_rate),
            filter: OnePoleFilter::new(),
            envelope: AdsrEnvelope::new(sample_rate),
            frequency: 440.0,
            velocity: 1.0,
            sample_rate,
        }
    }

    /// Rebuild the DSP for a new sample rate. `params` is deliberately left alone: the host and
    /// the editor hold their own `Arc` to it, and replacing it silently breaks automation.
    fn prepare(&mut self, sample_rate: f32) {
        self.oscillator = TableOscillator::new(sample_rate);
        self.envelope = AdsrEnvelope::new(sample_rate);
        self.filter = OnePoleFilter::new();
        self.sample_rate = sample_rate;
    }

    /// Start a note: pitch comes from the MIDI note, level from the velocity.
    fn note_on(&mut self, note: u8, velocity: f32) {
        diag(&format!("note on: {note} velocity {velocity}"));
        self.frequency = 440.0 * 2.0f32.powf((note as f32 - 69.0) / 12.0);
        self.velocity = velocity.clamp(0.0, 1.0);
        self.oscillator.set_frequency(self.frequency);
        self.envelope.gate_on();
    }

    /// Push the parameter block into the DSP. Runs once per block, before the sample loop.
    fn apply_parameters(&mut self) {
        // The host re-targets this smoother on automation; aiming it at the current value here
        // keeps the audio path correct even before the first automation event arrives.
        self.params
            .gain
            .smoothed
            .set_target(self.sample_rate, self.params.gain.value());
        self.envelope.set_parameters(
            self.params.attack.value(),
            self.params.decay.value(),
            self.params.sustain.value(),
            self.params.release.value(),
        );
        self.filter.set_lowpass(self.normalised_cutoff(self.sample_rate));
    }

    /// One sample of the chain: oscillator -> one-pole -> envelope -> velocity -> gain.
    #[inline]
    fn next_sample(&mut self) -> f32 {
        let gain = self.params.gain.smoothed.next();
        let level = self.envelope.next_coefficient();
        self.filter.process(self.oscillator.next_sample(self.waveform())) * level * self.velocity * gain
    }

    fn waveform(&self) -> Waveform {
        let index = self.params.wave.value().clamp(0, 3);
        WAVEFORMS[index as usize]
    }

    /// Normalised one-pole cutoff for `cutoff` Hz. `OnePoleFilter::set_lowpass` takes a fraction
    /// of the sample rate (0.0 to 0.5), not a coefficient.
    fn normalised_cutoff(&self, sample_rate: f32) -> f32 {
        let cutoff = self.params.cutoff.value().clamp(20.0, sample_rate * 0.45);
        (cutoff / sample_rate).clamp(0.0, 0.5)
    }
}

impl Plugin for O3Synth {
    const NAME: &'static str = "O3DSP Synth";
    const VENDOR: &'static str = "ArtisticMusician";
    const URL: &'static str = "https://github.com/ArtisticMusician/O3_DSP_crates";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: None,
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        let params = self.params.clone();
        let state = self.params.editor_state.clone();
        let window_state = state.clone();

        create_egui_editor(state, (), |_, _| {}, move |ui_context, setter, _state| {
            ResizableWindow::new("o3dsp-synth")
                .min_size(Vec2::new(280.0, 200.0))
                .show(ui_context, window_state.as_ref(), |ui| {
                    ui.heading("O3DSP Synth");

                    for (label, param) in [
                        ("Gain", &params.gain),
                        ("Cutoff", &params.cutoff),
                        ("Attack", &params.attack),
                        ("Decay", &params.decay),
                        ("Sustain", &params.sustain),
                        ("Release", &params.release),
                    ] {
                        ui.label(label);
                        ui.add(widgets::ParamSlider::for_param(param, setter));
                    }

                    ui.label("Wave");
                    ui.add(widgets::ParamSlider::for_param(&params.wave, setter));
                });
        })
    }

    fn initialize(
        &mut self,
        _layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.prepare(buffer_config.sample_rate);
        diag(&format!("initialize: sample_rate {}", buffer_config.sample_rate));
        true
    }

    fn reset(&mut self) {
        let sample_rate = self.sample_rate;
        self.prepare(sample_rate);
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let sample_rate = context.transport().sample_rate as f32;

        while let Some(event) = context.next_event() {
            match event {
                NoteEvent::NoteOn { note, velocity, .. } => self.note_on(note, velocity),
                NoteEvent::NoteOff { .. } => {
                    diag("note off");
                    self.envelope.gate_off();
                }
                _ => (),
            }
        }

        self.apply_parameters();

        let mut peak = 0.0f32;
        for channel_samples in buffer.iter_samples() {
            let sample = self.next_sample();
            peak = peak.max(sample.abs());

            for channel in channel_samples {
                *channel = sample;
            }
        }

        let block = BLOCKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if block < 3 || block % 100 == 0 {
            diag(&format!(
                "block {block}: channels {} samples {} peak {peak:.5} cutoff {:.4} freq {:.1}",
                buffer.channels(),
                buffer.samples(),
                self.normalised_cutoff(sample_rate),
                self.frequency
            ));
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for O3Synth {
    const CLAP_ID: &'static str = "com.artisticmusician.o3dsp.synth";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Single-oscillator synth built from the O3DSP components");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::Instrument,
        ClapFeature::Synthesizer,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for O3Synth {
    const VST3_CLASS_ID: [u8; 16] = *b"O3DSPSynth123456";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Instrument];
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: f32 = 48_000.0;

    fn render(synth: &mut O3Synth, samples: usize) -> Vec<f32> {
        (0..samples).map(|_| synth.next_sample()).collect()
    }

    fn rms(buffer: &[f32]) -> f32 {
        (buffer.iter().map(|s| s * s).sum::<f32>() / buffer.len() as f32).sqrt()
    }

    /// Upward zero crossings per second. Independent of the FFT, so it cannot share a bug with it.
    fn frequency(buffer: &[f32], sample_rate: f32) -> f32 {
        let crossings = buffer.windows(2).filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0).count();
        crossings as f32 * sample_rate / buffer.len() as f32
    }

    fn sounding_synth() -> O3Synth {
        let mut synth = O3Synth::default();
        synth.prepare(SAMPLE_RATE);
        synth.apply_parameters();
        synth
    }

    #[test]
    fn a_note_on_produces_audio() {
        let mut synth = sounding_synth();
        synth.note_on(69, 1.0);

        let output = render(&mut synth, 4_800);
        let peak = output.iter().fold(0.0f32, |acc, s| acc.max(s.abs()));

        assert!(peak > 0.05, "a note on produced silence, peak {peak}");
        assert!(rms(&output) > 0.02, "rms {} from a note on", rms(&output));
    }

    #[test]
    fn a_note_off_silences_the_synth() {
        let mut synth = sounding_synth();
        synth.note_on(69, 1.0);
        render(&mut synth, 4_800);

        synth.envelope.gate_off();
        render(&mut synth, SAMPLE_RATE as usize);
        let tail = render(&mut synth, 4_800);

        assert!(rms(&tail) < 0.01, "release left rms {}", rms(&tail));
    }

    #[test]
    fn pitch_comes_from_the_note_and_never_from_the_velocity() {
        // Velocity used to be multiplied into the oscillator frequency, so a quiet note played
        // flat. Both velocities must give the same pitch.
        let mut quiet = sounding_synth();
        quiet.note_on(69, 0.2);
        let mut loud = sounding_synth();
        loud.note_on(69, 1.0);

        let quiet_output = render(&mut quiet, 24_000);
        let loud_output = render(&mut loud, 24_000);

        let quiet_pitch = frequency(&quiet_output, SAMPLE_RATE);
        let loud_pitch = frequency(&loud_output, SAMPLE_RATE);

        assert!(
            (quiet_pitch - loud_pitch).abs() < 1.0,
            "velocity changed the pitch: {} Hz at 0.2 vs {} Hz at 1.0",
            quiet_pitch,
            loud_pitch
        );
        assert!(
            (quiet_pitch - 440.0).abs() < 5.0,
            "note 69 should be 440 Hz, measured {quiet_pitch}"
        );
    }

    #[test]
    fn velocity_scales_the_level() {
        let mut quiet = sounding_synth();
        quiet.note_on(69, 0.25);
        let mut loud = sounding_synth();
        loud.note_on(69, 1.0);

        let quiet_rms = rms(&render(&mut quiet, 4_800));
        let loud_rms = rms(&render(&mut loud, 4_800));

        assert!(
            loud_rms > quiet_rms * 2.0,
            "velocity 1.0 ({loud_rms}) should be far louder than 0.25 ({quiet_rms})"
        );
    }

    #[test]
    fn cutoff_is_a_fraction_of_the_sample_rate() {
        // `OnePoleFilter::set_lowpass` takes f/fs, not a pre-computed coefficient. The double
        // conversion that shipped first turned 2 kHz into ~11 kHz.
        let synth = sounding_synth();
        let default_cutoff = synth.normalised_cutoff(SAMPLE_RATE);

        assert!(
            (default_cutoff - 2_000.0 / SAMPLE_RATE).abs() < 1e-6,
            "2 kHz at 48 kHz should be {} , got {default_cutoff}",
            2_000.0 / SAMPLE_RATE
        );
    }

    #[test]
    fn cutoff_never_leaves_the_stable_range() {
        for cutoff in [20.0, 440.0, 2_000.0, 20_000.0, 96_000.0] {
            let mut synth = sounding_synth();
            synth.params = Arc::new(O3SynthParams {
                cutoff: FloatParam::new(
                    "Cutoff",
                    cutoff,
                    FloatRange::Linear { min: 20.0, max: 96_000.0 },
                ),
                ..O3SynthParams::default()
            });
            synth.apply_parameters();

            let normalised = synth.normalised_cutoff(SAMPLE_RATE);
            assert!(
                (0.0..=0.5).contains(&normalised),
                "cutoff {cutoff} produced {normalised}"
            );
        }
    }

    #[test]
    fn a_higher_cutoff_passes_more_high_end() {
        // Behavioural counterpart to the mapping test: if the cutoff is mis-scaled the two
        // settings stop being distinguishable at the top end.
        let brightness = |cutoff: f32| {
            let mut synth = sounding_synth();
            synth.note_on(81, 1.0); // 880 Hz
            synth.params = Arc::new(O3SynthParams {
                cutoff: FloatParam::new(
                    "Cutoff",
                    cutoff,
                    FloatRange::Linear { min: 20.0, max: 20_000.0 },
                ),
                ..O3SynthParams::default()
            });
            synth.apply_parameters();

            let output = render(&mut synth, 24_000);
            // Slope energy relative to signal energy: a shape measure, so the filter's level
            // change cannot hide the difference.
            let slope: f32 = output.windows(2).map(|pair| (pair[1] - pair[0]).powi(2)).sum();
            let energy: f32 = output.iter().map(|sample| sample * sample).sum();
            (slope / energy).sqrt()
        };

        let dark = brightness(300.0);
        let bright = brightness(12_000.0);
        assert!(bright > dark * 2.0, "cutoff 12 kHz ({bright}) should be brighter than 300 Hz ({dark})");
    }

    #[test]
    fn rebuilding_for_a_new_sample_rate_keeps_the_parameter_block() {
        // `initialize()` used to do `*self = Self::with_sample_rate(..)`, which swapped in a fresh
        // `Arc<Params>`; the host and the editor kept pointing at the old one, so automation and
        // the GUI wrote to a parameter block the audio path never read.
        let mut synth = O3Synth::default();
        let before = synth.params.clone();

        synth.prepare(96_000.0);

        assert!(
            Arc::ptr_eq(&before, &synth.params),
            "prepare() replaced the parameter block"
        );
        assert_eq!(synth.sample_rate, 96_000.0);
    }

    #[test]
    fn the_editor_builds_a_window_state() {
        // Guards the cut-off-control failure: a plugin with no editor opens nothing, and the
        // window size has to leave room for every control.
        let synth = O3Synth::default();
        let size = synth.params.editor_state.size();
        assert!(size.0 >= 300 && size.1 >= 360, "editor window is too small: {size:?}");
    }
}
