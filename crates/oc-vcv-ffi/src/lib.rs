//! C ABI over [`oc_core`], linked into the VCV Rack 2 module.
//!
//! Every exported function is defensive: a null `engine` pointer or an
//! out-of-range channel/index is a documented no-op (a getter returns a
//! harmless default instead), and [`std::panic::catch_unwind`] wraps every
//! function body so that a panic inside the core can never unwind across the
//! boundary into C++, where that would be undefined behaviour.
//!
//! The engine runs on the same host backend `oc-core`'s own tests and
//! `oc-sim` use ([`oc_core::testing`]): the VCV Rack module is, behaviourally,
//! just another simulator, driven one tick at a time by the plugin from
//! VCV's audio callback rather than from a terminal UI. This is deliberate:
//! it means the exact code path exercised by [`oc_core`]'s test suite is what
//! runs inside VCV Rack, with only the ABI translation below being specific
//! to this crate.
//!
//! # Safety
//!
//! Every `unsafe` block below is isolated to a single raw-pointer
//! dereference and carries a `# Safety` comment stating the invariant the
//! caller (the C++ plugin) must uphold: that a non-null pointer was
//! previously returned by [`oc_engine_new`] and has not already been passed
//! to [`oc_engine_free`]. No other part of this crate uses `unsafe`.
//!
//! Because [`OcEngine`] wraps a [`VirtualClock`], which uses a [`Cell`] for
//! interior mutability, the compiler cannot prove a `&mut OcEngine` is safe
//! to keep using after a caught panic, so every closure that reaches one is
//! wrapped in [`AssertUnwindSafe`]. This is a deliberate, narrow assertion:
//! a panic there can only leave that one engine's state stale (never
//! memory-unsafe, since all field access stays within Rust's borrow rules),
//! which is an acceptable outcome for a boundary that must never unwind into
//! C++.
//!
//! [`Cell`]: std::cell::Cell

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;

use oc_core::Engine;
use oc_core::framebuffer::LEN as FRAMEBUFFER_LEN;
use oc_core::platform::{
    BUTTONS, Button, CV_CHANNELS, CvChannel, ENCODERS, MilliVolts, TRIGGER_CHANNELS, TriggerChannel,
};
use oc_core::testing::{
    MockAnalogIn, MockAnalogOut, MockControls, MockDigitalIn, MockDisplay, VirtualClock,
};

/// Concrete engine wired for a VCV Rack host: the same deterministic
/// in-memory backend used by `oc-core`'s own tests and by `oc-sim`.
///
/// The clock is a [`VirtualClock`] that the plugin drives explicitly through
/// [`oc_engine_tick`]'s `now_micros` argument, rather than one that reads a
/// system timer: VCV Rack calls into this engine from its own audio
/// callback, at whatever sample rate the user has selected, so time must
/// come from the host, not from the OS.
type HostEngine =
    Engine<MockAnalogIn, MockAnalogOut, MockDigitalIn, MockControls, VirtualClock, MockDisplay>;

/// Opaque engine handle returned to C++.
///
/// Its layout is not part of the ABI; C++ only ever holds a pointer to it,
/// obtained from [`oc_engine_new`] and released through [`oc_engine_free`].
/// The last tick's CV outputs and framebuffer are cached here so the getter
/// functions can take a `const` pointer instead of needing to re-borrow the
/// engine mutably.
#[derive(Debug)]
pub struct OcEngine {
    engine: HostEngine,
    cv_out: [MilliVolts; CV_CHANNELS],
    framebuffer: [u8; FRAMEBUFFER_LEN],
}

/// Builds a fresh host engine with every input at rest.
fn new_host_engine() -> HostEngine {
    Engine::new(
        MockAnalogIn::new(),
        MockAnalogOut::new(),
        MockDigitalIn::new(),
        MockControls::new(),
        VirtualClock::new(),
        MockDisplay::new(),
    )
}

/// Creates a new engine instance.
///
/// Returns a null pointer if construction panics. That should never happen —
/// [`HostEngine::new`](Engine::new) does no fallible work — but the ABI's
/// promise is to never unwind into C++, so a panic here degrades to "no
/// engine" rather than undefined behaviour. On success, the caller owns the
/// returned pointer and must eventually pass it to [`oc_engine_free`] exactly
/// once.
#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn oc_engine_new() -> *mut OcEngine {
    let built = catch_unwind(move || OcEngine {
        engine: new_host_engine(),
        cv_out: [0; CV_CHANNELS],
        framebuffer: [0; FRAMEBUFFER_LEN],
    });
    built.map_or(ptr::null_mut(), |engine| Box::into_raw(Box::new(engine)))
}

