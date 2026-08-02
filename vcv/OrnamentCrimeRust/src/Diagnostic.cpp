#include <cassert>
#include <cmath>
#include <cstdint>

#include "oc_vcv_ffi.h"
#include "plugin.hpp"

namespace {

// Panel layout constants. These mirror the panel counts baked into the ABI
// (`oc_engine_cv_channels()` and friends); `Diagnostic`'s constructor asserts
// they still agree, so a future change to `oc-core`'s panel shape fails loudly
// here instead of silently misrouting a jack.
constexpr int kCvChannels = 4;
constexpr int kTriggerChannels = 4;
constexpr int kEncoders = 2;
constexpr int kButtons = 4;

// A trigger input is considered high above this voltage. Rack has no single
// fixed convention for gate/trigger thresholds; 1 V matches the level most
// Eurorack sequencers and the `oc-core` firmware's own debouncer expect.
constexpr float kTriggerHighVolts = 1.f;

// The engine is ticked at roughly this rate regardless of the audio sample
// rate, matching the firmware's own 1 kHz main loop (`oc-firmware`).
constexpr double kTickIntervalMicros = 1000.0;

// Rack's knob widgets are bounded, not infinite like a physical rotary
// encoder, so a knob's *movement* between two process() calls -- not its
// absolute position -- stands in for encoder detents. `kDetentsPerFullSwing`
// sets how many detents a full excursion from one end of the knob to the
// other is worth; this is a display/interaction choice, not a behavioural
// one, since every actual detent still goes through the same
// `oc_engine_encoder()` call the real hardware's quadrature decoder would
// produce.
constexpr float kDetentsPerFullSwing = 32.f;

// Framebuffer geometry, mirrored from `oc_core::framebuffer` (128x64, 1bpp,
// 8-row pages) purely to know how to unpack the bytes `oc_engine_framebuffer`
// returns; no pixel decisions are made here, only byte-to-bit unpacking.
constexpr int kScreenWidth = 128;
constexpr int kScreenHeight = 64;

} // namespace

struct Diagnostic : Module {
    enum ParamId {
        LEFT_ENCODER_PARAM,
        RIGHT_ENCODER_PARAM,
        LEFT_BUTTON_PARAM,
        RIGHT_BUTTON_PARAM,
        UP_PARAM,
        DOWN_PARAM,
        PARAMS_LEN
    };
    enum InputId {
        CV1_INPUT,
        CV2_INPUT,
        CV3_INPUT,
        CV4_INPUT,
        TR1_INPUT,
        TR2_INPUT,
        TR3_INPUT,
        TR4_INPUT,
        INPUTS_LEN
    };
    enum OutputId { A_OUTPUT, B_OUTPUT, C_OUTPUT, D_OUTPUT, OUTPUTS_LEN };
    enum LightId { LIGHTS_LEN };

    // Owned by this module; created in the constructor, released in the
    // destructor. `OcEngine` is an opaque type on the C++ side -- only the
    // Rust side of the ABI ever looks inside it.
    OcEngine *engine = nullptr;

    // Accumulates fractional microseconds between process() calls so that
    // oc_engine_tick() is called at a fixed ~1 kHz regardless of the host's
    // sample rate (44.1 kHz to 192 kHz per the project's requirements).
    double microsAccumulator = 0.0;
    uint64_t totalMicros = 0;

    // Previous knob reading for each encoder, to turn absolute knob position
    // into a delta since the last process() call.
    float encoderPrev[kEncoders] = {};
    // Fractional part of the detent count not yet reported, carried over so
    // slow turns still eventually produce a detent instead of always
    // rounding to zero.
    float encoderRemainder[kEncoders] = {};

    Diagnostic() {
        // A mismatch here would mean this widget's port/param layout no
        // longer matches the ABI it was written against; fail immediately
        // rather than silently reading or writing the wrong channel.
        assert(oc_engine_cv_channels() == kCvChannels);
        assert(oc_engine_trigger_channels() == kTriggerChannels);
        assert(oc_engine_encoders() == kEncoders);
        assert(oc_engine_buttons() == kButtons);

        config(PARAMS_LEN, INPUTS_LEN, OUTPUTS_LEN, LIGHTS_LEN);
        configParam(LEFT_ENCODER_PARAM, -1.f, 1.f, 0.f, "Left encoder (channel select)");
        configParam(RIGHT_ENCODER_PARAM, -1.f, 1.f, 0.f, "Right encoder (offset)");
        configButton(LEFT_BUTTON_PARAM, "Left encoder switch (reset trigger counters)");
        configButton(RIGHT_BUTTON_PARAM, "Right encoder switch (zero the offset)");
        configButton(UP_PARAM, "Up (next output mode)");
        configButton(DOWN_PARAM, "Down (previous output mode)");

        configInput(CV1_INPUT, "CV 1");
        configInput(CV2_INPUT, "CV 2");
        configInput(CV3_INPUT, "CV 3");
        configInput(CV4_INPUT, "CV 4");
        configInput(TR1_INPUT, "Trigger 1");
        configInput(TR2_INPUT, "Trigger 2");
        configInput(TR3_INPUT, "Trigger 3");
        configInput(TR4_INPUT, "Trigger 4");

        configOutput(A_OUTPUT, "A");
        configOutput(B_OUTPUT, "B");
        configOutput(C_OUTPUT, "C");
        configOutput(D_OUTPUT, "D");

        engine = oc_engine_new();
    }

