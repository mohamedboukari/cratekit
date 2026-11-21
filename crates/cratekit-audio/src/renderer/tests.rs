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
    garbage: Consumer<Retired>,
    shared: Arc<Shared>,
}

fn rig(channels: usize) -> Rig {
    rig_with_filter(channels, 0.0)
}

/// A rig whose renderer starts with the DJ filter at `filter`.
fn rig_with_filter(channels: usize, filter: f32) -> Rig {
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
            filter,
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
    let Ok(Retired::Track(old)) = rig.garbage.pop() else {
        panic!("the old track is handed back");
    };
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
fn the_filter_knob_shapes_what_is_heard_and_the_meters() {
    let frames = RATE as usize / 2;
    let mut rig = rig(2);
    let track = sine(RATE as usize * 4, 0.5);
    let wave: Vec<f32> = track.samples().to_vec();
    rig.send(Command::Load(track));
    rig.send(Command::Play);
    let opened = rig.render(frames);
    let open = opened.iter().fold(0.0_f32, |p, x| p.max(x.abs()));

    // Full high-pass removes most of a 440 Hz tone, and the meters see it.
    rig.send(Command::SetFilter(1.0));
    rig.render(frames);
    rig.shared.peak.swap(0, Ordering::Relaxed);
    let filtered = rig.render(frames);
    let heard = filtered.iter().fold(0.0_f32, |p, x| p.max(x.abs()));
    let metered = f32::from_bits(rig.shared.peak.load(Ordering::Relaxed));
    assert!(heard < 0.1 * open, "open {open}, filtered {heard}");
    assert!(
        (metered - heard).abs() < 1e-6,
        "meter {metered}, heard {heard}"
    );

    // Back to the centre: once settled, the output is the track exactly.
    rig.send(Command::SetFilter(0.0));
    rig.render(frames);
    let start = rig.position() as usize * 2;
    let back = rig.render(frames);
    assert_eq!(back, wave[start..start + frames * 2]);
}

#[test]
fn a_renderer_can_start_with_the_filter_already_set() {
    let mut rig = rig_with_filter(2, 1.0);
    rig.send(Command::Load(sine(RATE as usize, 0.5)));
    rig.send(Command::Play);
    let out = rig.render(4_800);
    // Right after the 5 ms fade in, the tone is already filtered away: no
    // sweep from an open filter.
    let peak = out[2 * FADE..].iter().fold(0.0_f32, |p, x| p.max(x.abs()));
    assert!(peak < 0.1, "peak {peak}");
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
        Command::SetFilter(-0.7),
        Command::AttachStems(stems_of(48_000, [0.1, 0.2, 0.3, 0.4])),
        Command::SetMode(PlaybackMode::Stems),
        Command::SetStemGain(Stem::Bass, 1.5),
        Command::SetStemMute(Stem::Drums, true),
        Command::SetStemSolo(Stem::Vocals, true),
        Command::AttachStems(stems_of(48_000, [0.4, 0.3, 0.2, 0.1])),
        Command::Load(ramp(48_000)),
        Command::AttachStems(stems_of(48_000, [0.1, 0.1, 0.1, 0.1])),
        Command::Play,
        Command::SetMode(PlaybackMode::Original),
        Command::SetFilter(0.4),
        Command::SetMaster(preview(1.0, 6.0, -6.0)),
        Command::SetMode(PlaybackMode::Mastered),
        Command::SetMaster(preview(0.5, 3.0, -3.0)),
        Command::SetLevelMatch(false),
        Command::Seek(1_000),
        Command::Load(sine(48_000, 0.3)),
        Command::Play,
        Command::SetLevelMatch(true),
        Command::SetMode(PlaybackMode::Original),
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

/// Four stems of steady levels: stem `i` is `levels[i]` on the left and
/// `-levels[i]` on the right.
fn stems_of(frames: usize, levels: [f32; STEM_COUNT]) -> Box<StemSet> {
    let channels = levels.map(|level| (vec![level; frames], vec![-level; frames]));
    Box::new(StemSet::from_stereo(
        [0, 1, 2, 3].map(|i| (channels[i].0.as_slice(), channels[i].1.as_slice())),
    ))
}

/// Stems of a 440 Hz sine split into quarters: they sum to `sine`.
fn sine_quarters(frames: usize, amplitude: f32) -> Box<StemSet> {
    let wave: Vec<f32> = (0..frames)
        .map(|n| amplitude / 4.0 * (std::f32::consts::TAU * 440.0 * n as f32 / RATE as f32).sin())
        .collect();
    Box::new(StemSet::from_stereo(
        [(wave.as_slice(), wave.as_slice()); STEM_COUNT],
    ))
}

/// Starts a rig playing a steady track of `level` with `stems`, past the
/// fade in and the mode crossfade.
fn playing_stems(level: f32, stems: Box<StemSet>) -> Rig {
    let mut rig = rig(2);
    let track = Track::from_stereo(&vec![level; 96_000], &vec![-level; 96_000]);
    rig.send(Command::Load(Box::new(track)));
    rig.send(Command::AttachStems(stems));
    rig.send(Command::Play);
    rig.send(Command::SetMode(PlaybackMode::Stems));
    rig.render(2_000);
    rig
}

#[test]
fn stems_at_full_gain_sound_like_the_track() {
    let mut rig = rig(2);
    rig.send(Command::Load(sine(48_000, 0.8)));
    rig.send(Command::AttachStems(sine_quarters(48_000, 0.8)));
    rig.send(Command::Play);
    rig.render(1_000);
    rig.send(Command::SetMode(PlaybackMode::Stems));
    let out = rig.render(4_000);
    assert!(largest_step(&out) < 0.07, "no click when switching");
    // The stems sum to the track, so the switch is inaudible.
    let mut original = super::tests::rig(2);
    original.send(Command::Load(sine(48_000, 0.8)));
    original.send(Command::Play);
    original.render(1_000);
    let expected = original.render(4_000);
    let error = out
        .iter()
        .zip(&expected)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0_f32, f32::max);
    assert!(error < 1e-5, "stems differ from the track by {error}");
}

#[test]
fn each_stem_has_its_gain_mute_and_solo() {
    let mut rig = playing_stems(0.0, stems_of(96_000, [0.1, 0.2, 0.3, 0.4]));
    let level = |rig: &mut Rig| rig.render(1_000)[1_998];
    assert!(
        (level(&mut rig) - 1.0).abs() < 1e-5,
        "all four: 0.1 + 0.2 + 0.3 + 0.4"
    );

    rig.send(Command::SetStemMute(Stem::Vocals, true));
    assert!((level(&mut rig) - 0.6).abs() < 1e-5, "vocals muted");
    rig.send(Command::SetStemGain(Stem::Drums, 2.0));
    assert!((level(&mut rig) - 0.7).abs() < 1e-5, "drums doubled");
    rig.send(Command::SetStemSolo(Stem::Bass, true));
    assert!(
        (level(&mut rig) - 0.2).abs() < 1e-5,
        "only bass while it is soloed"
    );
    rig.send(Command::SetStemSolo(Stem::Vocals, true));
    assert!(
        (level(&mut rig) - 0.6).abs() < 1e-5,
        "a soloed stem is heard even when muted"
    );
    rig.send(Command::SetStemSolo(Stem::Bass, false));
    rig.send(Command::SetStemSolo(Stem::Vocals, false));
    assert!(
        (level(&mut rig) - 0.7).abs() < 1e-5,
        "back to the mute and the gain"
    );
    rig.send(Command::SetStemGain(Stem::Other, f32::NAN));
    rig.send(Command::SetStemGain(Stem::Bass, 9.0));
    // Drums 2 x 0.1, bass clamped to 2 x 0.2, other 1 x 0.3, vocals muted.
    assert!((level(&mut rig) - 0.9).abs() < 1e-5, "NaN is 1, 9 is 2");
}

#[test]
fn stem_changes_ramp_without_clicks() {
    let mut rig = playing_stems(0.0, stems_of(96_000, [0.5, 0.0, 0.0, 0.5]));
    rig.send(Command::SetStemMute(Stem::Drums, true));
    let out = rig.render(1_000);
    // 0.5 removed over 10 ms (480 frames): about 0.001 per frame.
    assert!(largest_step(&out) < 0.002, "the mute ramps");
    assert!((out[1_998] - 0.5).abs() < 1e-5);
    rig.send(Command::SetMode(PlaybackMode::Original));
    let out = rig.render(1_000);
    assert!(largest_step(&out) < 0.002, "the switch back crossfades");
    assert!(out[1_998].abs() < 1e-5, "the track itself is silent");
}

#[test]
fn stems_mode_without_stems_plays_the_track() {
    let mut rig = rig(2);
    let track = Track::from_stereo(&vec![0.3; 48_000], &vec![-0.3; 48_000]);
    rig.send(Command::Load(Box::new(track)));
    rig.send(Command::SetMode(PlaybackMode::Stems));
    rig.send(Command::Play);
    let out = rig.render(2_000);
    assert!((out[3_998] - 0.3).abs() < 1e-5);
}

#[test]
fn stems_sent_after_a_load_wait_for_that_track() {
    let mut rig = playing_stems(0.0, stems_of(96_000, [0.1, 0.1, 0.1, 0.1]));
    // A new track and its stems while the old one plays.
    let track = Track::from_stereo(&vec![0.0; 96_000], &vec![0.0; 96_000]);
    rig.send(Command::Load(Box::new(track)));
    rig.send(Command::AttachStems(stems_of(96_000, [0.2, 0.2, 0.2, 0.2])));
    rig.render(1_000);
    rig.send(Command::Play);
    let out = rig.render(2_000);
    assert!(
        (out[3_998] - 0.8).abs() < 1e-5,
        "the new track plays its own stems"
    );
    let handed_back: Vec<&str> = std::iter::from_fn(|| rig.garbage.pop().ok())
        .map(|item| match item {
            Retired::Track(_) => "track",
            Retired::Stems(_) => "stems",
        })
        .collect();
    assert_eq!(
        handed_back,
        ["track", "stems"],
        "the old track and its stems are freed elsewhere"
    );
}

#[test]
fn stems_attached_after_a_load_crossfade_in_from_the_track() {
    let mut rig = playing_stems(0.0, stems_of(96_000, [0.25, 0.25, 0.25, 0.25]));
    let track = Track::from_stereo(&vec![0.0; 96_000], &vec![0.0; 96_000]);
    rig.send(Command::Load(Box::new(track)));
    rig.render(1_000);
    rig.send(Command::Play);
    rig.render(1_000);
    // New stems a moment later: they ramp in from 0, not from the old mix.
    rig.send(Command::AttachStems(stems_of(96_000, [0.5, 0.0, 0.0, 0.0])));
    let out = rig.render(1_000);
    assert!(largest_step(&out) < 0.002, "no jump when the stems arrive");
    assert!((out[1_998] - 0.5).abs() < 1e-5);
}

/// A mastered side: Club at `intensity`, with makeup and level match in dB.
fn preview(intensity: f32, makeup_db: f32, level_match_db: f32) -> MasterPreview {
    MasterPreview {
        preset: PresetName::Club.preset().at(intensity),
        makeup_db,
        level_match_db,
    }
}

/// A rig playing `track`, past its fade in.
fn playing(track: Box<Track>) -> Rig {
    let mut rig = rig(2);
    rig.send(Command::Load(track));
    rig.send(Command::Play);
    rig.render(FADE * 4);
    rig
}

fn rms(stereo: &[f32]) -> f32 {
    (stereo.iter().map(|s| s * s).sum::<f32>() / stereo.len().max(1) as f32).sqrt()
}

#[test]
fn the_mastered_mode_plays_the_track_until_a_preview_is_set() {
    let mut original = playing(sine(48_000, 0.3));
    let mut mastered = playing(sine(48_000, 0.3));
    mastered.send(Command::SetMode(PlaybackMode::Mastered));
    assert_eq!(original.render(2_000), mastered.render(2_000));
}

#[test]
fn turning_the_preview_on_while_playing_fades_out_and_back_in() {
    let mut rig = playing(tone(437.3, 48_000, 0.3));
    rig.render(37); // any phase
    rig.send(Command::SetMaster(preview(0.0, 0.0, 0.0)));
    let out = rig.render(FADE * 6);
    let quietest = out
        .chunks(64)
        .map(|block| block.iter().fold(0.0_f32, |m, s| m.max(s.abs())))
        .fold(1.0_f32, f32::min);
    assert!(quietest < 0.01, "it dipped to silence: {quietest}");
    assert!(
        largest_step(&out) < 0.05,
        "no click: {}",
        largest_step(&out)
    );
    let after = rig.render(2_000);
    assert!(rms(&after) > 0.15, "playing again");
}

#[test]
fn with_a_neutral_preview_a_and_b_are_the_same_sample_for_sample() {
    // Intensity 0, no makeup, no level match: the B side is the A side
    // through a limiter that does nothing; the A side is delayed to match.
    // Level match off: it follows live meters, which this test is not about.
    let mut rig = playing(sine(48_000, 0.3));
    rig.send(Command::SetLevelMatch(false));
    rig.send(Command::SetMaster(preview(0.0, 0.0, 0.0)));
    rig.render(FADE * 4);
    let a = rig.render(1_000);
    rig.send(Command::SetMode(PlaybackMode::Mastered));
    rig.render(1_000); // past the 10 ms crossfade
    let mut reference = playing(sine(48_000, 0.3));
    reference.send(Command::SetLevelMatch(false));
    reference.send(Command::SetMaster(preview(0.0, 0.0, 0.0)));
    reference.render(FADE * 4 + 1_000 + 1_000);
    let b = rig.render(1_000);
    let a_again = reference.render(1_000);
    for (x, y) in b.iter().zip(&a_again) {
        assert!((x - y).abs() < 1e-5, "A and B aligned: {x} vs {y}");
    }
    assert!(rms(&a) > 0.15);
}

#[test]
fn the_mastered_side_is_limited_under_the_ceiling() {
    let mut rig = playing(sine(48_000, 0.8));
    rig.send(Command::SetLevelMatch(false));
    // No compressor (intensity 0): the limiter alone takes +12 dB of makeup.
    rig.send(Command::SetMaster(preview(0.0, 12.0, 0.0)));
    rig.send(Command::SetMode(PlaybackMode::Mastered));
    rig.render(4_000);
    let out = rig.render(4_000);
    let peak = out.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
    let ceiling = 10_f32.powf(-1.0 / 20.0);
    assert!(peak <= ceiling, "{peak} over {ceiling}");
    assert!(peak > 0.7, "pushed to the limiter: {peak}");
}

#[test]
fn level_match_keeps_a_and_b_about_as_loud() {
    let measure = |level_match: bool| {
        let mut rig = playing(sine(48_000, 0.2));
        rig.send(Command::SetLevelMatch(level_match));
        rig.send(Command::SetMaster(preview(0.0, 6.0, -6.0)));
        rig.render(FADE * 4);
        let a = rms(&rig.render(4_000));
        rig.send(Command::SetMode(PlaybackMode::Mastered));
        rig.render(2_000);
        let b = rms(&rig.render(4_000));
        20.0 * (b / a).log10()
    };
    let matched = measure(true);
    assert!(matched.abs() < 1.0, "level matched: {matched} dB");
    let raw = measure(false);
    assert!((raw - 6.0).abs() < 1.0, "the real loudness: {raw} dB");
}

#[test]
fn switching_sides_is_click_free() {
    let mut rig = playing(sine(48_000, 0.3));
    rig.send(Command::SetMaster(preview(1.0, 6.0, -6.0)));
    rig.render(FADE * 4);
    let steady = largest_step(&rig.render(1_000));
    for mode in [
        PlaybackMode::Mastered,
        PlaybackMode::Original,
        PlaybackMode::Mastered,
    ] {
        rig.send(Command::SetMode(mode));
        let switch = largest_step(&rig.render(1_000));
        assert!(
            switch < steady * 1.5 + 0.01,
            "{mode:?}: {switch} vs steady {steady}"
        );
    }
}

#[test]
fn a_new_track_starts_without_the_old_ones_tail_on_the_mastered_side() {
    let mut rig = playing(sine(48_000, 0.8));
    rig.send(Command::SetMaster(preview(1.0, 6.0, 0.0)));
    rig.send(Command::SetMode(PlaybackMode::Mastered));
    rig.render(4_000);
    let silent = Box::new(Track::from_stereo(&[0.0; 4_000], &[0.0; 4_000]));
    rig.send(Command::Load(silent));
    rig.send(Command::Play);
    rig.render(FADE * 2); // the fade out and the swap
    let after = rig.render(2_000);
    assert!(
        after.iter().all(|s| *s == 0.0),
        "nothing of the old track came out"
    );
}

/// A stereo tone at `freq` (not a whole number of periods per callback, so
/// no test passes by phase luck).
fn tone(freq: f32, frames: usize, amplitude: f32) -> Box<Track> {
    let wave: Vec<f32> = (0..frames)
        .map(|n| amplitude * (std::f32::consts::TAU * freq * n as f32 / RATE as f32).sin())
        .collect();
    Box::new(Track::from_stereo(&wave, &wave))
}

#[test]
fn seeks_and_loads_with_the_preview_on_do_not_click() {
    for mode in [PlaybackMode::Original, PlaybackMode::Mastered] {
        let mut rig = playing(tone(1_013.7, 96_000, 0.5));
        rig.send(Command::SetMaster(preview(0.0, 0.0, 0.0)));
        rig.send(Command::SetMode(mode));
        rig.render(FADE * 6);
        let steady = largest_step(&rig.render(1_000));
        for offset in [7_013, 20_001, 33_337] {
            rig.send(Command::Seek(offset));
            let around = largest_step(&rig.render(1_500));
            assert!(
                around < steady * 1.5 + 0.01,
                "{mode:?} seek: {around} vs {steady}"
            );
        }
        rig.send(Command::Load(tone(733.1, 48_000, 0.5)));
        rig.send(Command::Play);
        let around = largest_step(&rig.render(1_500));
        assert!(
            around < steady * 1.5 + 0.01,
            "{mode:?} load: {around} vs {steady}"
        );
    }
}

#[test]
fn preset_changes_while_hearing_b_glide_without_a_click() {
    let mut rig = playing(tone(437.3, 96_000, 0.3));
    rig.send(Command::SetLevelMatch(false));
    rig.send(Command::SetMaster(preview(1.0, 0.0, 0.0)));
    rig.send(Command::SetMode(PlaybackMode::Mastered));
    rig.render(4_000);
    let steady = largest_step(&rig.render(1_000));
    let lofi = MasterPreview {
        preset: PresetName::LoFi.preset(),
        makeup_db: 0.0,
        level_match_db: 0.0,
    };
    for change in [lofi, preview(1.0, 0.0, 0.0), preview(0.0, 0.0, 0.0), lofi] {
        rig.send(Command::SetMaster(change));
        let around = largest_step(&rig.render(2_000));
        assert!(
            around < steady * 1.5 + 0.01,
            "{:?}: {around} vs {steady}",
            change.preset.name
        );
    }
}

#[test]
fn the_last_milliseconds_play_out_with_the_preview_on() {
    let frames = 3_000;
    let mut rig = rig(2);
    rig.send(Command::SetMaster(preview(0.0, 0.0, 0.0)));
    rig.send(Command::Load(Box::new(Track::from_stereo(
        &vec![0.25; frames],
        &vec![0.25; frames],
    ))));
    rig.send(Command::Play);
    let out = rig.render(frames + 2_000);
    let heard = out.chunks(2).filter(|frame| frame[0] != 0.0).count();
    // Every frame of the track comes out (the warm up runs silently first).
    assert!(heard >= frames - 1, "{heard} of {frames} frames heard");
    assert_eq!(rig.shared.ends.load(Ordering::Relaxed), 1);
}

#[test]
fn the_preview_stops_at_a_silent_point_while_b_is_not_heard() {
    let mut with_preview = playing(tone(437.3, 96_000, 0.3));
    with_preview.send(Command::SetMaster(preview(1.0, 6.0, -6.0)));
    with_preview.send(Command::SetMode(PlaybackMode::Mastered));
    with_preview.render(4_000);
    with_preview.send(Command::SetMode(PlaybackMode::Original));
    with_preview.send(Command::Seek(10_000));
    with_preview.render(FADE * 4);
    let mut plain = playing(tone(437.3, 96_000, 0.3));
    plain.send(Command::Seek(10_000));
    plain.render(FADE * 4);
    // After the seek both play the same frames: the preview (and its
    // delay) is off.
    let (a, b) = (with_preview.render(500), plain.render(500));
    assert_eq!(with_preview.position(), plain.position());
    for (x, y) in a.iter().zip(&b) {
        assert!((x - y).abs() < 1e-6, "{x} vs {y}");
    }
}

#[test]
fn level_match_follows_the_original_with_the_compressor_working() {
    // Club at 0.6 compresses this tone hard: the live meters still bring
    // the B side to the A side's loudness (and, without level match, add
    // back what the compressor took).
    let measure = |level_match: bool| {
        let mut rig = playing(tone(437.3, 48_000 * 6, 0.4));
        rig.send(Command::SetLevelMatch(level_match));
        rig.send(Command::SetMaster(preview(0.6, 3.0, -3.0)));
        rig.render(FADE * 4);
        let a = rms(&rig.render(24_000));
        rig.send(Command::SetMode(PlaybackMode::Mastered));
        rig.render(48_000 * 3); // the meters settle
        let b = rms(&rig.render(24_000));
        20.0 * (b / a).log10()
    };
    let matched = measure(true);
    assert!(matched.abs() < 1.0, "level matched: {matched} dB");
    let raw = measure(false);
    assert!(
        (raw - 3.0).abs() < 1.5,
        "the compressor's loss made up: {raw} dB"
    );
}
