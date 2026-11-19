//! Tests for the renderer, with synthetic audio and no audio device.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use rtrb::RingBuffer;

use super::*;

const RATE: u32 = 48_000;
/// Frames of one 5 ms fade at 48 kHz.
const FADE: usize = 240;

/// Counts allocations made by the current thread, so a test can prove
/// `render` makes none.
struct CountingAllocator;

thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

fn count_allocation() {
    // `try_with`: the counter may already be gone while a thread exits.
    let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
}

#[expect(
    unsafe_code,
    reason = "a global allocator must implement the unsafe GlobalAlloc trait"
)]
// SAFETY: every method forwards to the system allocator with the same
// arguments, so its contract is upheld; counting touches only a const
// thread-local `Cell`, which neither allocates nor panics.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count_allocation();
        // SAFETY: forwarded unchanged; the caller upholds `alloc`'s contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        count_allocation();
        // SAFETY: `ptr` came from `System.alloc` with this `layout`.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count_allocation();
        // SAFETY: forwarded unchanged; the caller upholds `realloc`'s contract.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// Allocations (and frees) the current thread made while running `work`.
fn allocations_during(work: impl FnOnce()) -> usize {
    let before = ALLOCATIONS.with(Cell::get);
    work();
    ALLOCATIONS.with(Cell::get) - before
}

/// A renderer with its command queue, garbage queue and shared state.
struct Rig {
    renderer: Renderer,
    commands: Producer<(DeckId, Command)>,
    garbage: Consumer<Box<Track>>,
    shared: Arc<Shared>,
}

fn rig(channels: usize) -> Rig {
    let (commands, command_queue) = RingBuffer::new(16);
    let (garbage_queue, garbage) = RingBuffer::new(4);
    let shared = Arc::new(Shared::default());
    Rig {
        renderer: Renderer::new(
            command_queue,
            garbage_queue,
            Arc::clone(&shared),
            channels,
            RATE,
        ),
        commands,
        garbage,
        shared,
    }
}

impl Rig {
    fn send(&mut self, command: Command) {
        self.commands
            .push((DeckId::A, command))
            .expect("room in the queue");
    }

    /// Renders `frames` stereo frames and returns them interleaved.
    fn render(&mut self, frames: usize) -> Vec<f32> {
        let mut out = vec![1.0; frames * self.renderer.channels];
        self.renderer.render(&mut out);
        out
    }

    fn position(&self) -> u64 {
        self.shared.position.load(Ordering::Relaxed)
    }
}

/// A stereo track: left a ramp from 0 up (sample n is n / frames), right the
/// negative, so every frame's value tells where it came from.
fn ramp(frames: usize) -> Box<Track> {
    let left: Vec<f32> = (0..frames).map(|n| n as f32 / frames as f32).collect();
    let right: Vec<f32> = left.iter().map(|v| -v).collect();
    Box::new(Track::from_stereo(&left, &right))
}

/// A stereo 440 Hz sine at `amplitude`.
fn sine(frames: usize, amplitude: f32) -> Box<Track> {
    let wave: Vec<f32> = (0..frames)
        .map(|n| amplitude * (std::f32::consts::TAU * 440.0 * n as f32 / RATE as f32).sin())
        .collect();
    Box::new(Track::from_stereo(&wave, &wave))
}

/// The largest jump between neighbouring samples of one channel.
fn largest_step(stereo: &[f32]) -> f32 {
    let left: Vec<f32> = stereo.iter().step_by(2).copied().collect();
    left.windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .fold(0.0, f32::max)
}

#[test]
fn outputs_silence_with_nothing_loaded() {
    let mut rig = rig(2);
    rig.send(Command::Play);
    assert!(rig.render(256).iter().all(|&s| s == 0.0));
    assert_eq!(rig.position(), 0);
}

