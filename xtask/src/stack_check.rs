//! A static estimate of the main stack's deepest use, over `llvm-objdump`
//! text (journal/D-038).
//!
//! The firmware has one stack, 16 KiB at the bottom of the DTCM: the main
//! loop and every interrupt share it, and an overflow runs below
//! `0x2000_0000`, where the next write faults. Nothing else sees it coming:
//! the host tests run on a large stack, and a frame grows silently when the
//! optimiser inlines a function with big locals into another (a probe reply
//! of 460 bytes, two copies of the framebuffer). This module reads each
//! function's frame from its prologue, follows the direct calls, and adds the
//! deepest path from `main` to the deepest path of every interrupt handler,
//! as if each one nested over the others.
//!
//! A call through a pointer (`blx rN`, or `bx rN` as a tail call) has no
//! address to follow. The applets are reached that way, through `&dyn App`
//! (journal/D-043), and some of their methods are called deep, from the
//! probe's server or while a slot is filled: so a call through a pointer
//! from a function of this workspace's crates (`oc_…`) is taken as a call to
//! the deepest of every applet's `App` methods, whichever it really reaches.
//! That bounds the applets from above, the same bound at every such call.
//!
//! What it cannot see, and the margin of the check is there for:
//!
//! * the other calls through a pointer, from the crates it depends on (the
//!   logger, the storage's wakers), which it does not follow;
//! * an applet method that itself calls through a pointer, counted with its
//!   direct calls only;
//! * a frame that grows at run time (`sub sp, sp, rN`), which it ignores;
//! * recursion, which it breaks at the first repeat.
//!
//! It counts conservatively elsewhere: a tail call (`b.w` to another
//! function's start) as a call under its caller's frame, every `sub sp` of a
//! function as part of its frame, and an exception's extended frame (with
//! the floating-point registers) for every handler. The panic path is left
//! out: a panic ends in an endless loop, and if it overflows the module is
//! lost either way.
//!
//! # `llvm-objdump` contract
//!
//! ```text
//! llvm-objdump --disassemble --demangle --no-show-raw-insn <elf>
//! llvm-objdump --full-contents --section=.vector_table <elf>
//! ```

use std::collections::{BTreeMap, BTreeSet};

use crate::size_check::{CheckItem, CheckStatus};

/// Below this margin the estimate warns: the calls it does not follow and
/// the next feature's frames need room.
pub const STACK_MARGIN: u64 = 2 * 1024;

/// What an exception pushes before its handler runs, the floating-point
/// registers included (Cortex-M7, `FPCA` set): 26 words.
pub const EXCEPTION_FRAME: u64 = 26 * 4;

/// Handlers that never return, whose stack does not matter: a fault ends
/// the firmware, and the default handler loops.
const NO_RETURN_HANDLERS: [&str; 2] = ["HardFault", "DefaultHandler"];

/// Functions on the panic path, left out of the estimate (see the module's
/// documentation).
const PANIC_PATH: [&str; 2] = ["panic", "rust_begin_unwind"];

/// What names a method of the applets' trait, `<X as oc_core::apps::App>::`,
/// which a call through a pointer may reach (see the module's documentation).
const DYN_TRAIT_METHOD: &str = " as oc_core::apps::App>::";

/// The prefix of this workspace's crates, whose calls through a pointer may
/// be calls to an applet.
const WORKSPACE_CRATE: &str = "oc_";

/// Names the main function may have: `cortex-m-rt`'s `#[entry]`, or a bare
/// `main`.
const MAIN_NAMES: [&str; 2] = ["__cortex_m_rt_main", "main"];

/// One function of the disassembly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    /// Its demangled name.
    pub name: String,
    /// What its prologue takes from the stack, in bytes.
    pub frame: u64,
    /// The addresses it branches to with a link, or tail-calls.
    calls: BTreeSet<u64>,
    /// Whether it calls through a pointer (`blx rN`, `bx rN`).
    indirect: bool,
}

impl Function {
    /// Whether its calls through a pointer may reach an applet: it belongs
    /// to one of this workspace's crates (`oc_core::…`, `<oc_core::…>`,
    /// `<dyn oc_core::…>`).
    fn may_call_an_applet(&self) -> bool {
        let name = self.name.trim_start_matches('<');
        let name = name.strip_prefix("dyn ").unwrap_or(name);
        self.indirect && name.starts_with(WORKSPACE_CRATE)
    }
}

/// Every function by its start address, with its frame and direct calls.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CallGraph {
    functions: BTreeMap<u64, Function>,
}

