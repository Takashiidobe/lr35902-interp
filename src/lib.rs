use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod error;
mod instructions;
mod parser;
mod preprocessor;
mod vm;

pub use error::AssemblyError;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Inputs {
    pub registers: BTreeMap<String, u16>,
    pub flags: BTreeMap<String, bool>,
    pub memory: BTreeMap<u16, u8>,
    pub entry: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionResult {
    pub line: usize,
    pub instruction: String,
    pub bytes: u8,
    pub cycles: Option<u16>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub bytes: u32,
    pub cycles: Option<u32>,
    pub executed_instructions: u64,
    pub instructions: Vec<InstructionResult>,
}
pub fn analyze(source: &str, inputs: &Inputs) -> Result<Report, AssemblyError> {
    let preprocessed = preprocessor::preprocess(source)?;
    let (instructions, mut report) = parser::parse(&preprocessed, inputs)?;
    let (cycles, steps) =
        vm::execute(&instructions, &preprocessed.text, inputs).map_err(|error| match error {
            AssemblyError::ExecutionLimit { steps, line } => AssemblyError::ExecutionLimit {
                steps,
                line: preprocessed
                    .source_lines
                    .get(line.saturating_sub(1))
                    .copied()
                    .unwrap_or(line),
            },
            AssemblyError::Label { line, target } if line > 0 => AssemblyError::Label {
                line: preprocessed
                    .source_lines
                    .get(line.saturating_sub(1))
                    .copied()
                    .unwrap_or(line),
                target,
            },
            other => other,
        })?;
    report.cycles = Some(cycles);
    report.executed_instructions = steps;
    Ok(report)
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn analyze_wasm(source: &str, registers_json: &str) -> Result<String, wasm_bindgen::JsValue> {
    let i: Inputs = serde_json::from_str(registers_json).unwrap_or_default();
    analyze(source, &i)
        .map(|r| serde_json::to_string(&r).unwrap())
        .map_err(|e| wasm_bindgen::JsValue::from_str(&e.to_string()))
}
