use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Operand {
    Register8(String),
    Register16(String),
    Indirect(String),
    Immediate(String),
    Condition(String),
    Symbol(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstructionKind {
    Nop,
    Load,
    LoadHigh,
    Inc,
    Dec,
    Add,
    Adc,
    Sub,
    Sbc,
    And,
    Or,
    Xor,
    Cp,
    Jump,
    RelativeJump,
    Call,
    Return,
    ReturnInterrupt,
    Restart,
    Push,
    Pop,
    Bit,
    ResetBit,
    SetBit,
    RotateLeft,
    RotateRight,
    RotateCircularLeft,
    RotateCircularRight,
    AccRotateLeft,
    AccRotateLeftCarry,
    AccRotateRight,
    AccRotateRightCarry,
    ShiftLeft,
    ShiftRightArithmetic,
    ShiftRightLogical,
    Swap,
    Daa,
    Complement,
    SetCarry,
    ComplementCarry,
    Halt,
    Stop,
    DisableInterrupts,
    EnableInterrupts,
}

#[derive(Debug, Clone)]
pub(crate) struct ParsedInstruction {
    pub(crate) kind: Option<InstructionKind>,
    pub(crate) operands: Vec<Operand>,
    pub(crate) line: usize,
    pub(crate) bytes: u8,
    pub(crate) cycles: Option<u16>,
}

#[derive(Debug, Clone)]
struct OpcodeSpec {
    operands: Vec<(String, bool)>,
    bytes: u8,
    cycles: Vec<u16>,
}

impl InstructionKind {
    pub(crate) fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "NOP" => Self::Nop,
            "LD" => Self::Load,
            "LDH" => Self::LoadHigh,
            "INC" => Self::Inc,
            "DEC" => Self::Dec,
            "ADD" => Self::Add,
            "ADC" => Self::Adc,
            "SUB" => Self::Sub,
            "SBC" => Self::Sbc,
            "AND" => Self::And,
            "OR" => Self::Or,
            "XOR" => Self::Xor,
            "CP" => Self::Cp,
            "JP" => Self::Jump,
            "JR" => Self::RelativeJump,
            "CALL" => Self::Call,
            "RET" => Self::Return,
            "RETI" => Self::ReturnInterrupt,
            "RST" => Self::Restart,
            "PUSH" => Self::Push,
            "POP" => Self::Pop,
            "BIT" => Self::Bit,
            "RES" => Self::ResetBit,
            "SET" => Self::SetBit,
            "RL" => Self::RotateLeft,
            "RR" => Self::RotateRight,
            "RLC" => Self::RotateCircularLeft,
            "RRC" => Self::RotateCircularRight,
            "RLCA" => Self::AccRotateLeftCarry,
            "RLA" => Self::AccRotateLeft,
            "RRCA" => Self::AccRotateRightCarry,
            "RRA" => Self::AccRotateRight,
            "SLA" => Self::ShiftLeft,
            "SRA" => Self::ShiftRightArithmetic,
            "SRL" => Self::ShiftRightLogical,
            "SWAP" => Self::Swap,
            "DAA" => Self::Daa,
            "CPL" => Self::Complement,
            "SCF" => Self::SetCarry,
            "CCF" => Self::ComplementCarry,
            "HALT" => Self::Halt,
            "STOP" => Self::Stop,
            "DI" => Self::DisableInterrupts,
            "EI" => Self::EnableInterrupts,
            _ => return None,
        })
    }
    pub(crate) fn mnemonic(self) -> &'static str {
        match self {
            Self::Nop => "NOP",
            Self::Load => "LD",
            Self::LoadHigh => "LDH",
            Self::Inc => "INC",
            Self::Dec => "DEC",
            Self::Add => "ADD",
            Self::Adc => "ADC",
            Self::Sub => "SUB",
            Self::Sbc => "SBC",
            Self::And => "AND",
            Self::Or => "OR",
            Self::Xor => "XOR",
            Self::Cp => "CP",
            Self::Jump => "JP",
            Self::RelativeJump => "JR",
            Self::Call => "CALL",
            Self::Return => "RET",
            Self::ReturnInterrupt => "RETI",
            Self::Restart => "RST",
            Self::Push => "PUSH",
            Self::Pop => "POP",
            Self::Bit => "BIT",
            Self::ResetBit => "RES",
            Self::SetBit => "SET",
            Self::RotateLeft => "RL",
            Self::RotateRight => "RR",
            Self::RotateCircularLeft => "RLC",
            Self::RotateCircularRight => "RRC",
            Self::AccRotateLeft => "RLA",
            Self::AccRotateLeftCarry => "RLCA",
            Self::AccRotateRight => "RRA",
            Self::AccRotateRightCarry => "RRCA",
            Self::ShiftLeft => "SLA",
            Self::ShiftRightArithmetic => "SRA",
            Self::ShiftRightLogical => "SRL",
            Self::Swap => "SWAP",
            Self::Daa => "DAA",
            Self::Complement => "CPL",
            Self::SetCarry => "SCF",
            Self::ComplementCarry => "CCF",
            Self::Halt => "HALT",
            Self::Stop => "STOP",
            Self::DisableInterrupts => "DI",
            Self::EnableInterrupts => "EI",
        }
    }
}
impl Operand {
    pub(crate) fn parse(text: &str) -> Self {
        let s = text.trim();
        if s.starts_with('(') && s.ends_with(')') {
            return Self::Indirect(s[1..s.len() - 1].to_string());
        }
        if matches!(s, "NZ" | "Z" | "NC" | "C") {
            return Self::Condition(s.into());
        }
        if matches!(s, "A" | "B" | "C" | "D" | "E" | "H" | "L") {
            return Self::Register8(s.into());
        }
        if matches!(s, "AF" | "BC" | "DE" | "HL" | "SP") {
            return Self::Register16(s.into());
        }
        if s.starts_with('$') || s.starts_with("0X") || s.parse::<i32>().is_ok() {
            Self::Immediate(s.into())
        } else {
            Self::Symbol(s.into())
        }
    }
    pub(crate) fn text(&self) -> String {
        match self {
            Self::Register8(s)
            | Self::Register16(s)
            | Self::Condition(s)
            | Self::Immediate(s)
            | Self::Symbol(s) => s.clone(),
            Self::Indirect(s) => format!("({s})"),
        }
    }
}
pub(crate) fn reg16(s: &str) -> bool {
    matches!(s, "BC" | "DE" | "HL" | "SP")
}
pub(crate) fn reg8(s: &str) -> bool {
    matches!(s, "A" | "B" | "C" | "D" | "E" | "H" | "L" | "(HL)")
}
pub(crate) fn cond(s: &str, i: &crate::Inputs) -> Option<bool> {
    let f = |n: &str| *i.flags.get(n).unwrap_or(&false);
    match s {
        "NZ" => Some(!f("Z")),
        "Z" => Some(f("Z")),
        "NC" => Some(!f("C")),
        "C" => Some(f("C")),
        _ => None,
    }
}
pub(crate) fn opcode_metadata(m: &str, ops: &[&str], inputs: &crate::Inputs) -> Option<(u8, u16)> {
    if ops.len() == 1 && ["ADD", "ADC", "SUB", "SBC", "AND", "OR", "XOR", "CP"].contains(&m) {
        return opcode_metadata(m, &["A", ops[0]], inputs);
    }
    if ["BIT", "SET", "RES"].contains(&m)
        && ops.first().and_then(|x| parse_num(x)).is_none_or(|x| x > 7)
    {
        return None;
    }
    if m == "RST"
        && ops
            .first()
            .and_then(|x| parse_num(x))
            .is_none_or(|x| x > 0x38 || x % 8 != 0)
    {
        return None;
    }
    static SPECS: OnceLock<BTreeMap<String, Vec<OpcodeSpec>>> = OnceLock::new();
    let specs = SPECS.get_or_init(|| {
        let root: serde_json::Value =
            serde_json::from_str(include_str!("../data/Opcodes.json")).expect("valid opcode table");
        let mut map = BTreeMap::<String, Vec<OpcodeSpec>>::new();
        for table in ["unprefixed", "cbprefixed"] {
            for entry in root[table].as_object().unwrap().values() {
                let mnemonic = entry["mnemonic"].as_str().unwrap().to_string();
                let operands = entry["operands"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|o| {
                        (
                            o["name"].as_str().unwrap().to_string(),
                            o["immediate"].as_bool().unwrap_or(false),
                        )
                    })
                    .collect();
                let cycles = entry["cycles"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|n| n.as_u64().unwrap() as u16)
                    .collect();
                map.entry(mnemonic).or_default().push(OpcodeSpec {
                    operands,
                    bytes: entry["bytes"].as_u64().unwrap() as u8,
                    cycles,
                });
            }
        }
        map
    });
    if m == "STOP" && ops.is_empty() {
        return specs
            .get(m)?
            .iter()
            .find(|s| s.operands.len() == 1 && s.operands[0].0 == "n8")
            .map(|s| (s.bytes, s.cycles[0]));
    }
    if m == "LD" && ops.len() == 2 && ops[0] == "HL" && ops[1].starts_with("SP +") {
        return specs
            .get(m)?
            .iter()
            .find(|s| {
                s.operands.len() == 3
                    && s.operands[0].0 == "HL"
                    && s.operands[1].0 == "SP"
                    && s.operands[2].0 == "e8"
            })
            .map(|s| (s.bytes, s.cycles[0]));
    }
    for spec in specs.get(m)? {
        if spec.operands.len() != ops.len() {
            continue;
        }
        let mut matches = true;
        for ((name, immediate), actual) in spec.operands.iter().zip(ops) {
            let actual_raw = actual.trim();
            let is_memory = actual_raw.starts_with('(');
            let actual = actual_raw.trim_matches(['(', ')']);
            let wildcard = name.starts_with('n')
                || name.starts_with('a')
                || name.starts_with('e')
                || name.contains("r8");
            let signed_sp = name.starts_with("SP+") && actual.starts_with("SP +");
            if name == "HL" && is_memory == *immediate {
                matches = false;
                break;
            }
            let immediate_match = wildcard
                && *immediate
                && !is_memory
                && !reg8(actual)
                && !reg16(actual)
                && !matches!(actual, "A" | "NZ" | "Z" | "NC" | "C");
            let address_match = name.starts_with('a')
                && !*immediate
                && is_memory
                && !reg8(actual)
                && !reg16(actual);
            let bit_match =
                name.parse::<u8>().is_ok() && parse_num(actual).is_some_and(|bit| bit <= 7);
            let restart_match = m == "RST"
                && name.starts_with('$')
                && parse_num(actual).is_some_and(|v| v <= 0x38 && v % 8 == 0);
            if !immediate_match
                && !address_match
                && !bit_match
                && !restart_match
                && !signed_sp
                && !name.eq_ignore_ascii_case(actual)
            {
                matches = false;
                break;
            }
        }
        if !matches {
            continue;
        }
        let cycles = if spec.cycles.len() == 1 {
            spec.cycles[0]
        } else {
            let taken = ops.first().and_then(|x| cond(x, inputs)).unwrap_or(false);
            spec.cycles[if taken { 0 } else { 1 }]
        };
        return Some((spec.bytes, cycles));
    }
    None
}
pub(crate) fn parse_num(s: &str) -> Option<u16> {
    let s = s.trim();
    if let Some(x) = s.strip_prefix("-$") {
        Some(0u16.wrapping_sub(u16::from_str_radix(x, 16).ok()?))
    } else if let Some(x) = s.strip_prefix('-') {
        Some(0u16.wrapping_sub(x.parse::<u16>().ok()?))
    } else if let Some(x) = s.strip_prefix('$') {
        u16::from_str_radix(x, 16).ok()
    } else if let Some(x) = s.strip_prefix("0X") {
        u16::from_str_radix(x, 16).ok()
    } else {
        s.parse().ok()
    }
}
