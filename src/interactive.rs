use std::{
    fmt,
    io::{stdin, stdout, Error as IoError, Write},
    path::{Path, PathBuf},
};
use crate::benc::Value as BencValue;

pub enum Error {
    Io(IoError),
    InvalidFile(String),
}

enum CmdError {
    UnknownCommand(String),
    ArgUnknownEscape(usize, char),
    ArgTrailingEscape,
    ArgEOL,
    ArgCount(usize),
    ArgCountMin(usize),
    ArgCountMax(usize),
}

struct State {
    path: PathBuf,
    data: Option<BencValue>,
}

pub fn interactive<P>(file: P) -> Result<(), Error> where P: AsRef<Path> {
    let mut state = State::new(file.as_ref())?;

    let mut input_buffer = String::new();

    loop {
        {
            let mut out = stdout();
            out.write_all(b"bencedit> ")?;
            out.flush()?;
        }
        stdin().read_line(&mut input_buffer)?;
        let input = input_buffer.trim();
        let space_at = input.find(' ');

        let (cmd, argbuf) = if let Some(space_at) = space_at {
            let s = input.split_at(space_at);
            (Some(s.0), s.1)
        } else {
            (if input.len() > 0 { Some(input) } else { None }, "")
        };

        if let Some(cmd) = cmd {
            let cmd = cmd.to_lowercase();
            let argbuf = if argbuf.len() > 0 {
                &argbuf[1..]
            } else {
                argbuf
            };

            match interactive_cmd(&mut state, cmd, argbuf) {
                Ok(keep_running) => if !keep_running { break; },
                Err(e) => eprintln!("Error: {}", e),
            }
        }

        input_buffer.clear();
    }

    Ok(())
}

fn interactive_cmd(state: &mut State, cmd: String, argbuf: &str) -> Result<bool, CmdError> {
    let args = parse_args(argbuf)?;

    Ok(match cmd.as_ref() {
        "show" => {
            if args.len() > 1 {
                return Err(CmdError::ArgCountMax(1));
            }

            let data = state.data.as_ref().unwrap();

            if let Some(arg) = args.iter().next() {
                println!("TODO");
            } else {
                println!("{}", data);
            }

            true
        }

        "quit" | "exit" | "q" => false,
        _ => return Err(CmdError::UnknownCommand(cmd)),
    })
}

fn parse_args(buf: &str) -> Result<Vec<String>, CmdError> {
    let mut args = vec![];
    let mut escaped = false;
    let mut quoted = false;
    let mut left = 0usize;

    if buf.len() > 0 {
        args.push(String::new());
    } else {
        return Ok(args);
    }

    for (i, c) in buf.chars().enumerate() {
        if c == ' ' && !quoted {
            args.last_mut().unwrap().extend(buf[left..i].chars());
            args.push(String::new());
            left = i + 1;
        } else if c == '"' {
            if escaped {
                escaped = false;
            } else {
                args.last_mut().unwrap().extend(buf[left..i].chars());
                quoted = !quoted;
                left = i + 1;
            }
        } else if c == '\\' {
            if escaped {
                escaped = false;
            } else {
                args.last_mut().unwrap().extend(buf[left..i].chars());
                escaped = true;
                left = i + 1;
            }
        } else if escaped {
            if c == 'n' {
                args.last_mut().unwrap().push('\n');
            } else {
                return Err(CmdError::ArgUnknownEscape(i, c));
            }

            left = i + 1;
            escaped = false;
        }
    }

    if quoted {
        return Err(CmdError::ArgEOL);
    }

    if escaped {
        return Err(CmdError::ArgTrailingEscape);
    }

    args.last_mut().unwrap().extend(buf[left..].chars());
    Ok(args)
}

impl State {
    pub fn new<P: Into<PathBuf>>(path: P) -> Result<Self, Error> {
        let mut me = Self {
            path: path.into(),
            data: None,
        };

        me.reload()?;
        Ok(me)
    }

    pub fn reload(&mut self) -> Result<(), Error> {
        use std::fs::File;
        use crate::benc::load;

        let mut fp = File::open(&self.path)?;
        println!("Loading {}", self.path.display());

        match load(&mut fp) {
            Ok(v) => self.data = Some(v),
            Err(e) => return Err(Error::InvalidFile(format!("{}", e))),
        }

        Ok(())
    }
}

impl From<IoError> for Error {
    fn from(e: IoError) -> Self {
        Self::Io(e)
    }
}

impl fmt::Display for CmdError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::UnknownCommand(name) => write!(f, "Unknown command '{}'", name),
            Self::ArgUnknownEscape(pos, c) => write!(f, "Unknown escape character '{}' at {}", c, pos + 1),
            Self::ArgTrailingEscape => write!(f, "Trailing escape character"),
            Self::ArgEOL => write!(f, "Reached end of line trying to match quote"),
            Self::ArgCount(n) => write!(f, "Expected {} arguments", n),
            Self::ArgCountMin(n) => write!(f, "Expected at least {} arguments", n),
            Self::ArgCountMax(n) => write!(f, "Expected at most {} arguments", n),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO Error: {}", e),
            Self::InvalidFile(s) => write!(f, "File is invalid - {}", s),
        }
    }
}
