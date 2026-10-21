use crate::synthesis::filters::FilterModule;
use crate::synthesis::oscillators::OscillatorModule;
use crate::synthesis::oscillators::polyblep::Waveform;
use crate::synthesis::oscillators::noise::NoiseType;
use crate::synthesis::utils::Processor;

/// Represents a distinct processing element in the AudioGraph.
#[derive(Debug, Clone)]
pub enum GraphNode {
    /// A constant value, often used as a static modulator.
    Constant(f32),
    /// A generator. Inputs: 0: Phase Modulation. Outputs: 1.
    Oscillator {
        module: OscillatorModule,
        wave_type: Waveform,
        noise_type: NoiseType,
    },
    /// An effect. Inputs: 0: Audio. Outputs: 1.
    Filter(FilterModule),
    /// Mathematical operations.
    Math(MathOp),
    /// A special node that outputs the graph's overall input signal.
    Input,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MathOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Clamp { min: f32, max: f32 },
}

impl GraphNode {
    /// Evaluate the node given its resolved inputs.
    #[inline(always)]
    pub fn process_sample(&mut self, inputs: &[f32], graph_input: f32) -> f32 {
        match self {
            GraphNode::Constant(val) => *val,
            GraphNode::Input => graph_input,
            GraphNode::Oscillator { module, wave_type, noise_type } => {
                // If it's an FM operator or Phase Distortion, it might use inputs[0] as phase/PM.
                let pm = inputs.get(0).copied().unwrap_or(0.0);
                match module {
                    OscillatorModule::Fm(osc) => osc.next_sample(pm),
                    OscillatorModule::PhaseDistortion(osc) => osc.process_sample(pm),
                    OscillatorModule::PolyBlep(osc) => osc.next_sample(*wave_type),
                    OscillatorModule::Noise(osc) => osc.next_sample(*noise_type),
                    OscillatorModule::Wavetable(osc) => osc.process_sample(),
                    OscillatorModule::KarplusStrong(osc) => osc.next_sample(),
                    OscillatorModule::Supersaw(osc) => osc.next_sample(),
                    OscillatorModule::Granular(osc) => osc.process_sample(0.0),
                    OscillatorModule::Vector(osc) => osc.process_sample(0.0),
                    OscillatorModule::PhysicalModeling(osc) => osc.process_sample(0.0),
                    OscillatorModule::Additive(osc) => osc.next_sample(),
                }
            }
            GraphNode::Filter(module) => {
                let input = inputs.get(0).copied().unwrap_or(0.0);
                // In a true modular setup, inputs[1] might map to cutoff_mod.
                // We're keeping the enum simple for now, but letting it process audio.
                module.process_sample(input)
            }
            GraphNode::Math(op) => {
                let a = inputs.get(0).copied().unwrap_or(0.0);
                let b = inputs.get(1).copied().unwrap_or(0.0);
                match op {
                    MathOp::Add => a + b,
                    MathOp::Subtract => a - b,
                    MathOp::Multiply => a * b,
                    MathOp::Divide => if b.abs() > 1e-6 { a / b } else { 0.0 },
                    MathOp::Clamp { min, max } => a.clamp(*min, *max),
                }
            }
        }
    }
}

/// An edge mapping an output from a source node to an input slot of a destination node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edge {
    pub source_node: usize,
    pub dest_node: usize,
    pub dest_input_index: usize,
}

/// Represents the pre-calculated input sources for a single node during execution.
#[derive(Debug, Clone)]
struct CompiledNode {
    node_idx: usize,
    // For each input slot (e.g., slot 0, slot 1), which source nodes feed into it?
    // Using a small fixed vector avoids allocations and keeps cache tight.
    input_sources: [Vec<usize>; 4],
}

/// A zero-allocation, statically-dispatched audio processing graph.
pub struct AudioGraph {
    nodes: Vec<GraphNode>,
    edges: Vec<Edge>,

    // Execution State (Pre-allocated in `compile`)
    compiled_execution: Vec<CompiledNode>,
    node_outputs: Vec<f32>,
    input_scratch: Vec<f32>,
    output_node: Option<usize>,
}

