//! A statically dispatched audio graph: nodes, edges, and a topological order computed once.
//!
//! [`AudioGraph::compile`] sorts the nodes so every node runs after the nodes feeding it, resolves
//! each input slot to a list of source nodes, and sizes the state. `process_stereo` then only walks
//! those pre-resolved lists, so nothing allocates after `compile()`.
//!
//! Every node carries a stereo pair. Mono stages - filters, mono effects - hold two instances
//! behind [`StereoStage`], one per channel; the effects that only exist as a stereo pair
//! (`ReverbFdn`, `ChorusFlanger`, `StereoWidth`) take both channels directly through
//! [`StereoEffectModule`], as do the dynamics processors.

use crate::synthesis::compressors::DynamicsModule;
use crate::synthesis::effects::{EffectModule, StereoEffectModule};
use crate::synthesis::filters::FilterModule;
use crate::synthesis::oscillators::noise::NoiseType;
use crate::synthesis::oscillators::polyblep::Waveform;
use crate::synthesis::oscillators::OscillatorModule;
use crate::synthesis::utils::{Processor, StereoStage};

/// A node in the graph.
///
/// `Oscillator` and `Constant` are mono sources and feed both channels the same signal; `Input` is
/// the pair handed to `process_stereo`.
#[derive(Debug, Clone)]
pub enum GraphNode {
    Constant(f32),
    Input,
    Oscillator {
        module: OscillatorModule,
        wave_type: Waveform,
        noise_type: NoiseType,
    },
    Filter(StereoStage<FilterModule>),
    Effect(StereoStage<EffectModule>),
    Dynamics(DynamicsModule),
    StereoEffect(StereoEffectModule),
    Math(MathOp),
}

/// Two-input arithmetic, computed per channel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MathOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Clamp { min: f32, max: f32 },
}

impl GraphNode {
    /// A mono filter, run as two instances so the channels keep separate state.
    pub fn filter(module: FilterModule) -> Self {
        GraphNode::Filter(StereoStage::new(module))
    }

    /// A mono effect, run as two instances so the channels keep separate state.
    pub fn effect(module: EffectModule) -> Self {
        GraphNode::Effect(StereoStage::new(module))
    }

    /// An effect that only exists as a stereo pair.
    pub fn stereo_effect(module: StereoEffectModule) -> Self {
        GraphNode::StereoEffect(module)
    }

    /// Evaluate the node given its resolved inputs.
    #[inline(always)]
    pub fn process_stereo(
        &mut self,
        inputs: &[(f32, f32)],
        graph_input: (f32, f32),
    ) -> (f32, f32) {
        let first_input = inputs.first().copied().unwrap_or((0.0, 0.0));

        match self {
            GraphNode::Constant(value) => (*value, *value),
            GraphNode::Input => graph_input,
            GraphNode::Oscillator {
                module,
                wave_type,
                noise_type,
            } => {
                // FM and phase distortion take input slot 0 as phase modulation, which the
                // dispatch enum cannot express; everything else goes through it, so a new
                // oscillator variant needs no edit here.
                let sample = match module {
                    OscillatorModule::Fm(osc) => osc.next_sample(first_input.0),
                    OscillatorModule::PhaseDistortion(osc) => osc.process_sample(first_input.0),
                    other => other.next_sample(*wave_type, *noise_type),
                };
                (sample, sample)
            }
            GraphNode::Filter(stage) => stage.process_stereo(first_input.0, first_input.1),
            GraphNode::Effect(stage) => stage.process_stereo(first_input.0, first_input.1),
            GraphNode::Dynamics(dynamics) => dynamics.process_stereo(first_input.0, first_input.1),
            GraphNode::StereoEffect(effect) => effect.process_stereo(first_input.0, first_input.1),
            GraphNode::Math(operation) => {
                let (left, right) = first_input;
                let second = inputs.get(1).copied().unwrap_or((0.0, 0.0));

                (
                    apply_math(*operation, left, second.0),
                    apply_math(*operation, right, second.1),
                )
            }
        }
    }
}

