//! An effect plugin: the crate's feedback delay network reverb, stereo in and out.

use nih_plug::prelude::*;
use nih_plug_egui::{
    create_egui_editor, egui::Vec2, resizable_window::ResizableWindow, widgets, EguiState,
};
use std::sync::Arc;

use crate::synthesis::effects::ReverbFdn;

pub struct O3Reverb {
    params: Arc<O3ReverbParams>,
    reverb: ReverbFdn,
}

#[derive(Params)]
pub struct O3ReverbParams {
    /// Window size and DPI scale, saved with the plugin state.
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,

    #[id = "mix"]
    pub mix: FloatParam,
    #[id = "decay"]
    pub decay: FloatParam,
    #[id = "damping"]
    pub damping: FloatParam,
    #[id = "output"]
    pub output: FloatParam,
}

impl Default for O3ReverbParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(300, 200),
            mix: FloatParam::new("Mix", 0.3, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(10.0)),
            decay: FloatParam::new("Decay", 0.6, FloatRange::Linear { min: 0.0, max: 1.0 }),
            damping: FloatParam::new("Damping", 0.4, FloatRange::Linear { min: 0.0, max: 1.0 }),
            output: FloatParam::new("Output", 1.0, FloatRange::Linear { min: 0.0, max: 2.0 }),
        }
    }
}

impl Default for O3Reverb {
    fn default() -> Self {
        Self::with_sample_rate(48_000.0)
    }
}

impl O3Reverb {
    fn with_sample_rate(sample_rate: f32) -> Self {
        Self {
            params: Arc::new(O3ReverbParams::default()),
            reverb: ReverbFdn::new(sample_rate),
        }
    }
}

impl Plugin for O3Reverb {
    const NAME: &'static str = "O3DSP Reverb";
    const VENDOR: &'static str = "ArtisticMusician";
    const URL: &'static str = "https://github.com/ArtisticMusician/O3_DSP_crates";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

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
            ResizableWindow::new("o3dsp-reverb")
                .min_size(Vec2::new(280.0, 160.0))
                .show(ui_context, window_state.as_ref(), |ui| {
                    ui.heading("O3DSP Reverb");

                    for (label, param) in [
                        ("Mix", &params.mix),
                        ("Decay", &params.decay),
                        ("Damping", &params.damping),
                        ("Output", &params.output),
                    ] {
                        ui.label(label);
                        ui.add(widgets::ParamSlider::for_param(param, setter));
                    }
                });
        })
    }

    fn initialize(
        &mut self,
        _layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        *self = Self::with_sample_rate(buffer_config.sample_rate);
        true
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        self.reverb.set_decay(self.params.decay.value());
        self.reverb.set_damping(self.params.damping.value());

        let output = self.params.output.value();

        let slices = buffer.as_slice();
        if slices.len() < 2 {
            return ProcessStatus::Normal;
        }

        // Two channels, borrowed at once so the reverb can see a stereo pair per sample.
        let (first, rest) = slices.split_at_mut(1);
        let left = &mut *first[0];
        let (second, _) = rest.split_at_mut(1);
        let right = &mut *second[0];

        for frame in 0..left.len().min(right.len()) {
            self.reverb.set_mix(self.params.mix.smoothed.next());
            let (wet_left, wet_right) = self.reverb.process(left[frame], right[frame]);
            left[frame] = wet_left * output;
            right[frame] = wet_right * output;
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for O3Reverb {
    const CLAP_ID: &'static str = "com.artisticmusician.o3dsp.reverb";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Stereo FDN reverb built from the O3DSP FDN");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Stereo,
        ClapFeature::Reverb,
    ];
}

impl Vst3Plugin for O3Reverb {
    const VST3_CLASS_ID: [u8; 16] = *b"O3DSPReverb12345";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Fx, Vst3SubCategory::Reverb];
}
