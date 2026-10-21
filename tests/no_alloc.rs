//! Proof that the real-time paths do not touch the heap.
//!
//! The crate claims zero allocation after construction, so measure it rather than trust it: a
//! thread-local counting allocator wraps the system one, and each test drives the audio path for a
//! full second and asserts the allocation count did not move.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use o3dsp::synthesis::filters::{FilterModule, OnePoleFilter};
use o3dsp::synthesis::metering::LufsMeter;
use o3dsp::synthesis::oscillators::SineBank;
use o3dsp::synthesis::spectral::{AdditiveOscillator, HarmonicSpectrum, Partial, MAX_PARTIALS};
use o3dsp::synthesis::utils::graph::{AudioGraph, GraphNode, MathOp};
use o3dsp::synthesis::utils::Processor;

// Allocations made on this thread since the counter was last reset.
thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
        System.realloc(ptr, layout, new_size)
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Run `body`, then report how many allocations happened on this thread while it ran.
fn allocations_during<T>(body: impl FnOnce() -> T) -> (T, usize) {
    // Touching the thread-local first keeps its own one-off allocation out of the measurement.
    ALLOCATIONS.with(|count| count.set(0));
    let value = body();
    (value, ALLOCATIONS.with(|count| count.get()))
}

const SAMPLE_RATE: f32 = 48_000.0;

#[test]
fn lufs_meter_allocates_nothing_while_processing() {
    let mut meter = LufsMeter::new(SAMPLE_RATE);

    // Fill every window and the block history first.
    for _ in 0..(SAMPLE_RATE as usize * 2) {
        meter.process_stereo(0.1, -0.1);
    }

    let ((), allocations) = allocations_during(|| {
        for _ in 0..SAMPLE_RATE as usize {
            meter.process_stereo(0.1, -0.1);
            meter.process_sample(0.1);
        }
        let _ = meter.integrated();
        let _ = meter.momentary();
        let _ = meter.short_term();
        meter.reset();
    });

    assert_eq!(
        allocations, 0,
        "LufsMeter hit the heap {allocations} times on the audio path"
    );
}

#[test]
fn sine_bank_allocates_nothing_while_processing() {
    let mut bank = SineBank::new(SAMPLE_RATE, 64);
    let partials: Vec<Partial> = (1..=64)
        .map(|harmonic| Partial::new(harmonic as f32 * 110.0, 0.01, 0.0))
        .collect();

    // `set_partials` is the per-frame call, so it belongs in the measured window too.
    for _ in 0..1_000 {
        bank.set_partials(&partials);
        let _ = bank.process_sample();
    }

    let ((), allocations) = allocations_during(|| {
        for _ in 0..SAMPLE_RATE as usize {
            bank.set_partials(&partials);
            let _ = bank.process_sample();
        }
    });

    assert_eq!(
        allocations, 0,
        "SineBank hit the heap {allocations} times on the audio path"
    );
}

#[test]
fn additive_oscillator_allocates_nothing_while_processing() {
    let mut oscillator: AdditiveOscillator<MAX_PARTIALS> = AdditiveOscillator::new(SAMPLE_RATE);
    oscillator.set_frequency(440.0);
    oscillator.reset_frequency(440.0);

    for _ in 0..4_800 {
        let _ = oscillator.next_sample();
    }

    let ((), allocations) = allocations_during(|| {
        for _ in 0..SAMPLE_RATE as usize {
            let _ = oscillator.next_sample();
        }
        oscillator.set_spectrum(HarmonicSpectrum::saw());
        oscillator.set_stretch(1.5);
        oscillator.set_level(0.5);
    });

    assert_eq!(
        allocations, 0,
        "AdditiveOscillator hit the heap {allocations} times on the audio path"
    );
}

#[test]
fn audio_graph_allocates_nothing_while_processing() {
    let mut graph = AudioGraph::new();

    let input = graph.add_node(GraphNode::Input);
    let mut lowpass = OnePoleFilter::new();
    lowpass.set_lowpass(0.1);
    let filter = graph.add_node(GraphNode::filter(FilterModule::OnePole(lowpass)));
    let gain = graph.add_node(GraphNode::Constant(0.5));
    let mix = graph.add_node(GraphNode::Math(MathOp::Multiply));

    graph.connect(input, filter, 0);
    graph.connect(filter, mix, 0);
    graph.connect(gain, mix, 1);
    graph.set_output_node(mix);
    graph.compile();

    for _ in 0..1_000 {
        let _ = graph.process_sample(0.1);
    }

    let (last, allocations) = allocations_during(|| {
        let mut last = 0.0;
        for _ in 0..SAMPLE_RATE as usize {
            last = graph.process_sample(0.1);
        }
        last
    });

    assert!(last.is_finite(), "graph produced {last}");
    assert_eq!(
        allocations, 0,
        "AudioGraph hit the heap {allocations} times on the audio path"
    );
}

#[test]
fn the_gated_history_costs_no_heap() {
    // The histogram replaced a growing Vec, so the cost moved into the struct itself.
    assert!(
        std::mem::size_of::<LufsMeter>() < 32 * 1024,
        "LufsMeter grew to {} bytes",
        std::mem::size_of::<LufsMeter>()
    );
    println!(
        "size_of::<LufsMeter>() = {} bytes, size_of::<AdditiveOscillator>() = {} bytes",
        std::mem::size_of::<LufsMeter>(),
        std::mem::size_of::<AdditiveOscillator>()
    );
}