    ~Diagnostic() override {
        // A null `engine` (construction panicked, see oc_engine_new's own
        // documentation) is a defined no-op on the Rust side.
        oc_engine_free(engine);
    }

    // Converts a knob's movement since the last call into whole detents,
    // carrying the fractional remainder forward so it is never lost.
    int8_t encoderDelta(int index, float value) {
        const float moved = value - encoderPrev[index];
        encoderPrev[index] = value;
        encoderRemainder[index] += moved * kDetentsPerFullSwing / 2.f;

        const float whole = std::trunc(encoderRemainder[index]);
        encoderRemainder[index] -= whole;

        const float clamped = std::fmax(-127.f, std::fmin(127.f, whole));
        return static_cast<int8_t>(clamped);
    }

    void process(const ProcessArgs &args) override {
        if (!engine) {
            return;
        }

        for (int channel = 0; channel < kCvChannels; ++channel) {
            Input &input = inputs[CV1_INPUT + channel];
            const int32_t millivolts = static_cast<int32_t>(std::lround(input.getVoltage() * 1000.f));
            oc_engine_set_cv_in(engine, static_cast<uint8_t>(channel), millivolts, input.isConnected());
        }

        for (int channel = 0; channel < kTriggerChannels; ++channel) {
            const bool high = inputs[TR1_INPUT + channel].getVoltage() >= kTriggerHighVolts;
            oc_engine_set_trigger(engine, static_cast<uint8_t>(channel), high);
        }

        oc_engine_encoder(engine, 0, encoderDelta(0, params[LEFT_ENCODER_PARAM].getValue()),
                           params[LEFT_BUTTON_PARAM].getValue() > 0.5f);
        oc_engine_encoder(engine, 1, encoderDelta(1, params[RIGHT_ENCODER_PARAM].getValue()),
                           params[RIGHT_BUTTON_PARAM].getValue() > 0.5f);
        oc_engine_button(engine, 2, params[UP_PARAM].getValue() > 0.5f);
        oc_engine_button(engine, 3, params[DOWN_PARAM].getValue() > 0.5f);

        microsAccumulator += static_cast<double>(args.sampleTime) * 1e6;
        while (microsAccumulator >= kTickIntervalMicros) {
            microsAccumulator -= kTickIntervalMicros;
            totalMicros += static_cast<uint64_t>(kTickIntervalMicros);
            oc_engine_tick(engine, totalMicros);
        }

        for (int channel = 0; channel < kCvChannels; ++channel) {
            const int32_t millivolts = oc_engine_cv_out(engine, static_cast<uint8_t>(channel));
            outputs[A_OUTPUT + channel].setVoltage(static_cast<float>(millivolts) / 1000.f);
        }
    }
};

// Renders the module's 128x64 monochrome screen by reading the framebuffer
// exposed through the ABI; it decides nothing about what is on it.
struct DiagnosticScreen : TransparentWidget {
    Diagnostic *module = nullptr;
    // Size, in pixels, of one framebuffer pixel once drawn on the panel.
    // Chosen so that 128 x 64 framebuffer pixels exactly fill the widget's
    // 46mm x 23mm box (46 * MM2PX / 128 == 23 * MM2PX / 64, since 46:23
    // matches the framebuffer's 128:64 aspect ratio) instead of overflowing
    // or falling short of it.
    static constexpr float kPixelSize = 1.0611467f;