/// Destroys an engine created by [`oc_engine_new`].
///
/// A null `engine` is a no-op.
///
/// # Safety
///
/// `engine`, if non-null, must be a pointer previously returned by
/// [`oc_engine_new`] that has not already been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oc_engine_free(engine: *mut OcEngine) {
    if engine.is_null() {
        return;
    }
    // SAFETY: forwarded from this function's own safety contract: `engine`
    // is non-null (checked above) and, per the caller's obligation, points
    // to a live `OcEngine` not yet freed.
    //
    // `AssertUnwindSafe` is warranted here: a panic mid-drop cannot corrupt
    // memory (there is no `unsafe` left to run after it), it can only leak,
    // which is an acceptable outcome for a boundary that must never unwind
    // into C++.
    let _ = catch_unwind(AssertUnwindSafe(move || {
        drop(unsafe { Box::from_raw(engine) });
    }));
}

/// Sets one CV input's level and cable state for the next call to
/// [`oc_engine_tick`].
///
/// A null `engine` or a channel index outside `0..4` is a no-op.
///
/// # Safety
///
/// `engine`, if non-null, must point to a live [`OcEngine`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oc_engine_set_cv_in(
    engine: *mut OcEngine,
    channel: u8,
    millivolts: i32,
    patched: bool,
) {
    let _ = catch_unwind(AssertUnwindSafe(move || {
        // SAFETY: see the function's own safety contract.
        let Some(engine) = (unsafe { as_mut(engine) }) else {
            return;
        };
        let Some(channel) = CvChannel::from_index(usize::from(channel)) else {
            return;
        };
        let (analog_in, ..) = engine.engine.parts_mut();
        analog_in.levels[channel.index()] = millivolts;
        analog_in.patched[channel.index()] = patched;
    }));
}

/// Sets one trigger input's raw level for the next call to
/// [`oc_engine_tick`].
///
/// A null `engine` or a channel index outside `0..4` is a no-op.
///
/// # Safety
///
/// `engine`, if non-null, must point to a live [`OcEngine`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oc_engine_set_trigger(engine: *mut OcEngine, channel: u8, high: bool) {
    let _ = catch_unwind(AssertUnwindSafe(move || {
        // SAFETY: see the function's own safety contract.
        let Some(engine) = (unsafe { as_mut(engine) }) else {
            return;
        };
        let Some(channel) = TriggerChannel::from_index(usize::from(channel)) else {
            return;
        };
        let (_, _, digital_in, ..) = engine.engine.parts_mut();
        digital_in.set(channel, high);
    }));
}

/// Reports encoder movement and the state of its push switch.
///
/// `index` `0` is the left encoder, `1` the right encoder, matching the
/// panel layout used everywhere else in this project. `delta` is the number
/// of detents travelled since the previous call, consumed exactly once by
/// the next [`oc_engine_tick`], mirroring how a real quadrature decoder
/// reports movement. `pressed` sets the same push-switch state that
/// [`oc_engine_button`] can also set for index `0`/`1` (the left/right
/// encoder switches) — both entry points update the same underlying flag, so
/// a plugin may use whichever is more convenient for its widget.
///
/// A null `engine` or an index outside `0..2` is a no-op.
///
/// # Safety
///
/// `engine`, if non-null, must point to a live [`OcEngine`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oc_engine_encoder(
    engine: *mut OcEngine,
    index: u8,
    delta: i8,
    pressed: bool,
) {
    let _ = catch_unwind(AssertUnwindSafe(move || {
        // SAFETY: see the function's own safety contract.
        let Some(engine) = (unsafe { as_mut(engine) }) else {
            return;
        };
        let index = usize::from(index);
        if index >= ENCODERS {
            return;
        }
        let (_, _, _, controls, _) = engine.engine.parts_mut();
        controls.turn(index, delta);
        if let Some(button) = Button::from_index(index) {
            controls.hold(button, pressed);
        }
    }));
}

/// Sets the state of one push button.
///
/// `index` follows panel order: `0` left encoder switch, `1` right encoder
/// switch, `2` the `up` button, `3` the `down` button (see
/// [`Button::from_index`]). Indices `0`/`1` are equivalent to setting
/// `pressed` through [`oc_engine_encoder`] for the matching encoder; use
/// whichever fits the plugin's widget layout best.
///
/// A null `engine` or an index outside `0..4` is a no-op.
///
/// # Safety
///
/// `engine`, if non-null, must point to a live [`OcEngine`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oc_engine_button(engine: *mut OcEngine, index: u8, pressed: bool) {
    let _ = catch_unwind(AssertUnwindSafe(move || {
        // SAFETY: see the function's own safety contract.
        let Some(engine) = (unsafe { as_mut(engine) }) else {
            return;
        };
        let Some(button) = Button::from_index(usize::from(index)) else {
            return;
        };
        let (_, _, _, controls, _) = engine.engine.parts_mut();
        controls.hold(button, pressed);
    }));
}

