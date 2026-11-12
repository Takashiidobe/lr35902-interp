use miette::Diagnostic;
use thiserror::Error;

#[derive(Debug, Error, Diagnostic)]
pub enum AssemblyError {
    #[error("line {line}: preprocessing error: {message}")]
    #[diagnostic(code(lr35902::preprocess))]
    Preprocess { line: usize, message: String },
    #[error("line {line}: unknown or unsupported instruction `{text}`")]
    #[diagnostic(code(lr35902::instruction))]
    Unknown { line: usize, text: String },
    #[error("line {line}: invalid operand or condition `{text}`")]
    #[diagnostic(code(lr35902::operand))]
    Operand { line: usize, text: String },
    #[error("execution exceeded {steps} instructions at line {line}; possible infinite loop")]
    #[diagnostic(code(lr35902::execution_limit))]
    ExecutionLimit { steps: u64, line: usize },
    #[error("line {line}: unresolved branch target `{target}`")]
    #[diagnostic(code(lr35902::label))]
    Label { line: usize, target: String },
}
