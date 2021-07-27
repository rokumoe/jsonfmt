use std::env;
use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::process;

use jsonfmt::*;

fn fatal(msg: String) -> ! {
    eprintln!("{}", msg);
    process::exit(1)
}

fn show_help(code: i32) -> ! {
    println!("jsonfmt [options...] [file]");
    println!();
    println!("  -i <width> indent width");
    println!("  -w         write back");
    println!("  -f         fast");
    process::exit(code)
}

fn main() {
    let mut indent = 2usize;
    let mut write_back = false;
    let mut fast = false;
    let mut input = None;
    let mut args = env::args();
    args.next();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-i" => {
                if let Some(n) = args.next().and_then(|n| n.parse().ok()) {
                    indent = n;
                } else {
                    show_help(1);
                }
            }
            "-f" => fast = true,
            "-w" => write_back = true,
            "-h" => show_help(0),
            _ => {
                input = Some(arg);
                break;
            }
        }
    }

    let mut input_file = input
        .as_ref()
        .map(|fname| match File::open(fname.as_str()) {
            Ok(f) => f,
            Err(e) => fatal(e.to_string()),
        });
    let mut stdin = io::stdin();
    let r: &mut dyn Read = if let Some(f) = input_file.as_mut() {
        f
    } else {
        &mut stdin
    };
    let mut br = BufReader::with_capacity(256, r);

    let mut stdout = io::stdout();
    let mut buf = Vec::<u8>::new();
    let w: &mut dyn Write = if write_back && input.is_some() {
        &mut buf
    } else {
        &mut stdout
    };
    let mut indent = Indent::new(indent);
    let res = if fast {
        format_json_fast(w, &mut br, &mut indent)
    } else {
        format_json(w, &mut br, &mut indent)
    };
    if let Err(e) = res {
        fatal(e.to_string());
    }

    if let (Some(fname), true) = (input, write_back) {
        if let Err(e) = File::create(fname.as_str()).and_then(|mut f| f.write_all(&buf)) {
            fatal(e.to_string());
        }
    }
}