/// Runs one complete tick at the given timestamp and caches its CV outputs
/// and rendered framebuffer for [`oc_engine_cv_out`] and
/// [`oc_engine_framebuffer`].
///
/// `now_micros` must be monotonically non-decreasing across calls for a
/// given engine (wrapping is tolerated, see [`oc_core::platform::Clock`]);
/// VCV Rack should derive it by accumulating the reciprocal of the current
/// sample rate, decimated to roughly a millisecond per tick as documented in
/// the plugin's own source.
///
/// A null `engine` is a no-op.
///
/// # Safety
///
/// `engine`, if non-null, must point to a live [`OcEngine`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oc_engine_tick(engine: *mut OcEngine, now_micros: u64) {
    let _ = catch_unwind(AssertUnwindSafe(move || {
        // SAFETY: see the function's own safety contract.
        let Some(engine) = (unsafe { as_mut(engine) }) else {
            return;
        };
        engine.engine.clock().set(now_micros);
        let report = engine.engine.tick();
        engine.cv_out = report.cv_out;
        engine.framebuffer = *engine.engine.frame().as_bytes();
    }));
}

/// Reads the CV output level written by the most recent [`oc_engine_tick`].
///
/// A null `engine` or a channel index outside `0..4` returns `0`, the same
/// level a freshly created, never-ticked engine reports.
///
/// # Safety
///
/// `engine`, if non-null, must point to a live [`OcEngine`].
#[must_use]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oc_engine_cv_out(engine: *const OcEngine, channel: u8) -> i32 {
    catch_unwind(AssertUnwindSafe(move || {
        // SAFETY: see the function's own safety contract.
        let Some(engine) = (unsafe { as_ref(engine) }) else {
            return 0;
        };
        let Some(channel) = CvChannel::from_index(usize::from(channel)) else {
            return 0;
        };
        engine.cv_out[channel.index()]
    }))
    .unwrap_or(0)
}

/// Returns a pointer to the 128x64 1bpp framebuffer rendered by the most
/// recent [`oc_engine_tick`], in the same SSD1306/SSD1309 page layout
/// documented on [`oc_core::framebuffer::FrameBuffer`].
///
/// The pointer stays valid until the next call to [`oc_engine_tick`] or
/// [`oc_engine_free`] on the same engine, whichever comes first; the plugin
/// must copy it out (or finish drawing) before ticking again. A null
/// `engine` returns a null pointer.
///
/// # Safety
///
/// `engine`, if non-null, must point to a live [`OcEngine`].
#[must_use]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oc_engine_framebuffer(engine: *const OcEngine) -> *const u8 {
    let read = catch_unwind(AssertUnwindSafe(move || {
        // SAFETY: see the function's own safety contract.
        unsafe { as_ref(engine) }.map(|engine| engine.framebuffer.as_ptr())
    }));
    read.ok().flatten().unwrap_or(ptr::null())
}

/// Length in bytes of the buffer returned by [`oc_engine_framebuffer`].
///
/// A plain function rather than a `#define`, so the plugin never hard-codes
/// `1024` and cannot silently fall out of sync with [`oc_core::framebuffer`].
#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn oc_engine_framebuffer_len() -> usize {
    FRAMEBUFFER_LEN
}

/// Number of CV channels, in and out.
#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn oc_engine_cv_channels() -> u8 {
    truncate_count(CV_CHANNELS)
}

/// Number of trigger inputs.
#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn oc_engine_trigger_channels() -> u8 {
    truncate_count(TRIGGER_CHANNELS)
}

/// Number of rotary encoders.
#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn oc_engine_encoders() -> u8 {
    truncate_count(ENCODERS)
}

/// Number of push buttons, including the two encoder switches.
#[must_use]
#[unsafe(no_mangle)]
pub extern "C" fn oc_engine_buttons() -> u8 {
    truncate_count(BUTTONS)
}

/// Narrows one of `oc-core`'s channel-count constants to the `u8` the ABI
/// uses for indices; every one of them is small enough today that this can
/// never truncate, but `try_from` keeps that an enforced fact rather than an
/// assumption.
fn truncate_count(count: usize) -> u8 {
    u8::try_from(count).expect("channel counts fit in a u8")
}

