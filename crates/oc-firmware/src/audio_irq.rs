//! The audio interrupt's registration and its NVIC line (journal/D-033):
//! the only `unsafe` of the audio path, kept apart as AGENTS.md asks. What
//! the interrupt does is in [`crate::audio`].
//!
//! GPT2's line keeps its reset priority, 0, the highest: no other interrupt
//! is enabled on the module (USB is polled), so nothing can delay it but the
//! loop's short critical sections and the boot ROM's flash calls.

#![allow(unsafe_code)]

use cortex_m::peripheral::NVIC;
use teensy4_bsp::interrupt;
use teensy4_bsp::rt::interrupt;

/// GPT2's compare: one audio period.
#[interrupt]
fn GPT2() {
    crate::audio::on_interrupt();
}

/// Lets GPT2's interrupt through; [`crate::audio::start`] has put its state
/// in place.
pub(crate) fn enable() {
    // SAFETY: unmasking can break a critical section built on masking this
    // line; the firmware's critical sections mask every interrupt instead
    // (`critical-section-single-core`, PRIMASK), and the handler only takes
    // state through them.
    unsafe { NVIC::unmask(interrupt::GPT2) };
}

/// Whether GPT2's line is enabled, and whether it is pending.
pub(crate) fn line_state() -> (bool, bool) {
    (
        NVIC::is_enabled(interrupt::GPT2),
        NVIC::is_pending(interrupt::GPT2),
    )
}

/// Blocks GPT2's interrupt again.
pub(crate) fn disable() {
    NVIC::mask(interrupt::GPT2);
}
