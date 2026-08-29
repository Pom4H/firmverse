use firmverse::controller::saturn_compiler::{compile_control_ir, parse_control_ir_json};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args_os().skip(1);
    let input = PathBuf::from(args.next().ok_or_else(|| {
        "usage: saturn-fbd-compile <control-ir.json> <program.fbdbin>".to_string()
    })?);
    let output = PathBuf::from(args.next().ok_or_else(|| {
        "usage: saturn-fbd-compile <control-ir.json> <program.fbdbin>".to_string()
    })?);
    if args.next().is_some() {
        return Err("usage: saturn-fbd-compile <control-ir.json> <program.fbdbin>".into());
    }

    let source =
        fs::read_to_string(&input).map_err(|error| format!("{}: {error}", input.display()))?;
    let ir = parse_control_ir_json(&source)?;
    let compiled = compile_control_ir(&ir)?;
    fs::write(&output, &compiled.fbdbin)
        .map_err(|error| format!("{}: {error}", output.display()))?;

    println!(
        "COMPILED {} bytes={} elements={} screens={} rtl={}",
        output.display(),
        compiled.fbdbin.len(),
        compiled.element_count,
        compiled.screen_count,
        compiled.required_rtl
    );
    for row in compiled.listing {
        println!(
            "ELEMENT {} id={} type={} inputs={} params={} {}",
            row.index,
            row.id,
            row.kind,
            row.inputs.join(","),
            row.params
                .iter()
                .map(i32::to_string)
                .collect::<Vec<_>>()
                .join(","),
            row.comment
        );
    }
    Ok(())
}
