//! Contracts between the core and its host platform.
//!
//! Everything the core needs from the outside world is expressed here. The
//! traits are deliberately narrow and synchronous: a backend is a handful of
//! getters and setters, which is what makes the firmware, the simulator and
//! the VCV Rack module interchangeable.

use crate::framebuffer::FrameBuffer;

/// A signal level in millivolts.
///
/// Millivolts as `i32` are the internal unit everywhere: they cover the module
/// ranges with room to spare, they are exact, and they keep floating point out
/// of the firmware's critical path. Conversion to `f32` happens only at the
/// VCV Rack boundary.
pub type MilliVolts = i32;

/// Number of CV inputs, and of CV outputs.
pub const CV_CHANNELS: usize = 4;

/// Number of trigger (gate) inputs.
pub const TRIGGER_CHANNELS: usize = 4;

/// Number of rotary encoders.
pub const ENCODERS: usize = 2;

/// Number of push buttons, including the two encoder switches.
pub const BUTTONS: usize = 4;

/// One of the four CV channels.
///
/// The same indices address the four inputs (`CV1`..`CV4` on the panel) and
/// the four outputs (`A`..`D` on the panel).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CvChannel {
    /// Input `CV1`, output `A`.
    One,
    /// Input `CV2`, output `B`.
    Two,
    /// Input `CV3`, output `C`.
    Three,
    /// Input `CV4`, output `D`.
    Four,
}

/// One of the four trigger inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TriggerChannel {
    /// Trigger input `TR1`.
    One,
    /// Trigger input `TR2`.
    Two,
    /// Trigger input `TR3`.
    Three,
    /// Trigger input `TR4`.
    Four,
}

/// A push button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Button {
    /// Switch of the left encoder.
    LeftEncoder,
    /// Switch of the right encoder.
    RightEncoder,
    /// The `up` button.
    Up,
    /// The `down` button.
    Down,
}

/// Generates index conversions for a fixed-size channel enum.
macro_rules! indexed_enum {
    ($name:ident, $count:expr, [$($variant:ident),+ $(,)?]) => {
        impl $name {
            /// All variants, in index order.
            pub const ALL: [Self; $count] = [$(Self::$variant),+];

            /// Zero-based index of this variant.
            #[must_use]
            pub const fn index(self) -> usize {
                self as usize
            }

            /// The variant with the given index, or `None` when out of range.
            #[must_use]
            pub const fn from_index(index: usize) -> Option<Self> {
                if index < $count {
                    Some(Self::ALL[index])
                } else {
                    None
                }
            }
        }
    };
}

indexed_enum!(CvChannel, CV_CHANNELS, [One, Two, Three, Four]);
indexed_enum!(TriggerChannel, TRIGGER_CHANNELS, [One, Two, Three, Four]);
indexed_enum!(Button, BUTTONS, [LeftEncoder, RightEncoder, Up, Down]);

/// Raw control state sampled during one tick.
///
/// Encoder movement is reported as a *delta in detents* since the previous
/// poll, so a backend is free to decode quadrature in hardware, in an
/// interrupt, or with [`crate::encoder::QuadratureDecoder`]. Buttons are
/// reported as raw levels; edge detection and debouncing belong to the core.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ControlEvents {
    /// Detents travelled by each encoder since the previous poll.
    pub encoder_delta: [i8; ENCODERS],
    /// Whether each button is currently held down.
    pub button_down: [bool; BUTTONS],
}

impl ControlEvents {
    /// Movement of one encoder, in detents.
    #[must_use]
    pub fn delta(&self, encoder: usize) -> i8 {
        self.encoder_delta.get(encoder).copied().unwrap_or(0)
    }

    /// Whether a button is held down.
    #[must_use]
    pub fn is_down(&self, button: Button) -> bool {
        self.button_down
            .get(button.index())
            .copied()
            .unwrap_or(false)
    }
}

/// Reads the CV inputs.
pub trait AnalogIn {
    /// Samples one CV input and returns its calibrated level.
    fn read_cv(&mut self, channel: CvChannel) -> MilliVolts;

    /// Whether a cable is known to be plugged into this input.
    ///
    /// The real module has no cable detection, so the firmware reports `true`
    /// unconditionally and the core relies on
    /// [`crate::signal::SignalDetector`] instead. Hosts that do know, such as
    /// VCV Rack, report the truth.
    fn is_patched(&self, channel: CvChannel) -> bool;
}

/// Writes the CV outputs.
pub trait AnalogOut {
    /// Stages a value for one CV output.
    fn write_cv(&mut self, channel: CvChannel, value: MilliVolts);

    /// Pushes all staged values to the hardware.
    ///
    /// Staging then flushing lets the DAC driver write the four channels in a
    /// single SPI transaction, so the outputs move together.
    fn flush(&mut self);
}

/// Reads the trigger inputs.
pub trait DigitalIn {
    /// Raw, undebounced level of one trigger input.
    fn trigger_state(&self, channel: TriggerChannel) -> bool;
}

/// Reads the encoders and buttons.
pub trait Controls {
    /// Consumes and returns the control activity since the previous call.
    fn poll(&mut self) -> ControlEvents;
}

/// Monotonic time source.
pub trait Clock {
    /// Microseconds since an arbitrary origin.
    ///
    /// Must be monotonic. The core compares timestamps with wrapping
    /// arithmetic, so a source that wraps is acceptable.
    fn now_micros(&self) -> u64;
}

/// The module's 128x64 monochrome screen.
pub trait Display {
    /// The framebuffer the core draws into.
    fn frame_mut(&mut self) -> &mut FrameBuffer;

    /// Sends the framebuffer to the screen.
    fn present(&mut self);
}

#[cfg(test)]
mod tests {
    use super::{Button, CvChannel, TriggerChannel};

    #[test]
    fn indices_are_dense_and_ordered() {
        for (expected, channel) in CvChannel::ALL.into_iter().enumerate() {
            assert_eq!(channel.index(), expected);
            assert_eq!(CvChannel::from_index(expected), Some(channel));
        }
        assert_eq!(CvChannel::from_index(4), None);
        assert_eq!(TriggerChannel::from_index(4), None);
        assert_eq!(Button::from_index(4), None);
    }

    #[test]
    fn buttons_are_named_in_panel_order() {
        assert_eq!(Button::LeftEncoder.index(), 0);
        assert_eq!(Button::Down.index(), 3);
    }
}
