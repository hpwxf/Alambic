//! Modulating a parameter by a CV input (journal/D-035, D-036): the shaped
//! signal, `CENTER`, the usable ranges and the submenu's panel, apart from
//! any applet.

use oc_core::apps::InputSnapshot;
use oc_core::buttons::ButtonReader;
use oc_core::modulation::editor::AssignEditor;
use oc_core::modulation::{Assign, AssignLine, Effect, Source, WINDOW_MICROS, usable_range};
use oc_core::platform::{ControlEvents, CvChannel};

/// A parameter swept by 0 V to 7.75 V when it follows its input.
const SWEEP: (i32, i32) = (0, 7_750);

/// Drives an editor tick by tick, its buttons debounced.
struct Panel {
    editor: AssignEditor,
    assign: Assign,
    reader: ButtonReader,
    open: bool,
}

impl Panel {
    fn new() -> Self {
        Self {
            editor: AssignEditor::new(),
            assign: Assign::NONE,
            reader: ButtonReader::new(),
            open: true,
        }
    }

    /// One tick: buttons `down` (left switch, right switch, `up`, `down`),
    /// the encoders turned, `CV1` at `cv`. Once closed, the editor is left
    /// alone, as an applet drops it.
    fn step(&mut self, down: [bool; 4], turns: [i8; 2], cv: i32) {
        if !self.open {
            return;
        }
        let controls = ControlEvents {
            encoder_delta: turns,
            button_down: down,
        };
        let input = InputSnapshot {
            cv: [cv, 0, 0, 0],
            triggers: [false; 4],
            buttons: self.reader.update(&controls),
            controls,
            elapsed_micros: 1_000,
            ..InputSnapshot::default()
        };
        self.open = self.editor.follow(&mut self.assign, &input, SWEEP);
        if let Some(reading) = self.assign.raw(&input) {
            self.editor.observe(reading, 1_000);
        }
    }

    fn press(&mut self, index: usize) {
        for held in [true, true, true, true, false, false, false, false] {
            let mut down = [false; 4];
            down[index] = held;
            self.step(down, [0, 0], 0);
        }
    }

    /// Picks `line` with the left encoder, then turns it by `turns`.
    fn turn_line(&mut self, line: AssignLine, turns: i8) {
        while self.editor.line() != line {
            self.step([false; 4], [1, 0], 0);
        }
        self.step([false; 4], [0, turns], 0);
    }
}

#[test]
fn the_signal_is_scaled_shifted_then_bounded() {
    let mut assign = Assign {
        input: Some(Source::Jack(CvChannel::One)),
        scale_percent: 50,
        shift_mv: 500,
        max_mv: 2_000,
        ..Assign::NONE
    };
    assert_eq!(assign.shape(1_000), 1_000);
    assert_eq!(assign.shape(6_000), 2_000, "bounded after the shift");
    assign.scale_percent = -100;
    assert_eq!(assign.shape(1_000), -500, "a negative scale inverts");
    assert_eq!(assign.reading(1_000), Some((-500, Effect::Add)));
    assert_eq!(Assign::NONE.reading(1_000), None);
}

#[test]
fn the_usable_range_follows_the_effect() {
    // Followed, the sweep itself; added, its width centred on 0 V;
    // multiplying, x0 to x2 around x1.
    assert_eq!(usable_range(SWEEP, Effect::Set), (0, 7_750));
    assert_eq!(usable_range(SWEEP, Effect::Add), (-3_875, 3_875));
    assert_eq!(usable_range(SWEEP, Effect::Mul), (0, 10_000));
}

#[test]
fn center_carries_the_readings_seen_onto_the_usable_range() {
    let mut assign = Assign::NONE;
    // -3.6 V to 4.9 V onto +-3.875 V: x0.90 (8.5 V to 7.65 V), the middle,
    // 0.65 V, brought to 0 V by -0.60 V; the steps round both.
    assert!(assign.center((-3_600, 4_900), (-3_875, 3_875)));
    assert_eq!((assign.scale_percent, assign.shift_mv), (90, -600));
    assert_eq!((assign.shape(-3_600), assign.shape(4_900)), (-3_840, 3_810));
    // A narrow signal is scaled up as far as x2.00, then centred.
    assert!(assign.center((1_000, 2_000), (0, 10_000)));
    assert_eq!((assign.scale_percent, assign.shift_mv), (200, 2_000));
    // Too still to measure: left as it is.
    assert!(!assign.center((1_000, 1_050), (-3_875, 3_875)));
    assert_eq!((assign.scale_percent, assign.shift_mv), (200, 2_000));
}

