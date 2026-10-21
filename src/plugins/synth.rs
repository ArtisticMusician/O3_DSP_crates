//! An instrument plugin: one band-limited oscillator through a one-pole lowpass and an ADSR.
//!
//! Notes arrive as MIDI, the gate drives the envelope, and the output is written to every channel
//! of the buffer.

use nih_plug::prelude::*;
use nih_plug_egui::{create_egui_editor, widgets, EguiState};
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

pub struct O3Synth {
    params: Arc<O3SynthParams>,
    oscillator: TableOscillator,
    filter: OnePoleFilter,
    envelope: AdsrEnvelope,
    frequency: f32,
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
            editor_state: EguiState::from_size(360, 300),
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
            wave: IntParam::new("Wave", 1, IntRange::Linear { min: 0, max: 3 }),
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
        }
    }

    fn waveform(&self) -> Waveform {
        let index = self.params.wave.value().clamp(0, 3);
        WAVEFORMS[index as usize]
    }

    /// One-pole coefficient for `cutoff` Hz: `1 - e^(-2*pi*fc/fs)`, clamped to a stable range.
    fn cutoff_coefficient(&self, sample_rate: f32) -> f32 {
        let cutoff = self.params.cutoff.value().clamp(20.0, sample_rate * 0.45);
        (1.0 - (-std::f32::consts::TAU * cutoff / sample_rate).exp()).clamp(0.0, 1.0)
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

        create_egui_editor(state, (), |_, _| {}, move |ui_context, setter, _state| {
            nih_plug_egui::egui::CentralPanel::default().show(ui_context, |ui| {
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
        // Rebuild the DSP at the rate the host actually runs at.
        *self = Self::with_sample_rate(buffer_config.sample_rate);
        true
    }

    fn reset(&mut self) {
        self.oscillator = TableOscillator::new(48_000.0);
        self.envelope = AdsrEnvelope::new(48_000.0);
        self.filter = OnePoleFilter::new();
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
                NoteEvent::NoteOn { note, velocity, .. } => {
                    self.frequency = 440.0 * 2.0f32.powf((note as f32 - 69.0) / 12.0);
                    self.oscillator.set_frequency(self.frequency * velocity.max(0.01));
                    self.envelope.gate_on();
                }
                NoteEvent::NoteOff { .. } => self.envelope.gate_off(),
                _ => (),
            }
        }

        self.envelope.set_parameters(
            self.params.attack.value(),
            self.params.decay.value(),
            self.params.sustain.value(),
            self.params.release.value(),
        );
        self.filter.set_lowpass(self.cutoff_coefficient(sample_rate));

        for channel_samples in buffer.iter_samples() {
            let gain = self.params.gain.smoothed.next();
            let level = self.envelope.next_coefficient();
            let sample =
                self.filter.process(self.oscillator.next_sample(self.waveform())) * level * gain;

            for channel in channel_samples {
                *channel = sample;
            }
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

    #[test]
    fn a_note_on_opens_the_envelope_and_note_off_closes_it() {
        let mut synth = O3Synth::default();
        synth.envelope.set_parameters(0.001, 0.001, 1.0, 0.001);

        synth.envelope.gate_on();
        let mut sounding = 0.0;
        for _ in 0..48_000 {
            sounding = synth.envelope.next_coefficient();
        }
        assert!(sounding > 0.9, "envelope should be open, got {sounding}");

        synth.envelope.gate_off();
        for _ in 0..48_000 {
            sounding = synth.envelope.next_coefficient();
        }
        assert!(sounding < 0.01, "envelope should be closed, got {sounding}");
    }

    #[test]
    fn the_cutoff_coefficient_stays_inside_the_stable_range() {
        let synth = O3Synth::default();
        for cutoff in [20.0, 440.0, 2_000.0, 20_000.0, 96_000.0] {
            let synth = O3Synth {
                params: {
                    let mut params = O3SynthParams::default();
                    params.cutoff = FloatParam::new(
                        "Cutoff",
                        cutoff,
                        FloatRange::Linear {
                            min: 20.0,
                            max: 96_000.0,
                        },
                    );
                    Arc::new(params)
                },
                ..synth
            };
            let coefficient = synth.cutoff_coefficient(48_000.0);
            assert!(
                (0.0..=1.0).contains(&coefficient),
                "cutoff {cutoff} produced {coefficient}"
            );
        }
    }
}