/// A chain of calls and the stack it takes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StackPath {
    /// The frames added together, in bytes.
    pub bytes: u64,
    /// Each function from the root down, with its own frame.
    pub frames: Vec<(String, u64)>,
}

/// The deepest use of the stack: `main`'s, then every handler's over it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackEstimate {
    /// The deepest path from `main`.
    pub main: StackPath,
    /// The deepest path of each interrupt handler, its exception frame
    /// included.
    pub interrupts: Vec<StackPath>,
    /// The deepest applet method, counted at every call through a pointer
    /// from this workspace's crates; `None` without applets.
    pub applet: Option<StackPath>,
}

impl StackEstimate {
    /// `main`'s deepest path with every handler nested over it.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.main.bytes + self.interrupts.iter().map(|path| path.bytes).sum::<u64>()
    }
}

/// Reads the functions, their frames and their direct calls from
/// `llvm-objdump --disassemble --demangle --no-show-raw-insn` output.
#[must_use]
pub fn parse_disassembly(text: &str) -> CallGraph {
    let mut functions = BTreeMap::new();
    let mut current: Option<(u64, Function)> = None;
    let mut tail_calls: Vec<(u64, u64)> = Vec::new();
    for line in text.lines() {
        if let Some((address, name)) = function_header(line) {
            if let Some((start, function)) = current.take() {
                functions.insert(start, function);
            }
            current = Some((
                address,
                Function {
                    name: name.to_owned(),
                    frame: 0,
                    calls: BTreeSet::new(),
                    indirect: false,
                },
            ));
            continue;
        }
        let Some((start, function)) = current.as_mut() else {
            continue;
        };
        let Some((mnemonic, operands)) = instruction(line) else {
            continue;
        };
        function.frame += frame_bytes(mnemonic, operands);
        if is_indirect_call(mnemonic, operands) {
            function.indirect = true;
        }
        if let Some(target) = branch_target(operands) {
            match mnemonic {
                "bl" | "blx" => {
                    function.calls.insert(target);
                }
                // An unconditional branch out of the function: a tail call,
                // known once every function's start is.
                "b" | "b.w" if target != *start => tail_calls.push((*start, target)),
                _ => {}
            }
        }
    }
    if let Some((start, function)) = current {
        functions.insert(start, function);
    }
    for (caller, target) in tail_calls {
        if functions.contains_key(&target)
            && let Some(function) = functions.get_mut(&caller)
        {
            function.calls.insert(target);
        }
    }
    CallGraph { functions }
}

/// `ADDR <name>:`, the line that opens a function.
fn function_header(line: &str) -> Option<(u64, &str)> {
    let (address, rest) = line.split_once(' ')?;
    let name = rest.strip_prefix('<')?.strip_suffix(">:")?;
    Some((u64::from_str_radix(address, 16).ok()?, name))
}

/// `   1b1c:  sub.w  sp, sp, #0x1020`: the mnemonic and its operands.
fn instruction(line: &str) -> Option<(&str, &str)> {
    let (address, rest) = line.split_once(':')?;
    if address.trim().is_empty() || !address.trim().chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let rest = rest.trim();
    let (mnemonic, operands) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    Some((mnemonic, operands.trim()))
}

/// What one instruction takes from the stack: a push of registers, a
/// pre-indexed store below the stack pointer, or a subtraction from it.
fn frame_bytes(mnemonic: &str, operands: &str) -> u64 {
    match mnemonic {
        "push" | "push.w" => 4 * register_count(operands),
        "stmdb" | "stmdb.w" if operands.starts_with("sp!") => 4 * register_count(operands),
        "vpush" | "vpush.w" => {
            let size = if operands.contains('d') { 8 } else { 4 };
            size * register_count(operands)
        }
        "str" | "str.w" if operands.contains("[sp, #-") && operands.ends_with('!') => {
            immediate(operands.split("#-").nth(1).unwrap_or("")).unwrap_or(0)
        }
        "sub" | "sub.w" | "subw" => sp_subtraction(operands).unwrap_or(0),
        _ => 0,
    }
}

/// How many registers a `{r4, r5, lr}` list names, ranges (`d8-d15`)
/// counted whole.
fn register_count(operands: &str) -> u64 {
    let Some(list) = operands
        .split_once('{')
        .and_then(|(_, rest)| rest.split_once('}'))
        .map(|(list, _)| list)
    else {
        return 0;
    };
    list.split(',')
        .map(|register| {
            let register = register.trim();
            match register.split_once('-') {
                Some((low, high)) => register_number(high)
                    .zip(register_number(low))
                    .map_or(1, |(high, low)| high.saturating_sub(low) + 1),
                None => 1,
            }
        })
        .sum()
}