#[test]
fn the_editor_turns_each_field_within_its_bounds() {
    let mut panel = Panel::new();
    assert_eq!(panel.editor.line(), AssignLine::Input);
    panel.turn_line(AssignLine::Input, 9);
    assert_eq!(
        panel.assign.input,
        Some(Source::Jack(CvChannel::Four)),
        "stops at CV4: no other instance"
    );
    panel.turn_line(AssignLine::Scale, 100);
    assert_eq!(panel.assign.scale_percent, 200);
    panel.turn_line(AssignLine::Shift, -3);
    assert_eq!(panel.assign.shift_mv, -150);
    panel.turn_line(AssignLine::Max, -120);
    panel.turn_line(AssignLine::Min, 127);
    assert_eq!(panel.assign.min_mv, panel.assign.max_mv, "MIN stops at MAX");
    panel.turn_line(AssignLine::Effect, 1);
    assert_eq!(panel.assign.effect, Effect::Mul);
    // The left encoder wraps round, from the last field to the first.
    panel.turn_line(AssignLine::Center, 0);
    panel.step([false; 4], [1, 0], 0);
    assert_eq!(panel.editor.line(), AssignLine::Input);
    assert!(panel.open);
}

#[test]
fn a_click_closes_the_editor_or_centres_on_center() {
    let mut panel = Panel::new();
    panel.turn_line(AssignLine::Input, 1);
    for cv in [-3_600, 4_900] {
        panel.step([false; 4], [0, 0], cv);
    }
    panel.turn_line(AssignLine::Center, 0);
    panel.press(1);
    assert!(panel.open, "CENTER applies and stays");
    // Added (the default): onto +-3.875 V.
    assert_eq!(
        (panel.assign.scale_percent, panel.assign.shift_mv),
        (90, -600)
    );
    panel.press(3);
    assert!(panel.open, "down is the panel's (journal/D-046)");
    panel.turn_line(AssignLine::Input, 0);
    panel.press(1);
    assert!(!panel.open, "a click off CENTER closes");
    let mut panel = Panel::new();
    panel.press(1);
    assert!(!panel.open, "a click elsewhere closes");
}

#[test]
fn the_range_seen_forgets_what_is_older_than_two_windows() {
    let mut panel = Panel::new();
    panel.turn_line(AssignLine::Input, 1);
    panel.step([false; 4], [0, 0], 5_000);
    assert_eq!(panel.editor.range(), Some((0, 5_000)));
    let ticks = 2 * WINDOW_MICROS / 1_000 + 1;
    for _ in 0..ticks {
        panel.step([false; 4], [0, 0], 1_000);
    }
    assert_eq!(panel.editor.range(), Some((1_000, 1_000)));
    // A new input starts the range again.
    panel.turn_line(AssignLine::Input, 1);
    assert_eq!(panel.editor.range(), Some((0, 0)));
}

#[test]
fn a_port_of_another_instance_is_assigned_as_it_comes() {
    use oc_core::fmt::TextBuf;

    // Scale, shift, bounds and effect as a jack would have them...
    let assign = Assign {
        input: Source::lane(0, 0),
        effect: Effect::Mul,
        scale_percent: 50,
        shift_mv: 1_000,
        max_mv: 2_000,
        ..Assign::NONE
    };
    // ...act on nothing: the port sets the parameter, unshaped.
    assert!(assign.assigns() && assign.sets());
    assert_eq!(assign.reading(4_250), Some((4_250, Effect::Set)));
    assert_eq!(assign.shaped(4_250), 4_250);
    let jack = Assign {
        input: Some(Source::Jack(CvChannel::One)),
        ..assign
    };
    assert!(!jack.assigns());
    assert_eq!(
        jack.reading(4_250),
        Some((2_000, Effect::Mul)),
        "kept for a jack"
    );

    // Assigned: `=`, the slot and the port's name.
    let mut name = TextBuf::<8>::new();
    assign.write(&mut name, Some("NOTE"));
    assert_eq!(name.as_str(), "=1:NOTE", "the main screen");
    let mut long = TextBuf::<8>::new();
    assign.write(&mut long, Some("L/MONO"));
    assert_eq!(long.as_str(), "=1:L/MON", "five letters at most");
    let mut gone = TextBuf::<8>::new();
    assign.write(&mut gone, None);
    assert_eq!(gone.as_str(), "=1:--", "a port that is no more");
    let mut cv = TextBuf::<8>::new();
    jack.write(&mut cv, None);
    assert_eq!(cv.as_str(), "*CV1");
}
