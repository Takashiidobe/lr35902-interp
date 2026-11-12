use crate::instructions::{
    InstructionKind, Operand, ParsedInstruction, cond, opcode_metadata, parse_num, reg16,
};
use crate::{AssemblyError, Inputs};
use std::collections::BTreeMap;

const MAX_INSTRUCTIONS: u64 = 10_000;

#[derive(Debug, Clone)]
pub(crate) struct Interpreter {
    pub(crate) registers: BTreeMap<String, u8>,
    pub(crate) sp: u16,
    pub(crate) flags: BTreeMap<String, bool>,
    pub(crate) memory: BTreeMap<u16, u8>,
    pub(crate) pc: usize,
    pub(crate) cycles: u32,
    pub(crate) steps: u64,
    pub(crate) ime: bool,
    pub(crate) ime_enable_delay: u8,
}

pub(crate) fn execute(
    ins: &[ParsedInstruction],
    source: &str,
    inputs: &Inputs,
) -> Result<(u32, u64), AssemblyError> {
    let mut labels = BTreeMap::<String, usize>::new();
    let mut global = String::new();
    for (line_no, raw) in source.lines().enumerate() {
        let line = raw.split(';').next().unwrap_or("").trim();
        let label = line.split_once(':').map(|x| x.0).or_else(|| {
            if line.starts_with('.') && !line.contains(char::is_whitespace) {
                Some(line)
            } else {
                None
            }
        });
        if let Some(name) = label
            && !name.contains(char::is_whitespace)
        {
            let name = name.trim_end_matches(':').to_ascii_uppercase();
            let key = if name.starts_with('.') {
                format!("{global}{name}")
            } else {
                global = name.clone();
                name
            };
            labels.insert(
                key,
                ins.iter()
                    .position(|x| x.line > line_no)
                    .unwrap_or(ins.len()),
            );
            continue;
        }
    }
    let mut pc_by_address = BTreeMap::<u16, usize>::new();
    let mut code_address = 0u16;
    for (index, item) in ins.iter().enumerate() {
        pc_by_address.insert(code_address, index);
        code_address = code_address.wrapping_add(item.bytes as u16);
    }
    let mut regs = BTreeMap::<String, u8>::new();
    for r in ["A", "B", "C", "D", "E", "H", "L"] {
        regs.insert(r.to_string(), *inputs.registers.get(r).unwrap_or(&0) as u8);
    }
    for (pair, hi, lo) in [("BC", "B", "C"), ("DE", "D", "E"), ("HL", "H", "L")] {
        if let Some(v) = inputs.registers.get(pair) {
            regs.insert(hi.into(), (v >> 8) as u8);
            regs.insert(lo.into(), *v as u8);
        }
    }
    let sp = *inputs.registers.get("SP").unwrap_or(&0);
    let mem = inputs.memory.clone();
    let flags = [
        ("Z".into(), *inputs.flags.get("Z").unwrap_or(&false)),
        ("N".into(), *inputs.flags.get("N").unwrap_or(&false)),
        ("H".into(), *inputs.flags.get("H").unwrap_or(&false)),
        ("C".into(), *inputs.flags.get("C").unwrap_or(&false)),
    ]
    .into();
    let pc = if let Some(entry) = &inputs.entry {
        *labels
            .get(&entry.to_ascii_uppercase())
            .ok_or_else(|| crate::AssemblyError::Label {
                line: 0,
                target: entry.clone(),
            })?
    } else {
        0
    };
    let mut vm = Interpreter {
        registers: regs,
        sp,
        flags,
        memory: mem,
        pc,
        cycles: 0,
        steps: 0,
        ime: false,
        ime_enable_delay: 0,
    };
    let mut global = inputs.entry.as_deref().unwrap_or("").to_ascii_uppercase();
    while vm.pc < ins.len() {
        if ins[vm.pc].cycles.is_none() {
            vm.pc += 1;
            continue;
        }
        if vm.steps >= MAX_INSTRUCTIONS {
            return Err(crate::AssemblyError::ExecutionLimit {
                steps: vm.steps,
                line: ins[vm.pc].line,
            });
        }
        vm.steps += 1;
        let pc = vm.pc;
        let mut regs = std::mem::take(&mut vm.registers);
        let mut sp = vm.sp;
        let mut mem = std::mem::take(&mut vm.memory);
        let mut z = *vm.flags.get("Z").unwrap_or(&false);
        let mut n = *vm.flags.get("N").unwrap_or(&false);
        let mut h = *vm.flags.get("H").unwrap_or(&false);
        let mut c = *vm.flags.get("C").unwrap_or(&false);
        let mut cycles = vm.cycles;
        let item = &ins[pc];
        let kind = item.kind.expect("parsed opcode");
        let m = kind.mnemonic();
        let ops: Vec<String> = item.operands.iter().map(Operand::text).collect();
        let refs: Vec<&str> = ops.iter().map(String::as_str).collect();
        let cpu_inputs = Inputs {
            registers: BTreeMap::new(),
            flags: [
                ("Z".into(), z),
                ("N".into(), n),
                ("H".into(), h),
                ("C".into(), c),
            ]
            .into(),
            memory: BTreeMap::new(),
            entry: None,
        };
        let mut cy = opcode_metadata(m, &refs, &cpu_inputs)
            .map(|x| x.1)
            .unwrap_or(4) as u32;
        let mut next = pc + 1;
        let mut jump = false;
        let mut halted = false;
        let val =
            |name: &str, regs: &BTreeMap<String, u8>, mem: &BTreeMap<u16, u8>, hl: u16| -> u8 {
                if let Some(v) = regs.get(name) {
                    *v
                } else if name == "(HL)" || name == "(HL+)" || name == "(HL-)" {
                    *mem.get(&hl).unwrap_or(&0)
                } else if name == "(BC)" || name == "(DE)" {
                    *mem.get(&pair(name.trim_matches(['(', ')']), regs, 0))
                        .unwrap_or(&0)
                } else if name.starts_with('(') {
                    *mem.get(&parse_num(name.trim_matches(['(', ')'])).unwrap_or(0))
                        .unwrap_or(&0)
                } else {
                    parse_num(name).unwrap_or(0) as u8
                }
            };
        let hl = ((regs["H"] as u16) << 8) | regs["L"] as u16;
        match kind {
            InstructionKind::Load
                if refs.len() == 2
                    && (refs[0].contains("HL+")
                        || refs[0].contains("HL-")
                        || refs[1].contains("HL+")
                        || refs[1].contains("HL-")) =>
            {
                if refs[0].contains("HL") {
                    mem.insert(hl, regs["A"]);
                } else {
                    regs.insert("A".into(), *mem.get(&hl).unwrap_or(&0));
                }
                let post = refs.iter().any(|x| x.contains("HL+"));
                let v = if post {
                    hl.wrapping_add(1)
                } else {
                    hl.wrapping_sub(1)
                };
                regs.insert("H".into(), (v >> 8) as u8);
                regs.insert("L".into(), v as u8);
            }
            InstructionKind::Load
                if refs.len() == 2 && refs[0].starts_with('(') && refs[1] == "SP" =>
            {
                if let Some(a) = address(refs[0], &regs, sp, hl) {
                    mem.insert(a, sp as u8);
                    mem.insert(a.wrapping_add(1), (sp >> 8) as u8);
                }
            }
            InstructionKind::Load if refs.len() == 2 => {
                let v = val(refs[1], &regs, &mem, hl);
                if refs[0].starts_with('(') {
                    if let Some(a) = address(refs[0], &regs, sp, hl) {
                        mem.insert(a, v);
                    }
                } else if refs[0] == "HL" && refs[1].starts_with("SP +") {
                    let offset =
                        parse_num(refs[1].trim_start_matches("SP +").trim()).unwrap_or(0) as u8;
                    h = (sp & 15) + (offset as u16 & 15) > 15;
                    c = (sp & 255) + offset as u16 > 255;
                    z = false;
                    n = false;
                    let x = sp.wrapping_add(offset as i8 as i16 as u16);
                    regs.insert("H".into(), (x >> 8) as u8);
                    regs.insert("L".into(), x as u8);
                } else if refs[0] == "HL" {
                    let x = parse_num(refs[1]).unwrap_or(v as u16);
                    regs.insert("H".into(), (x >> 8) as u8);
                    regs.insert("L".into(), x as u8);
                } else if refs[0] == "BC" || refs[0] == "DE" {
                    let x = parse_num(refs[1]).unwrap_or(v as u16);
                    regs.insert(refs[0].chars().next().unwrap().to_string(), (x >> 8) as u8);
                    regs.insert(refs[0].chars().nth(1).unwrap().to_string(), x as u8);
                } else if refs[0] == "SP" {
                    sp = if refs[1] == "HL" {
                        ((regs["H"] as u16) << 8) | regs["L"] as u16
                    } else {
                        parse_num(refs[1]).unwrap_or(v as u16)
                    };
                } else if let Some(r) = regs.get_mut(refs[0]) {
                    *r = v;
                }
            }
            InstructionKind::Or
            | InstructionKind::And
            | InstructionKind::Xor
            | InstructionKind::Cp
            | InstructionKind::Sub
            | InstructionKind::Sbc
            | InstructionKind::Add
            | InstructionKind::Adc
                if !refs.is_empty() && (refs.len() == 1 || refs[0] == "A") =>
            {
                let src = if refs.len() == 2 { refs[1] } else { refs[0] };
                let lhs = *regs.get("A").unwrap_or(&0);
                let rhs = val(src, &regs, &mem, hl);
                let carry = if m == "ADC" || m == "SBC" { c as u8 } else { 0 };
                let result = match m {
                    "OR" => lhs | rhs,
                    "AND" => lhs & rhs,
                    "XOR" => lhs ^ rhs,
                    "SUB" | "SBC" | "CP" => lhs.wrapping_sub(rhs).wrapping_sub(carry),
                    _ => lhs.wrapping_add(rhs).wrapping_add(carry),
                };
                z = result == 0;
                n = matches!(m, "SUB" | "SBC" | "CP");
                match kind {
                    InstructionKind::And => {
                        h = true;
                        c = false;
                    }
                    InstructionKind::Or | InstructionKind::Xor => {
                        h = false;
                        c = false;
                    }
                    InstructionKind::Sub | InstructionKind::Sbc | InstructionKind::Cp => {
                        h = (lhs & 15) < ((rhs & 15) + carry);
                        c = (lhs as u16) < rhs as u16 + carry as u16;
                    }
                    _ => {
                        h = (lhs & 15) as u16 + (rhs & 15) as u16 + carry as u16 > 15;
                        c = lhs as u16 + rhs as u16 + carry as u16 > 255;
                    }
                }
                if m != "CP" {
                    regs.insert("A".into(), result);
                }
            }
            InstructionKind::Inc | InstructionKind::Dec if !refs.is_empty() && !reg16(refs[0]) => {
                let r = refs[0];
                let old = val(r, &regs, &mem, hl);
                let v = if m == "INC" {
                    old.wrapping_add(1)
                } else {
                    old.wrapping_sub(1)
                };
                z = v == 0;
                n = m == "DEC";
                h = if m == "INC" {
                    old & 15 == 15
                } else {
                    old & 15 == 0
                };
                if r == "(HL)" {
                    mem.insert(hl, v);
                } else {
                    regs.insert(r.into(), v);
                }
            }
            InstructionKind::Bit | InstructionKind::ResetBit | InstructionKind::SetBit
                if refs.len() == 2 =>
            {
                let bit = parse_num(refs[0]).unwrap_or(0).min(7) as u8;
                let old = val(refs[1], &regs, &mem, hl);
                let mask = 1u8 << bit;
                if m == "BIT" {
                    z = old & mask == 0;
                    n = false;
                    h = true;
                } else {
                    let v = if m == "SET" { old | mask } else { old & !mask };
                    if refs[1] == "(HL)" {
                        mem.insert(hl, v);
                    } else {
                        regs.insert(refs[1].into(), v);
                    }
                }
            }
            InstructionKind::Daa => {
                let mut a = regs["A"];
                let mut adjust = 0u8;
                if !n {
                    if c || a > 0x99 {
                        adjust |= 0x60;
                        c = true;
                    }
                    if h || (a & 15) > 9 {
                        adjust |= 6;
                    }
                    a = a.wrapping_add(adjust);
                } else {
                    if c {
                        adjust |= 0x60;
                    }
                    if h {
                        adjust |= 6;
                    }
                    a = a.wrapping_sub(adjust);
                }
                regs.insert("A".into(), a);
                z = a == 0;
                h = false;
            }
            InstructionKind::Complement => {
                regs.insert("A".into(), !regs["A"]);
                n = true;
                h = true;
            }
            InstructionKind::SetCarry => {
                n = false;
                h = false;
                c = true;
            }
            InstructionKind::ComplementCarry => {
                n = false;
                h = false;
                c = !c;
            }
            InstructionKind::AccRotateLeftCarry
            | InstructionKind::AccRotateLeft
            | InstructionKind::AccRotateRightCarry
            | InstructionKind::AccRotateRight => {
                let a = regs["A"];
                let oldc = c;
                let v = match m {
                    "RLCA" => {
                        c = a & 0x80 != 0;
                        a.rotate_left(1)
                    }
                    "RLA" => {
                        c = a & 0x80 != 0;
                        (a << 1) | oldc as u8
                    }
                    "RRCA" => {
                        c = a & 1 != 0;
                        a.rotate_right(1)
                    }
                    _ => {
                        c = a & 1 != 0;
                        (a >> 1) | ((oldc as u8) << 7)
                    }
                };
                regs.insert("A".into(), v);
                z = false;
                n = false;
                h = false;
            }
            InstructionKind::LoadHigh if refs.len() == 2 => {
                let addr = if refs[0].contains("C") || refs[1].contains("C") {
                    0xff00u16.wrapping_add(regs["C"] as u16)
                } else {
                    0xff00u16.wrapping_add(
                        parse_num(refs.iter().find(|x| x.starts_with('(')).unwrap_or(&""))
                            .unwrap_or(0),
                    )
                };
                if refs[0] == "A" {
                    regs.insert("A".into(), *mem.get(&addr).unwrap_or(&0));
                } else {
                    mem.insert(addr, regs["A"]);
                }
            }
            InstructionKind::Load
                if refs.len() == 2
                    && (refs[0] == "(HL+)"
                        || refs[0] == "(HL-)"
                        || refs[1] == "(HL+)"
                        || refs[1] == "(HL-)") =>
            {
                let post = refs[0].contains("HL") && refs[0].contains("+")
                    || refs[1].contains("HL") && refs[1].contains("+");
                let pre = refs[0].contains("HL") && refs[0].contains("-")
                    || refs[1].contains("HL") && refs[1].contains("-");
                let addr = hl;
                let v = if refs[0].contains("HL") {
                    mem.insert(addr, regs["A"]);
                    regs["A"]
                } else {
                    let x = *mem.get(&addr).unwrap_or(&0);
                    regs.insert("A".into(), x);
                    x
                };
                let _ = v;
                let x = if post {
                    hl.wrapping_add(1)
                } else if pre {
                    hl.wrapping_sub(1)
                } else {
                    hl
                };
                regs.insert("H".into(), (x >> 8) as u8);
                regs.insert("L".into(), x as u8);
            }
            InstructionKind::Add if refs.len() == 2 && refs[0] == "HL" => {
                let lhs = hl;
                let rhs = pair(refs[1], &regs, sp);
                let sum = lhs as u32 + rhs as u32;
                h = (lhs & 0xfff) + (rhs & 0xfff) > 0xfff;
                c = sum > 0xffff;
                n = false;
                let x = sum as u16;
                regs.insert("H".into(), (x >> 8) as u8);
                regs.insert("L".into(), x as u8);
            }
            InstructionKind::Add if refs.len() == 2 && refs[0] == "SP" => {
                let e = parse_num(refs[1]).unwrap_or(0) as u8 as i8 as i16 as u16;
                let old = sp;
                let sum = old.wrapping_add(e);
                z = false;
                n = false;
                h = (old & 15) + (e & 15) > 15;
                c = (old & 255) + (e & 255) > 255;
                sp = sum;
            }
            InstructionKind::Inc | InstructionKind::Dec if refs.len() == 1 && reg16(refs[0]) => {
                let x =
                    pair(refs[0], &regs, sp).wrapping_add(if m == "INC" { 1 } else { u16::MAX });
                set_pair(refs[0], x, &mut regs, &mut sp);
            }
            InstructionKind::ShiftLeft
            | InstructionKind::ShiftRightArithmetic
            | InstructionKind::ShiftRightLogical
            | InstructionKind::Swap
            | InstructionKind::RotateCircularLeft
            | InstructionKind::RotateCircularRight
            | InstructionKind::RotateLeft
            | InstructionKind::RotateRight
                if !refs.is_empty() =>
            {
                let r = refs[0];
                let v = val(r, &regs, &mem, hl);
                let oldc = c;
                let out = match m {
                    "SLA" => {
                        c = v & 128 != 0;
                        v << 1
                    }
                    "SRA" => {
                        c = v & 1 != 0;
                        (v >> 1) | (v & 128)
                    }
                    "SRL" => {
                        c = v & 1 != 0;
                        v >> 1
                    }
                    "SWAP" => {
                        c = false;
                        v.rotate_left(4)
                    }
                    "RLC" => {
                        c = v & 128 != 0;
                        v.rotate_left(1)
                    }
                    "RRC" => {
                        c = v & 1 != 0;
                        v.rotate_right(1)
                    }
                    "RL" => {
                        c = v & 128 != 0;
                        (v << 1) | oldc as u8
                    }
                    _ => {
                        c = v & 1 != 0;
                        (v >> 1) | ((oldc as u8) << 7)
                    }
                };
                z = out == 0;
                n = false;
                h = false;
                if r == "(HL)" {
                    mem.insert(hl, out);
                } else {
                    regs.insert(r.into(), out);
                }
            }
            InstructionKind::Push if !refs.is_empty() => {
                let v = if refs[0] == "AF" {
                    ((regs["A"] as u16) << 8)
                        | ((z as u16) << 7)
                        | ((n as u16) << 6)
                        | ((h as u16) << 5)
                        | ((c as u16) << 4)
                } else {
                    pair(refs[0], &regs, sp)
                };
                push_word(&mut mem, &mut sp, v);
            }
            InstructionKind::Pop if !refs.is_empty() => {
                let v = pop_word(&mem, &mut sp);
                if refs[0] == "AF" {
                    regs.insert("A".into(), (v >> 8) as u8);
                    z = v & 0x80 != 0;
                    n = v & 0x40 != 0;
                    h = v & 0x20 != 0;
                    c = v & 0x10 != 0;
                } else {
                    set_pair(refs[0], v, &mut regs, &mut sp);
                }
            }
            InstructionKind::Jump if refs.len() == 1 && refs[0] == "HL" => {
                if let Some(dest) = pc_by_address.get(&hl) {
                    next = *dest;
                    jump = true;
                } else {
                    halted = true;
                }
            }
            InstructionKind::RelativeJump | InstructionKind::Jump | InstructionKind::Call
                if !refs.is_empty() =>
            {
                let (target, taken) = if refs.len() == 2 {
                    (refs[1], cond(refs[0], &cpu_inputs).unwrap_or(false))
                } else {
                    (refs[0], true)
                };
                cy = if m == "JR" {
                    if refs.len() == 2 {
                        if taken { 12 } else { 8 }
                    } else {
                        12
                    }
                } else if m == "CALL" {
                    if refs.len() == 2 {
                        if taken { 24 } else { 12 }
                    } else {
                        24
                    }
                } else if refs.len() == 2 {
                    if taken { 16 } else { 12 }
                } else {
                    16
                };
                if taken {
                    let key = if target.starts_with('.') {
                        format!("{global}{target}")
                    } else {
                        target.to_string()
                    };
                    let relative_dest = if kind == InstructionKind::RelativeJump {
                        parse_num(target).and_then(|offset| {
                            let address = ins[..pc]
                                .iter()
                                .fold(0u16, |sum, instruction| {
                                    sum.wrapping_add(instruction.bytes as u16)
                                })
                                .wrapping_add(item.bytes as u16)
                                .wrapping_add(offset as u8 as i8 as i16 as u16);
                            pc_by_address.get(&address).copied()
                        })
                    } else {
                        None
                    };
                    let dest = relative_dest.or_else(|| {
                        labels
                            .get(&key)
                            .or_else(|| {
                                labels
                                    .iter()
                                    .find(|(k, _)| k.ends_with(target))
                                    .map(|(_, v)| v)
                            })
                            .copied()
                            .or_else(|| {
                                parse_num(target)
                                    .and_then(|address| pc_by_address.get(&address).copied())
                            })
                    });
                    if let Some(dest) = dest {
                        if m == "CALL" {
                            let return_address =
                                ins[..=pc].iter().fold(0u16, |sum, instruction| {
                                    sum.wrapping_add(instruction.bytes as u16)
                                });
                            push_word(&mut mem, &mut sp, return_address);
                        }
                        next = dest;
                        jump = true;
                    } else if parse_num(target).is_some() {
                        halted = true;
                    } else {
                        return Err(crate::AssemblyError::Label {
                            line: item.line,
                            target: key,
                        });
                    }
                }
            }
            InstructionKind::Return | InstructionKind::ReturnInterrupt => {
                let taken = refs.is_empty() || cond(refs[0], &cpu_inputs).unwrap_or(false);
                if kind == InstructionKind::ReturnInterrupt {
                    vm.ime = true;
                    vm.ime_enable_delay = 0;
                }
                if taken {
                    let has_return_address =
                        mem.contains_key(&sp) && mem.contains_key(&sp.wrapping_add(1));
                    if has_return_address {
                        let return_address = pop_word(&mem, &mut sp);
                        if let Some(dest) = pc_by_address.get(&return_address) {
                            next = *dest;
                            jump = true;
                        } else {
                            halted = true;
                        }
                    } else {
                        halted = true;
                    }
                }
            }
            InstructionKind::Restart => {
                let vector = parse_num(refs.first().copied().unwrap_or("$00")).unwrap_or(0);
                if let Some(dest) = pc_by_address.get(&vector) {
                    let return_address = ins[..=pc].iter().fold(0u16, |sum, instruction| {
                        sum.wrapping_add(instruction.bytes as u16)
                    });
                    push_word(&mut mem, &mut sp, return_address);
                    next = *dest;
                    jump = true;
                } else {
                    halted = true;
                }
            }
            InstructionKind::Nop => {}
            InstructionKind::DisableInterrupts => {
                vm.ime = false;
                vm.ime_enable_delay = 0;
            }
            InstructionKind::EnableInterrupts => vm.ime_enable_delay = 2,
            InstructionKind::Stop => halted = true,
            InstructionKind::Halt => {
                halted = interrupt_pending(&mem).is_none() || vm.ime;
            }
            _ => {
                return Err(crate::AssemblyError::Unknown {
                    line: item.line,
                    text: m.into(),
                });
            }
        }
        if !jump
            && let Some((name, _)) = source
                .lines()
                .nth(item.line - 1)
                .and_then(|l| l.trim().split_once(':'))
            && !name.starts_with('.')
        {
            global = name.to_ascii_uppercase();
        }
        cycles += cy;
        vm.registers = regs;
        vm.sp = sp;
        vm.memory = mem;
        vm.flags.insert("Z".into(), z);
        vm.flags.insert("N".into(), n);
        vm.flags.insert("H".into(), h);
        vm.flags.insert("C".into(), c);
        vm.cycles = cycles;
        vm.pc = next;
        if vm.ime_enable_delay > 0 {
            vm.ime_enable_delay -= 1;
            if vm.ime_enable_delay == 0 {
                vm.ime = true;
            }
        }
        if vm.ime
            && let Some((mask, vector)) = interrupt_pending(&vm.memory)
            && let Some(dest) = pc_by_address.get(&vector)
        {
            let return_address = instruction_address(ins, vm.pc);
            push_word(&mut vm.memory, &mut vm.sp, return_address);
            vm.pc = *dest;
            vm.ime = false;
            vm.ime_enable_delay = 0;
            vm.cycles = vm.cycles.wrapping_add(20);
            let pending = vm.memory.get(&0xff0f).copied().unwrap_or(0);
            vm.memory.insert(0xff0f, pending & !mask);
            halted = false;
        }
        if halted {
            break;
        }
    }
    Ok((vm.cycles, vm.steps))
}
fn pair(name: &str, r: &BTreeMap<String, u8>, sp: u16) -> u16 {
    match name {
        "BC" => (r["B"] as u16) << 8 | r["C"] as u16,
        "DE" => (r["D"] as u16) << 8 | r["E"] as u16,
        "HL" => (r["H"] as u16) << 8 | r["L"] as u16,
        "SP" => sp,
        "AF" => (r["A"] as u16) << 8,
        _ => 0,
    }
}
fn push_word(mem: &mut BTreeMap<u16, u8>, sp: &mut u16, value: u16) {
    *sp = sp.wrapping_sub(1);
    mem.insert(*sp, (value >> 8) as u8);
    *sp = sp.wrapping_sub(1);
    mem.insert(*sp, value as u8);
}
fn pop_word(mem: &BTreeMap<u16, u8>, sp: &mut u16) -> u16 {
    let lo = *mem.get(sp).unwrap_or(&0) as u16;
    *sp = sp.wrapping_add(1);
    let hi = *mem.get(sp).unwrap_or(&0) as u16;
    *sp = sp.wrapping_add(1);
    lo | (hi << 8)
}
fn instruction_address(ins: &[ParsedInstruction], pc: usize) -> u16 {
    ins.iter().take(pc).fold(0u16, |sum, instruction| {
        sum.wrapping_add(instruction.bytes as u16)
    })
}
fn interrupt_pending(mem: &BTreeMap<u16, u8>) -> Option<(u8, u16)> {
    let enabled = mem.get(&0xffff).copied().unwrap_or(0);
    let requested = mem.get(&0xff0f).copied().unwrap_or(0);
    match enabled & requested & 0x1f {
        pending if pending & 0x01 != 0 => Some((0x01, 0x0040)),
        pending if pending & 0x02 != 0 => Some((0x02, 0x0048)),
        pending if pending & 0x04 != 0 => Some((0x04, 0x0050)),
        pending if pending & 0x08 != 0 => Some((0x08, 0x0058)),
        pending if pending & 0x10 != 0 => Some((0x10, 0x0060)),
        _ => None,
    }
}
fn address(name: &str, r: &BTreeMap<String, u8>, sp: u16, hl: u16) -> Option<u16> {
    let x = name.trim_matches(['(', ')']);
    match x {
        "HL" | "HL+" | "HL-" => Some(hl),
        "BC" | "DE" => Some(pair(x, r, sp)),
        _ => parse_num(x),
    }
}
fn set_pair(name: &str, v: u16, r: &mut BTreeMap<String, u8>, sp: &mut u16) {
    match name {
        "BC" => {
            r.insert("B".into(), (v >> 8) as u8);
            r.insert("C".into(), v as u8);
        }
        "DE" => {
            r.insert("D".into(), (v >> 8) as u8);
            r.insert("E".into(), v as u8);
        }
        "HL" => {
            r.insert("H".into(), (v >> 8) as u8);
            r.insert("L".into(), v as u8);
        }
        "SP" => *sp = v,
        _ => {}
    }
}
