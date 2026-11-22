//! The stack estimate over captured `llvm-objdump` text (journal/D-038):
//! frames read from prologues, direct and tail calls followed, the panic
//! path and the handlers that never return left out.

use xtask::size_check::CheckStatus;
use xtask::stack_check::{
    EXCEPTION_FRAME, STACK_MARGIN, check_stack, estimate, parse_disassembly, parse_vector_table,
};

/// `main` calls `serve`, which calls `send` and tail-calls `screen`; the
/// interrupt calls `apply`; `serve` may panic, whose path is deeper still.
const DISASSEMBLY: &str = "\
target/oc-firmware:\tfile format elf32-littlearm

Disassembly of section .text:

00000000 <__stext>:
       0:      \tpush\t{r4, r5, r7, lr}
       2:      \tsub\tsp, #0x10
       4:      \tbl\t0x400 <apply>
       8:      \tpop\t{r4, r5, r7, pc}

00000100 <oc_firmware::__cortex_m_rt_main>:
     100:      \tpush.w\t{r4, r5, r6, r7, r8, r9, r10, r11, lr}
     104:      \tvpush\t{d8-d15}
     108:      \tsub.w\tsp, sp, #0x1000
     10c:      \tbl\t0x200 <serve>
     110:      \tb\t0x10c <oc_firmware::__cortex_m_rt_main+0xc>

00000200 <serve>:
     200:      \tstr\tlr, [sp, #-0x4]!
     204:      \tsubw\tsp, sp, #0x200
     208:      \tbl\t0x300 <send>
     20c:      \tbl\t0x900 <core::panicking::panic>
     210:      \tbne\t0x500 <screen>
     214:      \tb.w\t0x500 <screen>

00000300 <send>:
     300:      \tpush\t{r7, lr}
     302:      \tsub\tsp, #0x40
     304:      \tblx\tr3
     306:      \tpop\t{r7, pc}

00000400 <apply>:
     400:      \tpush\t{r4, lr}
     402:      \tpop\t{r4, pc}

00000500 <screen>:
     500:      \tpush\t{r4, r5, r6, lr}
     502:      \tsub\tsp, #0x400
     504:      \tbl\t0x200 <serve>
     508:      \tpop\t{r4, r5, r6, pc}

00000800 <HardFault_>:
     800:      \tsub\tsp, #0x800
     802:      \tb\t0x802 <HardFault_+0x2>

00000900 <core::panicking::panic>:
     900:      \tsub.w\tsp, sp, #0x4000
     904:      \tb\t0x904 <core::panicking::panic+0x4>
";

/// The initial stack pointer, reset, `HardFault`, then the interrupt at
/// 0x0 twice (thumb bit set) and an empty slot.
const VECTOR_TABLE: &str = "\
target/oc-firmware:\tfile format elf32-littlearm
Contents of section .vector_table:
 20004000 00400020 31100060 01080000 01000000  .@. 1..`........
 20004010 01000000 00000000                    ........
";

#[test]
fn frames_are_read_from_pushes_and_subtractions() {
    let graph = parse_disassembly(DISASSEMBLY);
    let frame = |name| graph.function(graph.find(name).unwrap()).unwrap().frame;
    assert_eq!(frame("__cortex_m_rt_main"), 9 * 4 + 8 * 8 + 0x1000);
    assert_eq!(frame("serve"), 4 + 0x200, "a pre-indexed store and subw");
    assert_eq!(frame("send"), 2 * 4 + 0x40);
    assert_eq!(frame("__stext"), 4 * 4 + 0x10);
}

#[test]
fn the_deepest_path_follows_tail_calls_and_skips_the_panic_and_recursion() {
    let graph = parse_disassembly(DISASSEMBLY);
    let path = graph.deepest(graph.find("__cortex_m_rt_main").unwrap());
    let names: Vec<&str> = path.frames.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        ["oc_firmware::__cortex_m_rt_main", "serve", "screen"],
        "screen, tail-called, is deeper than send; serve again is a cycle"
    );
    assert_eq!(
        path.bytes,
        (9 * 4 + 8 * 8 + 0x1000) + (4 + 0x200) + (4 * 4 + 0x400)
    );
}

#[test]
fn interrupts_come_from_the_vector_table_and_add_their_exception_frame() {
    let graph = parse_disassembly(DISASSEMBLY);
    let vectors = parse_vector_table(VECTOR_TABLE).unwrap();
    assert_eq!(vectors.len(), 6);
    assert_eq!(vectors[0], 0x2000_4000, "words are little-endian");
    let estimate = estimate(&graph, &vectors).unwrap();
    assert_eq!(
        estimate.interrupts.len(),
        1,
        "HardFault left out, the handler counted once"
    );
    assert_eq!(
        estimate.interrupts[0].bytes,
        EXCEPTION_FRAME + (4 * 4 + 0x10) + 2 * 4
    );
    assert_eq!(
        estimate.total(),
        estimate.main.bytes + estimate.interrupts[0].bytes
    );
}

#[test]
fn the_check_fails_past_the_stack_and_warns_short_of_the_margin() {
    let graph = parse_disassembly(DISASSEMBLY);
    let vectors = parse_vector_table(VECTOR_TABLE).unwrap();
    let estimate = estimate(&graph, &vectors).unwrap();
    let total = estimate.total();
    assert_eq!(check_stack(&estimate, total - 1).status, CheckStatus::Fail);
    assert_eq!(check_stack(&estimate, total).status, CheckStatus::Warn);
    assert_eq!(
        check_stack(&estimate, total + STACK_MARGIN).status,
        CheckStatus::Pass
    );
    let detail = check_stack(&estimate, total - 1).detail;
    assert!(detail.contains("over by 1 B"), "{detail}");
}

#[test]
fn a_disassembly_without_main_or_a_table_is_refused() {
    let graph = parse_disassembly("00000000 <alone>:\n       0:      \tbx\tlr\n");
    assert!(estimate(&graph, &[0x2000_4000, 0x6000_1031]).is_err());
    assert!(parse_vector_table("Contents of section .vector_table:\n").is_err());
}
