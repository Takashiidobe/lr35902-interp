use crate::instructions::{InstructionKind, Operand, ParsedInstruction, opcode_metadata};

pub(crate) fn parse(
    source: &crate::preprocessor::PreprocessedSource,
    inputs: &crate::Inputs,
) -> Result<(Vec<ParsedInstruction>, crate::Report), crate::AssemblyError> {
    let mut out = crate::Report {
        bytes: 0,
        cycles: Some(0),
        executed_instructions: 0,
        instructions: vec![],
    };
    let mut parsed = Vec::<ParsedInstruction>::new();
    for (idx, raw) in source.text.lines().enumerate() {
        let source_line = source.source_lines.get(idx).copied().unwrap_or(idx + 1);
        let mut line = raw.split(';').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some((label, rest)) = line.split_once(':')
            && !label.contains(char::is_whitespace)
        {
            line = rest.trim_start_matches(':').trim();
            if line.is_empty() {
                continue;
            }
        }
        let head = line
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_end_matches(':')
            .to_ascii_uppercase();
        if [
            "SECTION",
            "EXPORT",
            "GLOBAL",
            "PURGE",
            "INCLUDE",
            "INCBIN",
            "DEF",
            "EQU",
            "REDEF",
            "ASSERT",
            "PRINTT",
            "PRINTV",
            "PRINTI",
            "PRINTW",
            "PRINTD",
            "FAIL",
            "WARN",
            "OPT",
            "PUSHO",
            "POPO",
            "NEWCHARMAP",
            "CHARMAP",
            "ENDC",
            "ELSE",
            "ELIF",
            "IF",
            "IFDEF",
            "IFNDEF",
            "IFN",
            "IFB",
            "IFNB",
            "MACRO",
            "ENDM",
            "REPT",
            "ENDR",
            "FOR",
            "NEXT",
            "SHIFT",
            "UNION",
            "NEXTU",
            "ENDU",
            "LOAD",
            "ENDL",
            "ALIGN",
        ]
        .contains(&head.as_str())
            || line.starts_with('.')
        {
            continue;
        }
        if ["DB", "DW", "DL", "DS", "RB", "RW"].contains(&head.as_str()) {
            let rest = line[head.len()..].trim();
            let args: Vec<&str> = rest
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();
            let width = match head.as_str() {
                "DB" | "RB" => 1,
                "DW" | "RW" => 2,
                "DL" => 4,
                _ => 0,
            };
            let size = if head == "DS" {
                args.first()
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(0)
            } else {
                args.iter()
                    .map(|s| {
                        if head == "DB" && s.starts_with('"') && s.ends_with('"') {
                            s[1..s.len() - 1].len() as u32
                        } else {
                            width
                        }
                    })
                    .sum()
            };
            out.bytes += size;
            out.instructions.push(crate::InstructionResult {
                line: source_line,
                instruction: line.into(),
                bytes: size.min(255) as u8,
                cycles: None,
            });
            parsed.push(ParsedInstruction {
                kind: None,
                operands: vec![],
                line: idx + 1,
                bytes: size.min(255) as u8,
                cycles: None,
            });
            continue;
        }
        let mut p = line.splitn(2, char::is_whitespace);
        let m = p.next().unwrap().to_ascii_uppercase();
        let rest = p.next().unwrap_or("");
        let ops: Vec<String> = rest
            .split(',')
            .map(|x| {
                x.trim()
                    .trim_start_matches('%')
                    .to_ascii_uppercase()
                    .replace('[', "(")
                    .replace(']', ")")
            })
            .filter(|x| !x.is_empty())
            .collect();
        let refs: Vec<&str> = ops.iter().map(String::as_str).collect();
        let (bytes, cy) = opcode_metadata(&m, &refs, inputs).ok_or_else(|| {
            if InstructionKind::parse(&m).is_some() {
                crate::AssemblyError::Operand {
                    line: source_line,
                    text: line.into(),
                }
            } else {
                crate::AssemblyError::Unknown {
                    line: source_line,
                    text: m.clone(),
                }
            }
        })?;
        let kind = InstructionKind::parse(&m).ok_or_else(|| crate::AssemblyError::Unknown {
            line: source_line,
            text: m.clone(),
        })?;
        parsed.push(ParsedInstruction {
            kind: Some(kind),
            operands: refs.iter().map(|x| Operand::parse(x)).collect(),
            line: idx + 1,
            bytes,
            cycles: Some(cy),
        });
        out.bytes += bytes as u32;
        out.cycles = out.cycles.map(|v| v + cy as u32);
        out.instructions.push(crate::InstructionResult {
            line: source_line,
            instruction: line.into(),
            bytes,
            cycles: Some(cy),
        });
    }
    Ok((parsed, out))
}