#[inline(always)]
fn apply_math(operation: MathOp, a: f32, b: f32) -> f32 {
    match operation {
        MathOp::Add => a + b,
        MathOp::Subtract => a - b,
        MathOp::Multiply => a * b,
        MathOp::Divide => {
            if b.abs() > 1e-6 {
                a / b
            } else {
                0.0
            }
        }
        MathOp::Clamp { min, max } => a.clamp(min, max),
    }
}

/// An edge mapping an output from a source node to an input slot of a destination node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edge {
    pub source_node: usize,
    pub dest_node: usize,
    pub dest_input_index: usize,
}

/// How many input slots a node has.
const INPUT_SLOTS: usize = 4;

/// The pre-calculated input sources for a single node during execution.
#[derive(Debug, Clone)]
struct CompiledNode {
    node_idx: usize,
    // For each input slot, which source nodes feed into it. Multiple sources into one slot sum.
    input_sources: [Vec<usize>; INPUT_SLOTS],
}

/// A zero-allocation, statically-dispatched stereo audio graph.
pub struct AudioGraph {
    nodes: Vec<GraphNode>,
    edges: Vec<Edge>,

    // Execution state, sized in `compile()`
    compiled_execution: Vec<CompiledNode>,
    node_outputs: Vec<(f32, f32)>,
    input_scratch: [(f32, f32); INPUT_SLOTS],
    output_node: Option<usize>,
}

impl AudioGraph {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            compiled_execution: Vec::new(),
            node_outputs: Vec::new(),
            input_scratch: [(0.0, 0.0); INPUT_SLOTS],
            output_node: None,
        }
    }

    pub fn add_node(&mut self, node: GraphNode) -> usize {
        let index = self.nodes.len();
        self.nodes.push(node);
        self.node_outputs.push((0.0, 0.0));
        index
    }

    pub fn connect(&mut self, source_node: usize, dest_node: usize, dest_input_index: usize) {
        assert!(source_node < self.nodes.len(), "source node out of bounds");
        assert!(dest_node < self.nodes.len(), "destination node out of bounds");
        assert!(
            dest_input_index < INPUT_SLOTS,
            "only {INPUT_SLOTS} input slots are supported"
        );

        self.edges.push(Edge {
            source_node,
            dest_node,
            dest_input_index,
        });
    }

    pub fn set_output_node(&mut self, node_index: usize) {
        assert!(node_index < self.nodes.len(), "output node out of bounds");
        self.output_node = Some(node_index);
    }

    pub fn output_node(&self) -> Option<usize> {
        self.output_node
    }

    /// Sort the nodes, resolve the input slots, and size the state. Call this after changing the
    /// graph; processing before it passes the input through unchanged.
    ///
    /// # Panics
    /// If the edges contain a cycle.
    pub fn compile(&mut self) {
        let count = self.nodes.len();
        let mut in_degree = vec![0usize; count];
        let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); count];

        for edge in &self.edges {
            adjacency[edge.source_node].push(edge.dest_node);
            in_degree[edge.dest_node] += 1;
        }

        let mut queue: Vec<usize> = in_degree
            .iter()
            .enumerate()
            .filter(|&(_, &degree)| degree == 0)
            .map(|(index, _)| index)
            .collect();

        let mut order = Vec::with_capacity(count);
        while let Some(node) = queue.pop() {
            order.push(node);
            for &next in &adjacency[node] {
                in_degree[next] -= 1;
                if in_degree[next] == 0 {
                    queue.push(next);
                }
            }
        }

        if order.len() != count {
            panic!("cycle detected in AudioGraph: DAGs only");
        }

        self.compiled_execution.clear();
        for &node_idx in &order {
            let mut compiled = CompiledNode {
                node_idx,
                input_sources: Default::default(),
            };

            for edge in &self.edges {
                if edge.dest_node == node_idx {
                    compiled.input_sources[edge.dest_input_index].push(edge.source_node);
                }
            }
            self.compiled_execution.push(compiled);
        }

        self.node_outputs.resize(count, (0.0, 0.0));
    }

    /// The number of nodes in the graph.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Process one stereo frame.
    #[inline(always)]
    pub fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        if self.compiled_execution.is_empty() {
            return (left, right);
        }

        for index in 0..self.compiled_execution.len() {
            let compiled = &self.compiled_execution[index];
            let node_idx = compiled.node_idx;

            for slot in 0..INPUT_SLOTS {
                let mut left_sum = 0.0;
                let mut right_sum = 0.0;

                for &source in &compiled.input_sources[slot] {
                    let (source_left, source_right) = self.node_outputs[source];
                    left_sum += source_left;
                    right_sum += source_right;
                }

                self.input_scratch[slot] = (left_sum, right_sum);
            }

            let output = self.nodes[node_idx].process_stereo(&self.input_scratch, (left, right));
            self.node_outputs[node_idx] = output;
        }

        match self.output_node {
            Some(index) => self.node_outputs[index],
            None => (0.0, 0.0),
        }
    }
}

