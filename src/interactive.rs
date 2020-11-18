use std::{
    fmt,
    io::{stdin, stdout, Error as IoError, Write},
    path::{Path, PathBuf},
};
use bencode::Value as BencValue;

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

    let mut input_buffer = String::new();

    loop {
        let indicator = if state.changed { " *" } else { "" };

        {
            let mut out = stdout();
            out.write_all(format!("bencedit{}> ", indicator).as_bytes())?;
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

            state.reload_data()
                .map(|_| true)
                .map_err(|e| CmdError::Command(format!("{}", e)))?
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

            let (key, index) = if child_selector.starts_with('.') {
                (&child_selector[1..], 0)
            } else {
                let end = child_selector.len() - 1;
                ("", (&child_selector[1..end]).parse::<usize>().unwrap())
            };

            if let Some(m) = parent.to_map_mut() {
                m.remove(key);
            } else if let Some(v) = parent.to_vec_mut() {
                v.remove(index);
            }

            state.changed = true;
            true
        }

        "insert" => {
            if args.len() != 3 {
                return Err(CmdError::ArgCount(3));
            }

            let root = state.data.as_mut().unwrap();
            value_insert(root, &args[0], Some(&args[1]), &args[2])?;
            state.changed = true;

            true
        }

        "append" => {
            if args.len() != 2 {
                return Err(CmdError::ArgCount(2));
            }

            let root = state.data.as_mut().unwrap();
            value_insert(root, &args[0], None, &args[1])?;
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

fn prompt_confirm(prompt: &str) -> Result<bool, CmdError> {
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

fn value_insert(root: &mut BencValue, selector: &str, ident: Option<&str>, value: &str) -> Result<(), CmdError> {
    use nanoserde::DeJson;

    let value = match BencValue::deserialize_json(value) {
        Ok(value) => value,
        Err(e) => return Err(CmdError::Command(
            format!("{}, at {}:{}", e.msg.trim_end(), e.line + 1, e.col)
        )),
    };

    let parent_len = root.select(selector)?.len();
    let parent = root.select_mut(selector)?;

    if let Some(m) = parent.to_map_mut() {
        if ident.is_none() {
            return Err(CmdError::Command("Appending can only be done on lists".into()));
        }

        m.insert(ident.unwrap().into(), value);
    } else if let Some(v) = parent.to_vec_mut() {
        let index = if let Some(ident) = ident {
            match ident.parse::<usize>() {
                Ok(i) => i,
                Err(_) => return Err(CmdError::Command("Index is not a number".into())),
            }
        } else {
            parent_len
        };

        if index > v.len() {
            return Err(CmdError::Command("Index out of bounds".into()));
        }

        v.insert(index, value);
    } else {
        return Err(CmdError::Command("Value is not a container".into()));
    }

    Ok(())
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
