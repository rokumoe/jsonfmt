use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Result, Write};
use std::process;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Value,
    Object,
    Pair,
    Array,
    Elem,
}

fn error_msg(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::Other, msg)
}

const BAD_CHAR: u8 = b'\x00';

fn peek_char(r: &mut BufReader<&mut dyn Read>) -> u8 {
    if let Ok(b) = r.fill_buf() {
        return *b.get(0).unwrap_or(&BAD_CHAR);
    }
    BAD_CHAR
}

fn skip_whitespace(br: &mut BufReader<&mut dyn Read>) -> Result<()> {
    loop {
        let buf = br.fill_buf()?;
        let mut i = 0usize;
        while i < buf.len() {
            if !buf[i].is_ascii_whitespace() {
                br.consume(i);
                return Ok(());
            }
            i += 1;
        }
        br.consume(i);
    }
}

fn write_string(w: &mut dyn Write, br: &mut BufReader<&mut dyn Read>) -> Result<()> {
    let mut buf = br.fill_buf()?;
    assert!(buf[0] == b'"');
    let mut i = 1usize;
    let mut prev = BAD_CHAR;
    loop {
        if i >= buf.len() {
            w.write_all(buf)?;
            br.consume(i);
            buf = br.fill_buf()?;
            if buf.is_empty() {
                return Err(error_msg(format!("unclosed string")));
            }
            i = 0;
        }
        let c = buf[i];
        i += 1;
        if c == b'"' && prev != b'\\' {
            break;
        } else if c == b'\\' && prev == b'\\' {
            prev = BAD_CHAR;
        } else {
            prev = c;
        }
    }
    w.write_all(&buf[..i])?;
    br.consume(i);
    Ok(())
}

fn write_number(w: &mut dyn Write, br: &mut BufReader<&mut dyn Read>) -> Result<()> {
    let mut buf = br.fill_buf()?;
    let mut i = 1usize;
    loop {
        if i >= buf.len() {
            w.write_all(buf)?;
            br.consume(i);
            buf = br.fill_buf()?;
            if buf.is_empty() {
                break;
            }
            i = 0;
        }
        let c = buf[i];
        if !c.is_ascii_digit() && c != b'-' && c != b'.' && c != b'+' && c != b'e' && c != b'E' {
            w.write_all(&buf[..i])?;
            br.consume(i);
            break;
        }
        i += 1;
    }
    Ok(())
}

fn write_expected(
    w: &mut dyn Write,
    br: &mut BufReader<&mut dyn Read>,
    expect: &[u8],
) -> Result<()> {
    const MAX_ID_LEN: usize = 5;
    assert!(expect.len() - 1 <= MAX_ID_LEN);
    let mut buf = [0u8; MAX_ID_LEN];
    br.read_exact(&mut buf[..expect.len()])?;
    if expect != &buf[..expect.len()] {
        for (i, &c) in expect.iter().enumerate() {
            if c != buf[i] {
                return Err(error_msg(format!("invalid input: {}", buf[i] as char)));
            }
        }
    }
    w.write_all(expect)
}

struct Indent {
    one_tab: String,
    prefix: String,
}

impl Indent {
    fn push(&mut self) {
        self.prefix.push_str(&self.one_tab);
    }

    fn pop(&mut self) {
        self.prefix.truncate(self.prefix.len() - self.one_tab.len());
    }

    fn write_to(&self, w: &mut dyn Write) -> Result<()> {
        w.write_all(self.prefix.as_bytes())
    }
}

fn format_json(
    w: &mut dyn Write,
    br: &mut BufReader<&mut dyn Read>,
    indent: &mut Indent,
) -> Result<()> {
    let mut stack = vec![State::Value];
    while let Some(state) = stack.pop() {
        skip_whitespace(br)?;
        match state {
            State::Value => match peek_char(br) {
                c if c == b'{' || c == b'[' => {
                    br.consume(1);
                    w.write(&[c, b'\n'])?;
                    indent.push();
                    stack.push(if c == b'{' {
                        State::Object
                    } else {
                        State::Array
                    });
                }
                b'"' => write_string(w, br)?,
                c if c.is_ascii_digit() || c == b'-' => write_number(w, br)?,
                b'n' => write_expected(w, br, b"null")?,
                b't' => write_expected(w, br, b"true")?,
                b'f' => write_expected(w, br, b"false")?,
                c => return Err(error_msg(format!("unexpected input: '{}'", c as char))),
            },
            State::Pair | State::Elem => {
                let c = peek_char(br);
                if c == b',' {
                    br.consume(1);
                    w.write(b",")?;
                } else if state == State::Pair && c != b'}' || state == State::Elem && c != b']' {
                    return Err(error_msg(format!("unexpected input: '{}'", c as char)));
                }
                w.write(b"\n")?;
                stack.push(if state == State::Pair {
                    State::Object
                } else {
                    State::Array
                });
            }
            State::Object => match peek_char(br) {
                b'"' => {
                    indent.write_to(w)?;
                    write_string(w, br)?;
                    skip_whitespace(br)?;
                    write_expected(w, br, b":")?;
                    w.write(b" ")?;
                    stack.push(State::Pair);
                    stack.push(State::Value);
                }
                b'}' => {
                    br.consume(1);
                    indent.pop();
                    indent.write_to(w)?;
                    w.write(b"}")?;
                }
                c => return Err(error_msg(format!("unexpected input: '{}'", c as char))),
            },
            State::Array => match peek_char(br) {
                b']' => {
                    br.consume(1);
                    indent.pop();
                    indent.write_to(w)?;
                    w.write(b"]")?;
                }
                _ => {
                    indent.write_to(w)?;
                    stack.push(State::Elem);
                    stack.push(State::Value);
                }
            },
        }
    }
    Ok(())
}

fn fatal(msg: String) -> ! {
    eprintln!("{}", msg);
    process::exit(1)
}

fn show_help(code: i32) -> ! {
    println!("jsonfmt [options...] [file]");
    println!();
    println!("  -i <width> indent width");
    println!("  -w         write back");
    process::exit(code)
}

fn main() {
    let mut indent = 2usize;
    let mut write_back = false;
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
    let mut indent = Indent {
        prefix: String::new(),
        one_tab: " ".repeat(indent),
    };
    if let Err(e) = format_json(w, &mut br, &mut indent) {
        fatal(e.to_string());
    }

    if let (Some(fname), true) = (input, write_back) {
        if let Err(e) = File::create(fname.as_str()).and_then(|mut f| f.write_all(&buf)) {
            fatal(e.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_json() {
        let mut outbuf = Vec::<u8>::new();
        let input =
            br#"{ "a" : 1 , "b": "test\\", "c": false, "d": null, "e": 1.234e5, "f":  [ 1, 2  ] , "g"   : {}}"#;
        let r: &mut dyn Read = &mut &input[..];
        let mut br = BufReader::new(r);
        let mut indent = Indent {
            prefix: String::new(),
            one_tab: " ".repeat(4),
        };
        format_json(&mut outbuf, &mut br, &mut indent).unwrap();
        println!("result: {}", String::from_utf8(outbuf).unwrap());
    }
}