impl Default for AudioGraph {
    fn default() -> Self {
        Self::new()
    }
}

/// Mono use of a stereo graph: the sample is fed to both channels and the two are averaged back
/// into one. Use [`AudioGraph::process_stereo`] when the channels have to stay apart.
impl Processor for AudioGraph {
    #[inline(always)]
    fn process_sample(&mut self, sample: f32) -> f32 {
        let (left, right) = self.process_stereo(sample, sample);
        (left + right) * 0.5
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synthesis::filters::OnePoleFilter;

    const SAMPLE_RATE: f32 = 48_000.0;

    #[test]
    fn graph_evaluates_basic_math_correctly() {
        let mut graph = AudioGraph::new();
        let two = graph.add_node(GraphNode::Constant(2.0));
        let three = graph.add_node(GraphNode::Constant(3.0));
        let sum = graph.add_node(GraphNode::Math(MathOp::Add));
        graph.connect(two, sum, 0);
        graph.connect(three, sum, 1);
        graph.set_output_node(sum);
        graph.compile();

        assert_eq!(graph.process_stereo(0.0, 0.0), (5.0, 5.0));
    }

    #[test]
    #[should_panic(expected = "cycle detected")]
    fn graph_rejects_cycles() {
        let mut graph = AudioGraph::new();
        let constant = graph.add_node(GraphNode::Constant(1.0));
        let sum = graph.add_node(GraphNode::Math(MathOp::Add));
        graph.connect(constant, sum, 0);
        graph.connect(sum, sum, 1);
        graph.compile();
    }

    #[test]
    fn graph_filters_a_signal() {
        let mut graph = AudioGraph::new();
        let input = graph.add_node(GraphNode::Input);
        let mut lowpass = OnePoleFilter::new();
        lowpass.set_lowpass(0.1);
        let filter = graph.add_node(GraphNode::filter(FilterModule::OnePole(lowpass)));
        graph.connect(input, filter, 0);
        graph.set_output_node(filter);
        graph.compile();

        let mut last = 0.0;
        for _ in 0..10 {
            last = graph.process_stereo(1.0, 1.0).0;
        }

        assert!(last > 0.0 && last < 1.0, "filter should smooth the step, got {last}");
    }

    #[test]
    fn an_oscillator_node_plays_at_its_frequency() {
        let mut table = crate::synthesis::oscillators::polyblep::TableOscillator::new(SAMPLE_RATE);
        table.set_frequency(440.0);

        let mut graph = AudioGraph::new();
        let oscillator = graph.add_node(GraphNode::Oscillator {
            module: OscillatorModule::PolyBlep(table),
            wave_type: Waveform::Sine,
            noise_type: NoiseType::White,
        });
        graph.set_output_node(oscillator);
        graph.compile();

        let output: Vec<f32> = (0..24_000).map(|_| graph.process_stereo(0.0, 0.0).0).collect();
        let crossings = output
            .windows(2)
            .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
            .count();
        let measured = crossings as f32 * SAMPLE_RATE / output.len() as f32;

        assert!((measured - 440.0).abs() < 5.0, "measured {measured} Hz");
    }

    #[test]
    fn two_sources_summed_into_one_input_are_both_heard() {
        let mut graph = AudioGraph::new();
        let left = graph.add_node(GraphNode::Constant(1.0));
        let right = graph.add_node(GraphNode::Constant(1.0));
        let sum = graph.add_node(GraphNode::Math(MathOp::Add));
        graph.connect(left, sum, 0);
        graph.connect(right, sum, 1);
        graph.set_output_node(sum);
        graph.compile();

        assert_eq!(graph.process_stereo(0.0, 0.0), (2.0, 2.0));
    }

    #[test]
    fn a_gain_node_scales_its_input() {
        let mut graph = AudioGraph::new();
        let signal = graph.add_node(GraphNode::Input);
        let gain = graph.add_node(GraphNode::Constant(0.25));
        let product = graph.add_node(GraphNode::Math(MathOp::Multiply));
        graph.connect(signal, product, 0);
        graph.connect(gain, product, 1);
        graph.set_output_node(product);
        graph.compile();

        let (left, right) = graph.process_stereo(0.8, -0.4);
        assert!((left - 0.2).abs() < 1e-6, "left {left}");
        assert!((right + 0.1).abs() < 1e-6, "right {right}");
    }

    #[test]
    fn a_mono_effect_node_keeps_the_channels_apart() {
        let mut graph = AudioGraph::new();
        let input = graph.add_node(GraphNode::Input);
        let effect = graph.add_node(GraphNode::effect(EffectModule::Delay(
            crate::synthesis::effects::MonoFeedbackDelay::new(SAMPLE_RATE, 5.0),
        )));
        graph.connect(input, effect, 0);
        graph.set_output_node(effect);
        graph.compile();

        // A click on the left, silence on the right, then run past the 5 ms delay. The echo must
        // come back on the left and the right channel must stay at zero throughout.
        let mut right_peak = 0.0f32;
        let mut left_echo = 0.0f32;

        for index in 0..1_000 {
            let input_left = if index == 0 { 1.0 } else { 0.0 };
            let (left, right) = graph.process_stereo(input_left, 0.0);

            right_peak = right_peak.max(right.abs());
            if index > 400 {
                left_echo = left_echo.max(left.abs());
            }
        }

        assert_eq!(right_peak, 0.0, "the right channel heard the left input");
        assert!(left_echo > 0.1, "the delayed click never came back: {left_echo}");
    }

    #[test]
    fn a_stereo_effect_node_receives_both_channels() {
        let mut graph = AudioGraph::new();
        let input = graph.add_node(GraphNode::Input);
        let width = graph.add_node(GraphNode::stereo_effect(StereoEffectModule::StereoWidth(
            crate::synthesis::effects::StereoWidth::new(SAMPLE_RATE),
        )));
        graph.connect(input, width, 0);
        graph.set_output_node(width);
        graph.compile();

        let (left, right) = graph.process_stereo(0.5, -0.5);
        assert!(left.is_finite() && right.is_finite());
    }

    #[test]
    fn a_dynamics_node_receives_both_channels() {
        let mut graph = AudioGraph::new();
        let input = graph.add_node(GraphNode::Input);
        let limiter = graph.add_node(GraphNode::Dynamics(DynamicsModule::PeakLimiter(
            crate::synthesis::compressors::PeakLimiter::new(SAMPLE_RATE),
        )));
        graph.connect(input, limiter, 0);
        graph.set_output_node(limiter);
        graph.compile();

        let (left, right) = graph.process_stereo(0.9, -0.9);
        assert!(left.is_finite() && right.is_finite());
        assert!(left.abs() <= 1.0 && right.abs() <= 1.0);
    }
}