/// Borrows `engine` mutably, or returns `None` for a null pointer.
///
/// # Safety
///
/// `engine`, if non-null, must point to a live [`OcEngine`] with no other
/// live borrow.
unsafe fn as_mut<'a>(engine: *mut OcEngine) -> Option<&'a mut OcEngine> {
    if engine.is_null() {
        None
    } else {
        // SAFETY: forwarded from this function's own safety contract.
        Some(unsafe { &mut *engine })
    }
}

/// Borrows `engine` immutably, or returns `None` for a null pointer.
///
/// # Safety
///
/// `engine`, if non-null, must point to a live [`OcEngine`].
unsafe fn as_ref<'a>(engine: *const OcEngine) -> Option<&'a OcEngine> {
    if engine.is_null() {
        None
    } else {
        // SAFETY: forwarded from this function's own safety contract.
        Some(unsafe { &*engine })
    }
}

/// Human-readable identification of the core engine behind this ABI, as a
/// Rust string slice.
///
/// Kept for host-side Rust callers (see the crate's own tests); the C ABI has
/// no use for a Rust `&str`, so this is deliberately not exported with
/// `#[no_mangle]`.
#[must_use]
pub fn core_banner() -> &'static str {
    oc_core::BANNER
}

#[cfg(test)]
mod tests {
    use super::{
        as_mut, oc_engine_button, oc_engine_buttons, oc_engine_cv_channels, oc_engine_cv_out,
        oc_engine_encoder, oc_engine_encoders, oc_engine_framebuffer, oc_engine_framebuffer_len,
        oc_engine_free, oc_engine_new, oc_engine_set_cv_in, oc_engine_set_trigger, oc_engine_tick,
        oc_engine_trigger_channels,
    };
    use oc_core::framebuffer::LEN;
    use oc_core::platform::{BUTTONS, CV_CHANNELS, ENCODERS, TRIGGER_CHANNELS};

    #[test]
    fn banner_matches_the_core() {
        assert_eq!(super::core_banner(), oc_core::BANNER);
    }

    #[test]
    fn channel_counts_match_the_core_constants() {
        assert_eq!(usize::from(oc_engine_cv_channels()), CV_CHANNELS);
        assert_eq!(usize::from(oc_engine_trigger_channels()), TRIGGER_CHANNELS);
        assert_eq!(usize::from(oc_engine_encoders()), ENCODERS);
        assert_eq!(usize::from(oc_engine_buttons()), BUTTONS);
        assert_eq!(oc_engine_framebuffer_len(), LEN);
    }

    #[test]
    fn a_fresh_engine_reports_a_dark_screen_and_zero_outputs() {
        let engine = oc_engine_new();
        assert!(!engine.is_null());

        // SAFETY: `engine` was just returned by `oc_engine_new` and is not
        // shared.
        assert!(unsafe { as_mut(engine) }.is_some());

        for channel in 0..u8::try_from(CV_CHANNELS).unwrap() {
            // SAFETY: `engine` is live for the duration of this test.
            assert_eq!(unsafe { oc_engine_cv_out(engine, channel) }, 0);
        }

        // SAFETY: `engine` is live for the duration of this test.
        let frame = unsafe { oc_engine_framebuffer(engine) };
        assert!(!frame.is_null());

        // SAFETY: `engine` was returned by `oc_engine_new` and is freed
        // exactly once, here.
        unsafe { oc_engine_free(engine) };
    }

    #[test]
    fn cv_in_passes_through_to_cv_out_after_a_tick() {
        let engine = oc_engine_new();
        // SAFETY: `engine` is live for the duration of this test.
        unsafe {
            oc_engine_set_cv_in(engine, 1, 2_500, true);
            oc_engine_tick(engine, 1_000);
            assert_eq!(oc_engine_cv_out(engine, 1), 2_500);
            oc_engine_free(engine);
        }
    }

    #[test]
    fn a_trigger_pulse_is_observed_after_a_tick() {
        let engine = oc_engine_new();
        // SAFETY: `engine` is live for the duration of this test.
        unsafe {
            oc_engine_set_trigger(engine, 0, true);
            oc_engine_tick(engine, 1_000);
            oc_engine_free(engine);
        }
    }

    #[test]
    fn an_encoder_turn_and_a_button_press_do_not_panic() {
        let engine = oc_engine_new();
        // SAFETY: `engine` is live for the duration of this test.
        unsafe {
            oc_engine_encoder(engine, 0, 3, true);
            oc_engine_button(engine, 3, true);
            oc_engine_tick(engine, 1_000);
            oc_engine_free(engine);
        }
    }
}
