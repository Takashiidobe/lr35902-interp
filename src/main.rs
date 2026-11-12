use clap::Parser;
use lr35902_interp::{Inputs, analyze};
use miette::Result;
use std::fs;
#[derive(Parser)]
#[command(about = "Estimate LR35902 assembly size and cycles")]
struct Cli {
    #[arg(value_name = "FILE")]
    file: Option<String>,
    #[arg(short, long)]
    asm: Option<String>,
    #[arg(long = "reg", value_name = "NAME=VALUE")]
    regs: Vec<String>,
    #[arg(long = "flag", value_name = "Z=true")]
    flags: Vec<String>,
    #[arg(long = "mem", value_name = "ADDRESS=BYTE")]
    memory: Vec<String>,
    #[arg(long)]
    entry: Option<String>,
}
fn number(s: &str) -> u16 {
    let s = s.trim();
    if let Some(x) = s.strip_prefix("0x").or_else(|| s.strip_prefix('$')) {
        u16::from_str_radix(x, 16).unwrap_or(0)
    } else {
        s.parse().unwrap_or(0)
    }
}
fn main() -> Result<()> {
    let c = Cli::parse();
    let mut i = Inputs {
        entry: c.entry,
        ..Default::default()
    };
    let source = if let Some(a) = c.asm {
        a
    } else if let Some(f) = c.file {
        fs::read_to_string(f).map_err(|e| miette::miette!(e.to_string()))?
    } else {
        use std::io::Read;
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .map_err(|e| miette::miette!(e.to_string()))?;
        s
    };
    for r in c.regs {
        if let Some((k, v)) = r.split_once('=') {
            i.registers.insert(k.to_uppercase(), number(v));
        }
    }
    for f in c.flags {
        if let Some((k, v)) = f.split_once('=') {
            i.flags.insert(k.to_uppercase(), v.parse().unwrap_or(false));
        }
    }
    for m in c.memory {
        if let Some((a, v)) = m.split_once('=') {
            i.memory.insert(number(a), number(v) as u8);
        }
    }
    let result = analyze(&source, &i).map_err(miette::Report::new)?;
    println!(
        "{} bytes - {} cycles - {} executed instructions",
        result.bytes,
        result.cycles.unwrap_or(0),
        result.executed_instructions
    );
    for r in result.instructions {
        println!("{:>4}  {:>2} bytes  {}", r.line, r.bytes, r.instruction);
    }
    Ok(())
}
