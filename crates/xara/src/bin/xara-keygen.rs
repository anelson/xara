use std::path::PathBuf;
use std::process::ExitCode;
use xara::cli::{self, Usage};
use xara_phy::hf::Keying;

const USAGE: Usage = Usage {
    name: "xara-keygen",
    synopsis: "usage: xara-keygen --name NAME --out DIR",
    tagline: "make the table files for a named HF keying",
    sections: &["xara-keygen"],
};

fn parse() -> Result<Option<(String, PathBuf)>, String> {
    let (mut name, mut out) = (None, None);
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        if cli::meta(&k, &USAGE) {
            return Ok(None);
        }
        let mut v = || it.next().ok_or(format!("{k} needs a value"));
        match k.as_str() {
            "--name" => name = Some(v()?),
            "--out" => out = Some(PathBuf::from(v()?)),
            _ => return Err(format!("unknown option {k}")),
        }
    }
    let name = name.ok_or("--name is required")?;
    xara_phy::hf::check_name(&name)?;
    Ok(Some((name, out.ok_or("--out is required")?)))
}

fn main() -> ExitCode {
    let (name, out) = match parse() {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xara-keygen: {e}\n{}", USAGE.synopsis);
            return ExitCode::from(2);
        }
    };
    match Keying::write_new(&out, &name) {
        Ok(k) => {
            println!("keying {name:?} (generator {}) written to {}", Keying::GENERATOR, out.display());
            println!("control-code seed offset: {}", k.seed_bias());
            println!("use it with: --vara-dir {}", out.display());
            println!("the name is the whole definition: publish it and anyone can rebuild these tables.");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("xara-keygen: {e}");
            ExitCode::FAILURE
        }
    }
}