impl AudioGraph {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            compiled_execution: Vec::new(),
            node_outputs: Vec::new(),
            input_scratch: vec![0.0; 4], // Support up to 4 inputs per node
            output_node: None,
        }
    }

    pub fn add_node(&mut self, node: GraphNode) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(node);
        self.node_outputs.push(0.0);
        idx
    }

    pub fn connect(&mut self, source_node: usize, dest_node: usize, dest_input_index: usize) {
        assert!(source_node < self.nodes.len(), "Source node out of bounds");
        assert!(dest_node < self.nodes.len(), "Destination node out of bounds");
        assert!(dest_input_index < 4, "Only 4 input slots are supported");

        self.edges.push(Edge {
            source_node,
            dest_node,
            dest_input_index,
        });
    }

    pub fn set_output_node(&mut self, node_index: usize) {
        assert!(node_index < self.nodes.len(), "Output node out of bounds");
        self.output_node = Some(node_index);
    }

    pub fn compile(&mut self) {
        let n = self.nodes.len();
        let mut in_degree = vec![0; n];
        let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); n];

        for edge in &self.edges {
            adjacency[edge.source_node].push(edge.dest_node);
            in_degree[edge.dest_node] += 1;
        }

        let mut queue: Vec<usize> = in_degree.iter()
            .enumerate()
            .filter(|&(_, &deg)| deg == 0)
            .map(|(i, _)| i)
            .collect();

        let mut order = Vec::with_capacity(n);
        while let Some(u) = queue.pop() {
            order.push(u);
            for &v in &adjacency[u] {
                in_degree[v] -= 1;
                if in_degree[v] == 0 {
                    queue.push(v);
                }
            }
        }

        if order.len() != n {
            panic!("Cycle detected in AudioGraph! DAGs only.");
        }

        // Build compiled execution order with O(1) input lookups
        self.compiled_execution.clear();
        for &node_idx in &order {
            let mut compiled = CompiledNode {
                node_idx,
                input_sources: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            };

            // Pre-calculate which sources feed into this node
            for edge in &self.edges {
                if edge.dest_node == node_idx {
                    compiled.input_sources[edge.dest_input_index].push(edge.source_node);
                }
            }
            self.compiled_execution.push(compiled);
        }

        self.node_outputs.resize(n, 0.0);
        self.input_scratch.resize(4, 0.0);
    }
}

impl Processor for AudioGraph {
    #[inline(always)]
    fn process_sample(&mut self, graph_input: f32) -> f32 {
        if self.compiled_execution.is_empty() {
            return graph_input; // Pass through if empty
        }

        for i in 0..self.compiled_execution.len() {
            let compiled = &self.compiled_execution[i];
            let node_idx = compiled.node_idx;

            // Gather inputs in O(1) time
            for slot in 0..4 {
                let mut sum = 0.0;
                for &src_idx in &compiled.input_sources[slot] {
                    sum += self.node_outputs[src_idx];
                }
                self.input_scratch[slot] = sum;
            }

            // Execute node
            let out_val = self.nodes[node_idx].process_sample(&self.input_scratch, graph_input);
            self.node_outputs[node_idx] = out_val;
        }

        if let Some(out_idx) = self.output_node {
            self.node_outputs[out_idx]
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synthesis::filters::one_pole::OnePoleFilter;
    use crate::synthesis::filters::FilterModule;

    #[test]
    fn graph_evaluates_basic_math_correctly() {
        let mut graph = AudioGraph::new();

        let n0 = graph.add_node(GraphNode::Constant(2.0));
        let n1 = graph.add_node(GraphNode::Constant(3.0));
        let n2 = graph.add_node(GraphNode::Math(MathOp::Add));

        graph.connect(n0, n2, 0);
        graph.connect(n1, n2, 1);

        graph.set_output_node(n2);
        graph.compile();

        let out = graph.process_sample(0.0);
        assert_eq!(out, 5.0, "2 + 3 should equal 5");
    }

    #[test]
    #[should_panic(expected = "Cycle detected")]
    fn graph_rejects_cycles() {
        let mut graph = AudioGraph::new();

        let n0 = graph.add_node(GraphNode::Constant(1.0));
        let n1 = graph.add_node(GraphNode::Math(MathOp::Add));

        graph.connect(n0, n1, 0);
        graph.connect(n1, n1, 1);
        graph.compile();
    }

    #[test]
    fn graph_filters_signal() {
        let mut graph = AudioGraph::new();

        // Pass external graph input into the filter
        let n0 = graph.add_node(GraphNode::Input);

        let mut lp = OnePoleFilter::new();
        lp.set_lowpass(0.1);
        let n1 = graph.add_node(GraphNode::Filter(FilterModule::OnePole(lp)));

        graph.connect(n0, n1, 0);
        graph.set_output_node(n1);
        graph.compile();

        let mut last_val = 0.0;
        for _ in 0..10 {
            // Push a step signal
            last_val = graph.process_sample(1.0);
        }

        assert!(last_val > 0.0 && last_val < 1.0, "Filter should be smoothing the step to 1.0. Got: {}", last_val);
    }
}
