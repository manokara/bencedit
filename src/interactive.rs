use std::{
    fmt,
    io::{stdin, stdout, Error as IoError, Write},
    path::{Path, PathBuf},
};
use bencode::Value as BencValue;
use rustyline::{
    error::ReadlineError,
    Editor,
};

pub enum Error {
    Io(IoError),
    InvalidFile(String),
}

enum CmdError {
    Io(IoError),
    UnknownCommand(String),
    Command(String),
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
    changed: bool,
}

pub fn interactive<P>(file: P) -> Result<(), Error> where P: AsRef<Path> {
    let mut state = State::new(file.as_ref())?;
    let mut rl = Editor::<()>::new();

    loop {
        let indicator = if state.changed { " *" } else { "" };
        let readline = rl.readline(&format!("bencedit{}> ", indicator));

        match readline {
            Ok(line) => {
                let input = line.trim();
                let space_at = input.find(' ');
                rl.add_history_entry(input);

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
            }

            Err(ReadlineError::Interrupted) => {}
            Err(ReadlineError::Eof) => {
                use std::fs::File;

                if state.changed {
                    let confirm = prompt_confirm("There were changes made, do you want to save them?")?;

                    if confirm {
                        let path = &state.path.canonicalize()?;
                        let mut file = File::create(path)?;

                        println!("Saving...");
                        state.data.as_ref().unwrap().encode(&mut file)?;
                    }
                }

                break
            },
            Err(e) => {
                println!("ERROR: {:?}", e);
                break
            }
        }
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
            let selector = args.iter().next().map(|s| s.as_str()).unwrap_or("");
            let value = data.select(selector)?;
            println!("{}", value);

            true
        }

        "set" => {
            use nanoserde::DeJson;

            if args.len() != 2 {
                return Err(CmdError::ArgCount(2));
            }

            let old_hash = hash_value(state.data.as_ref().unwrap().select(&args[0])?);
            let old_value = state.data.as_mut().unwrap().select_mut(&args[0])?;

            match BencValue::deserialize_json(&args[1]) {
                Ok(value) => {
                    let new_hash = hash_value(&value);
                    *old_value = value;

                    if new_hash != old_hash {
                        state.changed = true;
                    }
                },
                Err(e) => return Err(CmdError::Command(
                    format!("{}, at {}:{}", e.msg.trim_end(), e.line + 1, e.col)
                )),
            }

            true
        }

        "reload" => {
            if args.len() != 0 {
                return Err(CmdError::ArgCount(0));
            }

            let confirm = if state.changed {
                prompt_confirm("There were changes made, are you sure?")?
            } else {
                true
            };

            if confirm {
                state.reload_data()
                    .map(|_| true)
                    .map_err(|e| CmdError::Command(format!("{}", e)))?;

                state.changed = false;
            }

            true
        }

        "save" => {
            use std::fs::File;

            if !state.changed {
                println!("No changes to be saved.");
                return Ok(true);
            }

            let path = &state.path.canonicalize()?;
            let mut file = File::create(path)?;

            println!("Saving...");
            state.data.as_ref().unwrap().encode(&mut file)?;
            state.changed = false;

            true
        }

        "save-as" => {
            use std::fs::File;

            if args.len() != 1 {
                return Err(CmdError::ArgCount(1));
            }

            let path = Path::new(&args[0]);
            let confirm = if path.exists() {
                prompt_confirm(&format!("Path {} exists. Overwrite?", path.display()))?
            } else {
                true
            };

            if confirm {
                let mut file = File::create(path.clone())?;

                println!("Saving to {}...", path.display());
                state.data.as_ref().unwrap().encode(&mut file)?;
                state.changed = false;
            }

            true
        }

        "clear" => {
            if args.len() > 1 {
                return Err(CmdError::ArgCountMax(1));
            }

            let selector = args.iter().map(|s| s.as_str()).next().unwrap_or("");
            let value = state.data.as_mut().unwrap().select_mut(selector)?;

            match value {
                BencValue::Dict(m) => m.clear(),
                BencValue::List(v) => v.clear(),
                BencValue::Str(s) => s.clear(),
                BencValue::Bytes(v) => v.clear(),
                BencValue::Int(i) => *i = 0,
            }

            state.changed = true;
            true
        }

        "remove" => {
            use bencode::ValueAccessor;

            if args.len() != 1 {
                return Err(CmdError::ArgCount(1));
            }

            let selector = &args[0];
            let _ = state.data.as_ref().unwrap().select(selector)?; // Syntax + exists + parent is container check

            let (parent_selector, child_selector) = if let Some(i) = selector.rfind(|c| c == '.' || c == '[') {
                (&selector[0..i], &selector[i..])
            } else {
                ("", "")
            };

            let parent = state.data.as_mut().unwrap().select_mut(parent_selector)?;

            let a = if child_selector.starts_with('.') {
                ValueAccessor::Key(&child_selector[1..])
            } else {
                let end = child_selector.len() - 1;
                let index = child_selector[1..end]
                    .parse::<usize>()
                    .unwrap();

                ValueAccessor::Index(index)
            };

            parent.remove(a)?;
            state.changed = true;
            true
        }

        "insert" => {
            use bencode::ValueAccessor;
            use nanoserde::DeJson;

            if args.len() != 3 {
                return Err(CmdError::ArgCount(3));
            }

            let root = state.data.as_mut().unwrap();
            let container = root.select_mut(&args[0])?;

            let value = match BencValue::deserialize_json(&args[2]) {
                Ok(value) => value,
                Err(e) => return Err(CmdError::Command(
                    format!("{}, at {}:{}", e.msg.trim_end(), e.line + 1, e.col)
                )),
            };
            let a: ValueAccessor = match &args[1].parse::<usize>() {
                Ok(i) => (*i).into(),
                Err(_) => (&args[1]).into(),
            };
            container.insert(a, value)?;
            state.changed = true;

            true
        }

        "append" => {
            use nanoserde::DeJson;

            if args.len() != 2 {
                return Err(CmdError::ArgCount(2));
            }

            let root = state.data.as_mut().unwrap();
            let container = root.select_mut(&args[0])?;

            if !container.is_list() {
                return Err(CmdError::Command("Value is not a list".into()));
            }

            let value = match BencValue::deserialize_json(&args[1]) {
                Ok(value) => value,
                Err(e) => return Err(CmdError::Command(
                    format!("{}, at {}:{}", e.msg.trim_end(), e.line + 1, e.col)
                )),
            };

            container.push(value)?;
            state.changed = true;

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

fn prompt_confirm(prompt: &str) -> Result<bool, IoError> {
    let mut buffer = String::new();

    {
        let mut out = stdout();
        out.write_all(prompt.as_bytes())?;
        out.write_all(b" (y/N): ")?;
        out.flush()?;
    }

    stdin().read_line(&mut buffer)?;
    let input = buffer.trim().to_lowercase();

    Ok(if let Some(c) = input.chars().next() {
        match c {
            'y' => true,
            _ => false,
        }
    } else {
        false
    })
}

fn hash_value(root: &BencValue) -> u64 {
    use std::{
        collections::hash_map::DefaultHasher,
        hash::{Hash, Hasher},
    };
    use bencode::TraverseAction;

    let hasher = &mut DefaultHasher::new();

    if let Some(i) = root.to_i64() {
        i.hash(hasher);
    } else if let Some(s) = root.to_bytes() {
        s.hash(hasher);
    } else if let Some(s) = root.to_str() {
        s.hash(hasher);
    } else {
        let _ = root.traverse::<_, ()>(|key, index, parent, value, _| {
            if let Some(value) = value {
                if value.is_container() {
                    return Ok(TraverseAction::Enter);
                }

                if let Some(key) = key {
                    key.hash(hasher);
                } else if let Some(index) = index {
                    index.hash(hasher);
                }

                if let Some(i) = value.to_i64() {
                    i.hash(hasher);
                } else if let Some(s) = value.to_bytes() {
                    s.hash(hasher);
                } else if let Some(s) = value.to_str() {
                    s.hash(hasher);
                }
            } else if parent != root {
                return Ok(TraverseAction::Exit);
            }

            Ok(TraverseAction::Continue)
        });
    }

    hasher.finish()
}

impl State {
    pub fn new<P: Into<PathBuf>>(path: P) -> Result<Self, Error> {
        let mut me = Self {
            path: path.into(),
            data: None,
            changed: false,
        };

        me.reload_data()?;
        Ok(me)
    }

    pub fn reload_data(&mut self) -> Result<(), Error> {
        use std::fs::File;
        use bencode::load;

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

impl From<IoError> for CmdError {
    fn from(e: IoError) -> Self {
        Self::Io(e)
    }
}

impl From<bencode::SelectError> for CmdError {
    fn from(e: bencode::SelectError) -> Self {
        Self::Command(format!("{}", e))
    }
}

impl From<bencode::UpdateError> for CmdError {
    fn from(e: bencode::UpdateError) -> Self {
        Self::Command(format!("{}", e))
    }
}

impl fmt::Display for CmdError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO Error: {}", e),
            Self::UnknownCommand(name) => write!(f, "Unknown command '{}'", name),
            Self::Command(msg) => write!(f, "Command failed with: {}", msg),
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