/// `d15` as 15.
fn register_number(register: &str) -> Option<u64> {
    register
        .trim()
        .trim_start_matches(|c: char| c.is_ascii_alphabetic())
        .parse()
        .ok()
}

/// The bytes of `sp, #0x60` or `sp, sp, #0x1020`, `None` for another
/// destination or a register operand.
fn sp_subtraction(operands: &str) -> Option<u64> {
    let mut parts = operands.split(',').map(str::trim);
    if parts.next()? != "sp" {
        return None;
    }
    let mut last = parts.next()?;
    if last == "sp" {
        last = parts.next()?;
    }
    immediate(last.strip_prefix('#')?)
}

/// `0x1020` or `96`.
fn immediate(text: &str) -> Option<u64> {
    let text = text.trim().trim_end_matches([']', '!']);
    match text.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

/// Whether the instruction calls through a register: `blx r3`, or `bx r3`
/// as a tail call (`bx lr` is a return).
fn is_indirect_call(mnemonic: &str, operands: &str) -> bool {
    let register = operands.split_whitespace().next().unwrap_or("");
    let is_register = register.starts_with('r') && register[1..].parse::<u8>().is_ok()
        || matches!(register, "ip" | "sb" | "sl" | "fp");
    match mnemonic {
        "blx" => is_register || register == "lr",
        "bx" => is_register,
        _ => false,
    }
}

/// The address of `0x13744 <name>` at the start of a branch's operands.
fn branch_target(operands: &str) -> Option<u64> {
    let token = operands.split_whitespace().next()?;
    u64::from_str_radix(token.strip_prefix("0x")?, 16).ok()
}

impl CallGraph {
    /// The function named `name`, or whose name ends with `::name`.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<u64> {
        self.functions
            .iter()
            .find(|(_, function)| {
                function.name == name || function.name.ends_with(&format!("::{name}"))
            })
            .map(|(&address, _)| address)
    }

    /// The function starting at `address`.
    #[must_use]
    pub fn function(&self, address: u64) -> Option<&Function> {
        self.functions.get(&address)
    }

    /// The deepest chain of calls from `root`, the panic path left out,
    /// recursion broken at its first repeat: the direct calls, and a call
    /// through a pointer from this workspace's crates as a call to the
    /// deepest applet method ([`Self::deepest_applet_method`]).
    #[must_use]
    pub fn deepest(&self, root: u64) -> StackPath {
        let applet = self.deepest_applet_method();
        let mut memo = BTreeMap::new();
        self.deepest_from(root, applet.as_ref(), &mut Vec::new(), &mut memo)
    }

    /// The deepest of every applet's `App` methods, by its direct calls,
    /// its first frame named as reached through `dyn App`; `None` in an
    /// image without applets.
    #[must_use]
    pub fn deepest_applet_method(&self) -> Option<StackPath> {
        let mut memo = BTreeMap::new();
        let mut path = self
            .functions
            .iter()
            .filter(|(_, function)| function.name.contains(DYN_TRAIT_METHOD))
            .map(|(&address, _)| self.deepest_from(address, None, &mut Vec::new(), &mut memo))
            .max_by_key(|path| path.bytes)?;
        if let Some((name, _)) = path.frames.first_mut() {
            *name = format!("dyn App: {name}");
        }
        Some(path)
    }

    fn deepest_from(
        &self,
        address: u64,
        applet: Option<&StackPath>,
        on_path: &mut Vec<u64>,
        memo: &mut BTreeMap<u64, StackPath>,
    ) -> StackPath {
        if let Some(path) = memo.get(&address) {
            return path.clone();
        }
        let Some(function) = self.functions.get(&address) else {
            return StackPath::default();
        };
        if on_path.contains(&address) || PANIC_PATH.iter().any(|part| function.name.contains(part))
        {
            return StackPath::default();
        }
        on_path.push(address);
        let through_pointer = applet.filter(|_| function.may_call_an_applet()).cloned();
        let deepest_call = function
            .calls
            .iter()
            .map(|&callee| self.deepest_from(callee, applet, on_path, memo))
            .chain(through_pointer)
            .max_by_key(|path| path.bytes)
            .unwrap_or_default();
        on_path.pop();
        let mut frames = vec![(function.name.clone(), function.frame)];
        frames.extend(deepest_call.frames);
        let path = StackPath {
            bytes: function.frame + deepest_call.bytes,
            frames,
        };
        // Within a recursion the path kept may be cut where the cycle was
        // entered; the firmware has none worth more than a frame.
        memo.insert(address, path.clone());
        path
    }
}