#[test]
fn plays_the_track_after_a_short_fade_in() {
    let mut rig = rig(2);
    rig.send(Command::Load(ramp(48_000)));
    rig.send(Command::Play);
    let out = rig.render(1_000);
    // Fading in: frame n is the track's frame n times a gain rising to 1.
    let gain_at = |n: usize| ((n + 1) as f32 / FADE as f32).min(1.0);
    for n in [0, 10, 100, 239, 240, 999] {
        let expected = n as f32 / 48_000.0 * gain_at(n);
        assert!(
            (out[2 * n] - expected).abs() < 1e-6,
            "frame {n}: {}",
            out[2 * n]
        );
        assert!((out[2 * n + 1] + expected).abs() < 1e-6, "frame {n} right");
    }
    assert_eq!(rig.position(), 1_000);
    assert!(rig.shared.playing.load(Ordering::Relaxed));
}

#[test]
fn pause_fades_out_and_holds_the_position_then_play_resumes_there() {
    let mut rig = rig(2);
    rig.send(Command::Load(sine(48_000, 0.5)));
    rig.send(Command::Play);
    rig.render(1_000);
    rig.send(Command::Pause);
    let fading = rig.render(1_000);
    assert!(largest_step(&fading) < 0.05, "no click");
    assert!(
        fading[2 * FADE..].iter().all(|&s| s == 0.0),
        "silent after the fade"
    );
    let held = rig.position();
    assert_eq!(held, 1_000 + FADE as u64);
    rig.render(1_000);
    assert_eq!(rig.position(), held, "paused: the position stays");
    rig.send(Command::Play);
    rig.render(100);
    assert_eq!(rig.position(), held + 100);
}

#[test]
fn seeking_fades_out_jumps_and_fades_back_in_without_a_click() {
    let mut rig = rig(2);
    rig.send(Command::Load(sine(96_000, 0.5)));
    rig.send(Command::Play);
    let mut out = rig.render(2_000);
    rig.send(Command::Seek(50_123));
    out.extend(rig.render(2_000));
    // A 440 Hz sine at 0.5 moves at most 0.029 per sample; a hard jump would
    // move up to 1.0.
    assert!(largest_step(&out) < 0.05, "step {}", largest_step(&out));
    // Out over one fade (give or take the frame where it reaches zero),
    // then from 50 123 on.
    let expected = 50_123 + 2_000 - FADE as u64;
    assert!(rig.position().abs_diff(expected) <= 1, "{}", rig.position());
}

#[test]
fn seeking_while_paused_moves_at_once_and_stays_paused() {
    let mut rig = rig(2);
    rig.send(Command::Load(ramp(48_000)));
    rig.send(Command::Seek(30_000));
    assert!(rig.render(100).iter().all(|&s| s == 0.0));
    assert_eq!(rig.position(), 30_000);
    rig.send(Command::Seek(99_999_999));
    rig.render(10);
    assert_eq!(rig.position(), 48_000, "past the end is the end");
}

#[test]
fn reaching_the_end_stops_and_counts_one_end() {
    let mut rig = rig(2);
    rig.send(Command::Load(ramp(500)));
    rig.send(Command::Play);
    let out = rig.render(1_000);
    assert!(out[1_000..].iter().all(|&s| s == 0.0));
    assert_eq!(rig.position(), 500);
    assert!(!rig.shared.playing.load(Ordering::Relaxed));
    rig.render(1_000);
    assert_eq!(rig.shared.ends.load(Ordering::Relaxed), 1);
}

