//! The applets' share of the code over `llvm-nm` text (`xtask::applet_size`),
//! with no firmware build: synthetic symbol tables in the tool's layout.

use xtask::applet_size::{Share, attribute, format_breakdown, parse_nm};

const NM: &str = "\
00001000 00000400 t <oc_core::apps::melody::MelodyApp as oc_core::apps::App>::render
00001400 00000100 t oc_core::apps::melody::sequence::Sequence::step
00001500 00000080 t <oc_core::apps::melody::sequence::Steps as serde_core::de::Deserialize>::deserialize::<&mut postcard::de::deserializer::Deserializer<postcard::de::flavors::Slice>>
00001580 00000200 t <oc_core::apps::supersaw::SupersawApp as oc_core::apps::App>::update
00001780 00000300 T oc_dsp::supersaw::oscillator::Bank::prepare
00001a80 00000040 t oc_dsp::phase_step
00001ac0 00000500 t <oc_core::apps::AppHost>::update
00001fc0 00000090 t oc_core::apps::param::ParamSpec::check
000020c0 00000050 t <oc_core::apps::sound::Sound as oc_core::audio::Generator>::apply
00002110 00000600 t oc_firmware::run
00002710 00000200 t oc_protocol::server::Server::receive
00002910 00000300 T DMA0_DMA16
00002c10 00000020 T __aeabi_memcpy
20000000 00000040 d some_static
00002c30 00000010 t <oc_core::apps::gfx_demo::GfxDemoApp as oc_core::apps::App>::name
";

#[test]
fn functions_are_read_and_data_left_out() {
    let symbols = parse_nm(NM);
    assert_eq!(symbols.len(), 14, "the static is not code");
    assert_eq!(symbols[0].size, 400);
    assert!(symbols[2].name.contains("Deserialize>::deserialize"));
}

#[test]
fn each_applet_takes_its_module_and_its_dsp() {
    let breakdown = attribute(&parse_nm(NM));
    let share = |name: &str| breakdown.applets.get(name).copied().unwrap_or_default();
    assert_eq!(
        share("melody"),
        Share {
            bytes: 400 + 100 + 80,
            functions: 3
        },
        "its methods, its own modules, a generic over its type"
    );
    assert_eq!(
        share("supersaw").bytes,
        200 + 300,
        "its app and oc_dsp::supersaw"
    );
    assert_eq!(share("gfx_demo").bytes, 10);
    assert!(!breakdown.applets.contains_key("param"), "no applet");
    assert!(!breakdown.applets.contains_key("sound"));
}

#[test]
fn the_rest_is_shared_by_where_it_comes_from() {
    let breakdown = attribute(&parse_nm(NM));
    let shared = |name: &str| breakdown.shared.get(name).map_or(0, |share| share.bytes);
    assert_eq!(shared("oc_core, shared"), 500 + 90 + 50);
    assert_eq!(shared("oc_dsp, shared"), 40, "phase_step is no applet's");
    assert_eq!(shared("firmware, board, drivers"), 600);
    assert_eq!(shared("probe protocol"), 200);
    assert_eq!(shared("Rust core, USB, interrupts, the rest"), 300 + 20);
    let total: u64 = parse_nm(NM).iter().map(|symbol| symbol.size).sum();
    assert_eq!(breakdown.total(), total, "every function counted once");
}

#[test]
fn the_table_lists_the_applets_largest_first() {
    let text = format_breakdown(&attribute(&parse_nm(NM)));
    let melody = text.find("MELODY").unwrap();
    let supersaw = text.find("SUPERSAW").unwrap();
    let gfx = text.find("GFX DEMO").unwrap();
    assert!(melody < supersaw && supersaw < gfx, "{text}");
    assert!(text.contains("(firmware, board, drivers)"));
}