/// The words of the vector table from `llvm-objdump --full-contents
/// --section=.vector_table`: little-endian, the initial stack pointer
/// first, then the reset handler, then every exception and interrupt.
///
/// # Errors
///
/// When no word could be read.
pub fn parse_vector_table(text: &str) -> Result<Vec<u32>, String> {
    let mut words = Vec::new();
    for line in text.lines() {
        // ` 20004000 00400020 31100060 6f290100 7d9c0200  .@. 1..``
        let Some(rest) = line.strip_prefix(' ') else {
            continue;
        };
        let hex = rest.split_once("  ").map_or(rest, |(hex, _)| hex);
        let mut tokens = hex.split_whitespace();
        if tokens
            .next()
            .is_none_or(|address| u64::from_str_radix(address, 16).is_err())
        {
            continue;
        }
        for token in tokens {
            let Ok(bytes) = u32::from_str_radix(token, 16) else {
                break;
            };
            if token.len() != 8 {
                break;
            }
            words.push(bytes.swap_bytes());
        }
    }
    if words.len() < 2 {
        return Err("no vector table in the llvm-objdump output".to_owned());
    }
    Ok(words)
}

/// The deepest path of `main` and of every interrupt handler the vector
/// table names, the handlers that never return left out.
///
/// # Errors
///
/// When the disassembly has no `main`.
pub fn estimate(graph: &CallGraph, vectors: &[u32]) -> Result<StackEstimate, String> {
    let main = MAIN_NAMES
        .iter()
        .find_map(|name| graph.find(name))
        .ok_or("no main function in the disassembly")?;
    let handlers: BTreeSet<u64> = vectors
        .iter()
        .skip(2)
        .filter(|&&word| word != 0)
        // Thumb: the lowest bit only marks the instruction set.
        .map(|&word| u64::from(word & !1))
        .filter(|&address| {
            graph.function(address).is_some_and(|function| {
                !NO_RETURN_HANDLERS
                    .iter()
                    .any(|name| function.name.contains(name))
            })
        })
        .collect();
    let interrupts = handlers
        .into_iter()
        .map(|address| {
            let mut path = graph.deepest(address);
            path.bytes += EXCEPTION_FRAME;
            path.frames
                .insert(0, ("exception frame".to_owned(), EXCEPTION_FRAME));
            path
        })
        .collect();
    Ok(StackEstimate {
        main: graph.deepest(main),
        interrupts,
        applet: graph.deepest_applet_method(),
    })
}

/// The checklist row for `estimate` against a stack of `stack_size` bytes:
/// fails past it, warns under [`STACK_MARGIN`] of room.
#[must_use]
pub fn check_stack(estimate: &StackEstimate, stack_size: u64) -> CheckItem {
    let total = estimate.total();
    let interrupts = total - estimate.main.bytes;
    let margin = stack_size.saturating_sub(total);
    let status = if total > stack_size {
        CheckStatus::Fail
    } else if margin < STACK_MARGIN {
        CheckStatus::Warn
    } else {
        CheckStatus::Pass
    };
    let room = if total > stack_size {
        format!("over by {} B", total - stack_size)
    } else {
        format!("{margin} B free, warn under {STACK_MARGIN}")
    };
    CheckItem {
        id: "stack-depth",
        status,
        detail: format!(
            "≈ {total} B of .stack {stack_size} (main {} + {} handlers {interrupts}, direct calls and dyn App at its deepest method); {room}; journal/D-038",
            estimate.main.bytes,
            estimate.interrupts.len(),
        ),
    }
}

/// `path` a frame a line, deepest last, at most `limit` frames, for the
/// report under the checklist.
#[must_use]
pub fn format_path(title: &str, path: &StackPath, limit: usize) -> String {
    let mut lines = vec![format!("{title}: {} B", path.bytes)];
    for (name, frame) in path
        .frames
        .iter()
        .filter(|(_, frame)| *frame > 0)
        .take(limit)
    {
        let name: String = name.chars().take(100).collect();
        lines.push(format!("  {frame:>6}  {name}"));
    }
    lines.join("\n")
}