#[test]
fn a_seek_or_load_in_the_last_fade_of_a_track_is_not_its_end() {
    // Seek back while the last 5 ms play: keeps playing from the target.
    let mut seeking = rig(2);
    seeking.send(Command::Load(sine(10_000, 0.5)));
    seeking.send(Command::Play);
    seeking.render(9_900);
    seeking.send(Command::Seek(1_000));
    seeking.render(2_000);
    assert!(seeking.shared.playing.load(Ordering::Relaxed));
    assert_eq!(seeking.shared.ends.load(Ordering::Relaxed), 0);
    let position = seeking.position();
    assert!(position > 1_000 && position < 3_000, "{position}");

    // Load and play another track there: the new one starts.
    let mut loading = rig(2);
    loading.send(Command::Load(sine(10_000, 0.5)));
    loading.send(Command::Play);
    loading.render(9_900);
    loading.send(Command::Load(ramp(48_000)));
    loading.send(Command::Play);
    loading.render(2_000);
    assert!(loading.shared.playing.load(Ordering::Relaxed));
    assert_eq!(loading.shared.ends.load(Ordering::Relaxed), 0);
    assert!(loading.position() > 1_000, "{}", loading.position());
}

#[test]
fn loading_while_playing_fades_out_swaps_and_hands_the_old_track_back() {
    let mut rig = rig(2);
    rig.send(Command::Load(sine(48_000, 0.5)));
    rig.send(Command::Play);
    rig.render(1_000);
    rig.send(Command::Load(ramp(48_000)));
    let out = rig.render(1_000);
    assert!(largest_step(&out) < 0.05, "no click");
    assert_eq!(rig.position(), 0, "the new track waits at its start");
    assert!(!rig.shared.playing.load(Ordering::Relaxed));
    let old = rig.garbage.pop().expect("the old track is handed back");
    assert_eq!(old.frames(), 48_000);
}

#[test]
fn handles_any_callback_size_and_channel_count() {
    for channels in [1, 2, 6] {
        let mut rig = rig(channels);
        rig.send(Command::Load(ramp(48_000)));
        rig.send(Command::Play);
        rig.render(FADE);
        for frames in [1, 7, 511, 4_096] {
            let out = rig.render(frames);
            assert_eq!(out.len(), frames * channels);
            if channels == 1 {
                // Left and right are opposite: mono is their average, 0.
                assert!(out.iter().all(|s| s.abs() < 1e-6));
            } else {
                let frame = &out[..channels];
                assert!(frame[0] > 0.0 && frame[1] < 0.0);
                assert!(
                    frame[2..].iter().all(|&s| s == 0.0),
                    "extra outputs are silent"
                );
            }
        }
    }
    // A buffer that is not a whole number of frames: the tail is silent.
    let mut rig = rig(2);
    let mut out = vec![1.0; 5];
    rig.renderer.render(&mut out);
    assert_eq!(out, [0.0; 5]);
}

#[test]
fn publishes_peak_and_rms() {
    let mut rig = rig(2);
    rig.send(Command::Load(sine(48_000, 1.0)));
    rig.send(Command::Play);
    rig.render(FADE);
    rig.shared.peak.store(0, Ordering::Relaxed);
    rig.render(4_800);
    let peak = f32::from_bits(rig.shared.peak.load(Ordering::Relaxed));
    let rms = f32::from_bits(rig.shared.rms.load(Ordering::Relaxed));
    assert!((peak - 1.0).abs() < 0.01, "{peak}");
    assert!(
        (rms - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01,
        "{rms}"
    );
}

#[test]
fn render_never_allocates_or_frees() {
    let mut rig = rig(2);
    let mut out = vec![0.0_f32; 512 * 2];
    rig.send(Command::Load(sine(48_000, 0.5)));
    rig.send(Command::Play);
    let mut count = allocations_during(|| rig.renderer.render(&mut out));
    for command in [
        Command::Seek(10_000),
        Command::Pause,
        Command::Play,
        Command::Load(ramp(48_000)),
        Command::Play,
    ] {
        rig.send(command);
        count += allocations_during(|| {
            for _ in 0..4 {
                rig.renderer.render(&mut out);
            }
        });
    }
    assert_eq!(count, 0, "render allocated or freed memory");
    assert!(
        rig.garbage.pop().is_ok(),
        "the replaced track left the renderer"
    );
}