    void draw(const DrawArgs &args) override {
        nvgBeginPath(args.vg);
        nvgRect(args.vg, 0.f, 0.f, box.size.x, box.size.y);
        nvgFillColor(args.vg, nvgRGB(0x10, 0x10, 0x10));
        nvgFill(args.vg);

        if (!module || !module->engine) {
            return;
        }
        const uint8_t *frame = oc_engine_framebuffer(module->engine);
        if (!frame) {
            return;
        }

        nvgFillColor(args.vg, nvgRGB(0xe0, 0xe0, 0xe0));
        for (int y = 0; y < kScreenHeight; ++y) {
            const int page = y / 8;
            const uint8_t mask = static_cast<uint8_t>(1u << (y % 8));
            for (int x = 0; x < kScreenWidth; ++x) {
                const uint8_t byte = frame[page * kScreenWidth + x];
                if ((byte & mask) == 0) {
                    continue;
                }
                nvgBeginPath(args.vg);
                nvgRect(args.vg, static_cast<float>(x) * kPixelSize, static_cast<float>(y) * kPixelSize, kPixelSize,
                        kPixelSize);
                nvgFill(args.vg);
            }
        }
    }
};

struct DiagnosticWidget : ModuleWidget {
    explicit DiagnosticWidget(Diagnostic *module) {
        setModule(module);
        setPanel(createPanel(asset::plugin(pluginInstance, "res/Diagnostic.svg")));

        addChild(createWidget<ScrewSilver>(Vec(RACK_GRID_WIDTH, 0)));
        addChild(createWidget<ScrewSilver>(Vec(box.size.x - 2 * RACK_GRID_WIDTH, 0)));
        addChild(createWidget<ScrewSilver>(Vec(RACK_GRID_WIDTH, RACK_GRID_HEIGHT - RACK_GRID_WIDTH)));
        addChild(createWidget<ScrewSilver>(Vec(box.size.x - 2 * RACK_GRID_WIDTH, RACK_GRID_HEIGHT - RACK_GRID_WIDTH)));

        // Two encoder columns, positioned to straddle the screen the same
        // way the real O&C hardware lays out UP/DOWN over left/right knobs.
        constexpr float kLeftColumn = 21.10f;
        constexpr float kRightColumn = 50.02f;

        // UP/DOWN sit just under the screen, one per column.
        addParam(createParamCentered<TL1105>(mm2px(Vec(kLeftColumn, 38.f)), module, Diagnostic::UP_PARAM));
        addParam(createParamCentered<TL1105>(mm2px(Vec(kRightColumn, 38.f)), module, Diagnostic::DOWN_PARAM));

        // The two rotary encoders themselves.
        addParam(createParamCentered<RoundBlackKnob>(mm2px(Vec(kLeftColumn, 50.f)), module,
                                                       Diagnostic::LEFT_ENCODER_PARAM));
        addParam(createParamCentered<RoundBlackKnob>(mm2px(Vec(kRightColumn, 50.f)), module,
                                                       Diagnostic::RIGHT_ENCODER_PARAM));

        // Each encoder's push-to-click, directly beneath its knob so the
        // pairing reads visually even though Rack models them as two
        // separate widgets (a knob can't both drag and click in this API).
        addParam(createParamCentered<TL1105>(mm2px(Vec(kLeftColumn, 60.f)), module, Diagnostic::LEFT_BUTTON_PARAM));
        addParam(
            createParamCentered<TL1105>(mm2px(Vec(kRightColumn, 60.f)), module, Diagnostic::RIGHT_BUTTON_PARAM));

        // 3x4 I/O block: TRIG IN, then CV IN, then OUT, matching the
        // silkscreen order on the panel (and on the real hardware) instead
        // of grouping by channel.
        constexpr float kColumnX[kCvChannels] = {12.f, 28.f, 44.f, 60.f};
        constexpr float kTrigRowY = 78.f;
        constexpr float kCvRowY = 98.f;
        constexpr float kOutRowY = 118.f;
        for (int channel = 0; channel < kCvChannels; ++channel) {
            addInput(createInputCentered<PJ301MPort>(mm2px(Vec(kColumnX[channel], kTrigRowY)), module,
                                                       Diagnostic::TR1_INPUT + channel));
            addInput(createInputCentered<PJ301MPort>(mm2px(Vec(kColumnX[channel], kCvRowY)), module,
                                                       Diagnostic::CV1_INPUT + channel));
            addOutput(createOutputCentered<PJ301MPort>(mm2px(Vec(kColumnX[channel], kOutRowY)), module,
                                                         Diagnostic::A_OUTPUT + channel));
        }

        // Screen: centered horizontally on the 71.12mm-wide panel; the
        // panel art draws a 1mm bezel frame around this exact box.
        auto *screen = createWidget<DiagnosticScreen>(mm2px(Vec(12.56f, 9.f)));
        screen->module = module;
        screen->box.size = mm2px(Vec(46.f, 23.f));
        addChild(screen);
    }
};

Model *modelDiagnostic = createModel<Diagnostic, DiagnosticWidget>("Diagnostic");