const MAX_INT_BUF: usize = 32;
const CHUNK_SIZE: u64 = 2048;

use std::{
    cell::RefCell,
    collections::BTreeMap,
    convert::{TryFrom, TryInto},
    fmt,
    io::{Cursor, Error as IoError, Read, Seek, SeekFrom},
    rc::Rc,
};

enum Token {
    Dict,
    Int,
    List,
    End,
    Colon,
}

#[derive(Debug, PartialEq)]
enum State {
    Root,
    Dict,
    Int,
    Str,
    DictKey,
    DictVal,
    StrRem,
    DictFlush,
    DictValStr,
    DictValInt,
    DictValDict,
    DictValList,
    ListVal,
    ListValStr,
    ListValInt,
    ListValDict,
    ListValList,
    ListFlush,
    RootValInt,
    RootValStr,
    RootValDict,
    RootValList,
    Done,
}

#[derive(Debug, PartialEq)]
enum TraverseState {
    Root,
    Dict,
    List,
    Done,
}

#[derive(Debug)]
pub enum Error {
    Io(IoError),
    Empty,
    Syntax(usize, String),
    Eof,
    StackUnderflow,
    UnexpectedState,
    BigInt,
}

#[derive(Debug)]
pub enum SelectError {
    Key(String, String),
    Index(String, usize),
    Subscriptable(String),
    Indexable(String),
    Primitive(String),
    Syntax(String, usize, String),
    End,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Value {
    Int(i64),
    Str(String),
    Bytes(Vec<u8>),
    Dict(BTreeMap<Value, Value>),
    List(Vec<Value>),
    DictRef(Rc<RefCell<BTreeMap<Value, Value>>>),
    ListRef(Rc<RefCell<Vec<Value>>>),
}

pub struct ValueDisplay<'a>(&'a Value, usize);

pub fn load(stream: &mut (impl Read + Seek)) -> Result<Value, Error> {
    let file_size = stream.seek(SeekFrom::End(0))?;
    stream.seek(SeekFrom::Start(0))?;

    if file_size == 0 {
        return Err(Error::Empty);
    }

    #[cfg(test)] eprintln!("File size: {}", file_size);

    let mut file_index = 0u64;
    let mut buf_index = 0usize;
    let mut state = State::Root;
    let mut next_state = Vec::new();
    let mut buf = Vec::new();
    let mut buf_chars = buf.iter().peekable();
    let mut buf_str = Vec::new();
    let mut buf_str_remainder = 0u64;
    let mut buf_int = String::new();
    let mut key_stack = Vec::new();
    let mut val_stack = Vec::new();
    let mut item_stack = Vec::new();
    let mut dict_stack = Vec::new();
    let mut list_stack = Vec::new();
    let mut dict_i = -1i8;
    let mut list_i = -1i8;
    let root;

    while file_index + (buf_index as u64) < file_size {
        let real_index = file_index + buf_index as u64;

        if real_index >= (file_index + buf.len() as u64) && real_index < file_size {
            buf.clear();
            stream.take(CHUNK_SIZE).read_to_end(&mut buf)?;
            buf_chars = buf.iter().peekable();
            file_index += buf_index as u64;
            buf_index = 0;
        }

        #[cfg(test)] {
            eprintln!("------------------------");
            eprintln!("real_index: {:?}", real_index);
            eprintln!("state: {:?}", state);
            eprintln!("dict_i: {}", dict_i);
            eprintln!("list_i: {}", list_i);
            eprintln!("------------------------");
        }

        match state {
            State::Root => {
                let c = **buf_chars.peek().unwrap();
                #[cfg(test)]
                eprintln!("c = {}", c);

                match c.try_into() {
                    // Dict value
                    Ok(Token::Dict) => {
                        buf_chars.next();
                        buf_index += 1;
                        dict_stack.push(Rc::new(RefCell::new(BTreeMap::new())));
                        key_stack.push(None);
                        val_stack.push(None);
                        dict_i += 1;

                        state = State::DictKey;
                        next_state.push(State::RootValDict);
                    }

                    // List value
                    Ok(Token::List) => {
                        buf_chars.next();
                        buf_index += 1;
                        list_stack.push(Rc::new(RefCell::new(Vec::new())));
                        item_stack.push(None);
                        list_i += 1;

                        state = State::ListVal;
                        next_state.push(State::RootValList);
                    }

                    // Int value
                    Ok(Token::Int) => {
                        state = State::Int;
                        buf_chars.next();
                        buf_index += 1;
                        next_state.push(State::RootValInt);
                    }


                    // Str value
                    Err(_) => {
                        state = State::Str;
                        next_state.push(State::RootValStr);
                    }

                    // End, Colon
                    Ok(a) => return Err(
                        Error::Syntax(real_index as usize,
                                      format!("Unexpected '{}' token", Into::<u8>::into(a) as char))
                    ),
                }
            }

            // Root int value
            // Just increase buf_index here so the loop can be broken
            State::RootValInt => {
                buf_index += 1;
            }

            // Read dict key or end the dict if it's empty
            // Internally dict keys can be anything since BTreeMap's K type is Value, but here we only
            // consider them to be strings.
            // FIXME: Deny non-string tokens?
            State::DictKey => {
                let c = **buf_chars.peek().unwrap();

                if c == Token::End.into() {
                    buf_chars.next();
                    buf_index += 1;
                    state = next_state.pop().ok_or(Error::StackUnderflow)?;
                } else {
                    if buf_str.len() == 0 {
                        state = State::Str;
                        next_state.push(State::DictKey);
                    } else {
                        key_stack[dict_i as usize] = Some(str_or_bytes(buf_str.clone()));
                        buf_str.clear();
                        state = State::DictVal;
                    }
                }
            }

            // Read dict value
            State::DictVal => {
                let c = **buf_chars.peek().ok_or(Error::Eof)?;

                match c.try_into() {
                    // Dict value
                    Ok(Token::Dict) => {
                        let map = Rc::new(RefCell::new(BTreeMap::new()));

                        buf_chars.next();
                        buf_index += 1;
                        val_stack[dict_i as usize] = Some(Value::DictRef(Rc::clone(&map)));
                        dict_stack.push(map);
                        key_stack.push(None);
                        val_stack.push(None);
                        dict_i += 1;

                        state = State::DictKey;
                        next_state.push(State::DictValDict);
                    }

                    // List value
                    Ok(Token::List) => {
                        let vec = Rc::new(RefCell::new(Vec::new()));

                        buf_chars.next();
                        buf_index += 1;
                        val_stack[dict_i as usize] = Some(Value::ListRef(Rc::clone(&vec)));
                        list_stack.push(vec);
                        item_stack.push(None);
                        list_i += 1;

                        state = State::ListVal;
                        next_state.push(State::DictValList);
                    }

                    // Int value
                    Ok(Token::Int) => {
                        buf_chars.next();
                        buf_index += 1;
                        state = State::Int;
                        next_state.push(State::DictValInt);
                    }

                    // String value
                    Err(_) => {
                        state = State::Str;
                        next_state.push(State::DictValStr);
                    }

                    // Colon, End
                    _ => return Err(Error::Syntax(real_index as usize, format!("Unexpected '{}' token", c))),
                }
            }

            // Process current dict value as str
            State::DictValStr => {
                val_stack[dict_i as usize] = Some(str_or_bytes(buf_str.clone()));
                buf_str.clear();
                state = State::DictFlush;
            }

            // Process current dict value as int
            State::DictValInt => {
                // Unwrap here because Int state already checks for EOF
                let c = *buf_chars.next().unwrap();

                if c != Token::End.into() {
                    return Err(Error::Syntax(real_index as usize, "Expected 'e' token".into()));
                }

                let val = buf_int.parse::<i64>().map_err(|_| Error::Syntax(real_index as usize, "Invalid integer".into()))?;
                val_stack[dict_i as usize] = Some(Value::Int(val));
                buf_int.clear();
                buf_index += 1;

                state = State::DictFlush;
            }

            // Process current dict value as dict
            State::DictValDict => {
                let dict = dict_stack.pop().ok_or(Error::StackUnderflow)?;

                val_stack[dict_i as usize] = Some(Value::DictRef(dict));
                dict_i -= 1;
                key_stack.pop().ok_or(Error::StackUnderflow)?;
                val_stack.pop().ok_or(Error::StackUnderflow)?;
                state = State::DictFlush;
            }

            // Process current dict value as list
            State::DictValList => {
                let list = list_stack.pop().ok_or(Error::StackUnderflow)?;

                val_stack[dict_i as usize] = Some(Value::ListRef(list));
                list_i -= 1;
                item_stack.pop().ok_or(Error::StackUnderflow)?;
                state = State::DictFlush;
            }

            // Insert current (key, value) pair into current dict
            State::DictFlush => {
                let key = key_stack[dict_i as usize].clone().unwrap();
                let val = val_stack[dict_i as usize].clone().unwrap().unref();
                dict_stack[dict_i as usize].borrow_mut().insert(key, val);

                let c = **buf_chars.peek().ok_or(Error::Eof)?;

                if c == Token::End.into() {
                    buf_chars.next();
                    buf_index += 1;
                    state = next_state.pop().ok_or(Error::StackUnderflow)?;
                } else {
                    state = State::DictKey;
                }
            }

            // List value
            State::ListVal => {
                let c = **buf_chars.peek().ok_or(Error::Eof)?;

                match c.try_into() {
                    // End of list
                    Ok(Token::End) => {
                        buf_chars.next();
                        buf_index += 1;
                        state = next_state.pop().ok_or(Error::StackUnderflow)?;
                    }

                    // Dict value
                    Ok(Token::Dict) => {
                        let d = Rc::new(RefCell::new(BTreeMap::new()));

                        item_stack[list_i as usize] = Some(Value::DictRef(Rc::clone(&d)));
                        buf_chars.next();
                        dict_stack.push(d);
                        key_stack.push(None);
                        val_stack.push(None);
                        dict_i += 1;
                        buf_index += 1;

                        state = State::DictKey;
                        next_state.push(State::ListValDict);
                    }

                    // List value
                    Ok(Token::List) => {
                        let l = Rc::new(RefCell::new(Vec::new()));

                        item_stack[list_i as usize] = Some(Value::ListRef(Rc::clone(&l)));
                        buf_chars.next();
                        list_stack.push(l);
                        item_stack.push(None);
                        list_i += 1;
                        buf_index += 1;

                        next_state.push(State::ListValList);
                    }

                    // Int value
                    Ok(Token::Int) => {
                        buf_chars.next();
                        buf_index += 1;
                        state = State::Int;
                        next_state.push(State::ListValInt);
                    }

                    // String value
                    Err(_) => {
                        state = State::Str;
                        next_state.push(State::ListValStr);
                    }

                    // Colon
                    _ => return Err(Error::Syntax(real_index as usize, "Unexpected ':' token".into())),
                }
            }

            // Process current list value as str
            State::ListValStr => {
                item_stack[list_i as usize] = Some(str_or_bytes(buf_str.clone()));
                buf_str.clear();
                state = State::ListFlush;
            }

            // Process current list value as int
            State::ListValInt => {
                // Unwrap here because Int state already checks for EOF
                let c = *buf_chars.next().unwrap();

                if c != Token::End.into() {
                    return Err(Error::Syntax(real_index as usize, "Expected 'e' token".into()));
                }

                let val = buf_int.parse::<i64>().map_err(|_| Error::Syntax(real_index as usize, "Invalid integer".into()))?;

                item_stack[list_i as usize] = Some(Value::Int(val));
                buf_int.clear();
                buf_index += 1;
                state = State::ListFlush;
            }

            // Process current list value as dict
            State::ListValDict => {
                let dict = dict_stack.pop().ok_or(Error::StackUnderflow)?.borrow().clone();

                item_stack[list_i as usize] = Some(Value::Dict(dict));
                key_stack.pop();
                val_stack.pop();
                dict_i -= 1;

                state = State::ListFlush;
            }

            // Process current list value as list
            State::ListValList => {
                let list = list_stack.pop().ok_or(Error::StackUnderflow)?.borrow().clone();

                item_stack[list_i as usize] = Some(Value::List(list));
                item_stack.pop();
                list_i -= 1;

                state = State::ListFlush;
            }

            // Add current list value to the current list
            State::ListFlush => {
                let val = item_stack[list_i as usize].clone().unwrap().unref();
                list_stack[list_i as usize].borrow_mut().push(val);

                let c = **buf_chars.peek().unwrap();

                if c == Token::End.into() {
                    buf_chars.next();
                    buf_index += 1;
                    state = next_state.pop().ok_or(Error::StackUnderflow)?;
                } else {
                    state = State::ListVal;
                }
            }

            // Process string
            State::Str => {
                if buf_int.len() == 0 {
                    buf_str.clear();
                    buf_str_remainder = 0;
                    state = State::Int;
                    next_state.push(State::Str);
                } else {
                    let c = *buf_chars.next().ok_or(Error::Eof)?;
                    #[cfg(test)] eprintln!("c = {}", c);

                    if c != Token::Colon.into() {
                        return Err(Error::Syntax(real_index as usize, "Expected ':'".into()));
                    }

                    let buf_str_size = buf_int.parse::<u64>().map_err(|_| Error::Syntax(real_index as usize, "Invalid integer".into()))?;
                    buf_int.clear();
                    buf_index += 1;

                    // String is bigger than buffer
                    if buf_index + buf_str_size as usize > buf.len() {
                        let chunk_size = buf.len() - buf_index;
                        buf_str_remainder = buf_str_size - chunk_size as u64;
                        buf_str.extend(buf_chars.by_ref());
                        buf_index += chunk_size;
                        state = State::StrRem;
                    } else {
                        buf_str.extend(buf_chars.by_ref().take(buf_str_size as usize));
                        buf_index += buf_str_size as usize;
                        state = next_state.pop().ok_or(Error::StackUnderflow)?;
                    }
                }
            }

            // Process string remainder
            State::StrRem => {
                if buf_str_remainder > 0 && buf_index + buf_str_remainder as usize > buf.len() {
                    let chunk_size = buf.len() - buf_index;
                    buf_str_remainder -= chunk_size as u64;
                    buf_str.extend(buf_chars.by_ref());
                    buf_index += chunk_size;
                } else {
                    buf_str.extend(buf_chars.by_ref().take(buf_str_remainder as usize));
                    buf_index += buf_str_remainder as usize;
                    buf_str_remainder = 0;
                    state = next_state.pop().ok_or(Error::StackUnderflow)?;
                }
            }

            // Int
            State::Int => {
                const CHARS: &[char] = &['0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '-'];

                let c = **buf_chars.peek().ok_or(Error::Eof)? as char;
                #[cfg(test)] eprintln!("(int) c = {}", c);

                if CHARS.contains(&c) {
                    // Only allow minus at the beginning
                    if c == '-' && buf_int.len() > 0 {
                        return Err(Error::Syntax(real_index as usize, "Unexpected '-'".into()));
                    }

                    buf_int.push(c);
                    buf_chars.next();
                    buf_index += 1;
                } else {
                    if buf_int.len() == 0 {
                        return Err(Error::Syntax(real_index as usize, "Empty integer".into()));
                    }

                    if buf_int.len() > MAX_INT_BUF {
                        return Err(Error::BigInt);
                    }

                    state = next_state.pop().ok_or(Error::StackUnderflow)?;
                }
            }

            _ => return Err(Error::UnexpectedState),
        }
    }

    if next_state.len() > 0 {
        return Err(Error::Eof);
    }

    match state {
        State::RootValInt => {
            // Unwrap here because Int state already checks for EOF
            let c = *buf_chars.next().unwrap();

            if c != Token::End.into() {
                return Err(Error::Syntax(file_size as usize - 1, "Expected 'e' token".into()));
            }

            let val = buf_int.parse::<i64>()
                .map_err(|_| Error::Syntax(file_index as usize + buf_index,
                                           "Invalid integer".into()))?;
            root = Some(Value::Int(val));
        }

        State::RootValStr => root = Some(str_or_bytes(buf_str)),

        State::RootValDict => {
            let dict = dict_stack.pop().ok_or(Error::StackUnderflow)?.borrow().clone();

            root = Some(Value::Dict(dict));
        }

        State::RootValList => {
            let list = list_stack.pop().ok_or(Error::StackUnderflow)?.borrow().clone();

            root = Some(Value::List(list));
        }

        _ => return Err(Error::UnexpectedState),
    }

    Ok(root.unwrap())
}

pub fn load_str(s: &str) -> Result<Value, Error> {
    let mut cursor = Cursor::new(s);
    load(&mut cursor)
}

/// Try to convert a raw buffer to utf8 and return the appropriate Value
fn str_or_bytes(vec: Vec<u8>) -> Value {
    match String::from_utf8(vec) {
        Ok(s) => Value::Str(s),
        Err(e) => Value::Bytes(e.into_bytes()),
    }
}

fn repr_bytes(bytes: &[u8], truncate_at: usize) -> String {
    let mut buf = String::from("b\"");

    for b in bytes.iter().take(truncate_at) {
        if (32..128).contains(b) {
            buf.push(*b as char);
        } else {
            buf.extend(format!("\\x{:02X}", b).chars());
        }
    }

    buf.push('"');

    if bytes.len() > truncate_at {
        buf.push_str("... (");
        buf.extend(format!("{} bytes)", bytes.len()).chars());
    }

    buf
}

impl Value {
    /// Transforms possible references (Dict/ListRef) into owned values
    pub fn unref(self) -> Value {
        match self {
            Value::DictRef(rc) => Value::Dict(rc.borrow().clone()),
            Value::ListRef(rc) => Value::List(rc.borrow().clone()),
            a => a,
        }
    }

    /// Is the current value a reference?
    pub fn is_ref(&self) -> bool {
        matches!(self, Value::DictRef(_) | Value::ListRef(_))
    }

    /// Is the current value an int?
    pub fn is_int(&self) -> bool {
        matches!(self, Value::Int(_))
    }

    /// Is the current value a str?
    pub fn is_str(&self) -> bool {
        matches!(self, Value::Str(_))
    }

    /// Is the current value a bytes?
    pub fn is_bytes(&self) -> bool {
        matches!(self, Value::Bytes(_))
    }

    /// Is the current value a dict?
    pub fn is_dict(&self) -> bool {
        matches!(self, Value::Dict(_))
    }

    /// Is the current value a list?
    pub fn is_list(&self) -> bool {
        matches!(self, Value::List(_))
    }

    pub fn to_i64(&self) -> Option<i64> {
        if let Value::Int(v) = self {
            Some(*v)
        } else {
            None
        }
    }

    pub fn to_str(&self) -> Option<&str> {
        if let Value::Str(s) = self {
            Some(s.as_str())
        } else {
            None
        }
    }

    pub fn to_bytes(&self) -> Option<&[u8]> {
        if let Value::Bytes(v) = self {
            Some(v.as_slice())
        } else {
            None
        }
    }

    pub fn to_map(&self) -> Option<&BTreeMap<Value, Value>> {
        if let Value::Dict(map) = self {
            Some(map)
        } else {
            None
        }
    }

    pub fn to_vec(&self) -> Option<&Vec<Value>> {
        if let Value::List(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub fn value_type(value: &Self) -> &'static str {
        match value {
            Self::Int(_) => "int",
            Self::Str(_) => "str",
            Self::Bytes(_) => "bytes",
            Self::Dict(_) => "dict",
            Self::List(_) => "list",
            Self::DictRef(_) => "&dict",
            Self::ListRef(_) => "&list",
        }
    }

    /// Select a value inside this one if it is a container (dict or list).
    ///
    /// # Syntax
    ///
    /// - Selecting with key: `.keyname`. `keyname` can be be anything and also can contain spaces, but
    /// if it has dots or an open bracket (`[`), put a `\\` before them.
    /// - Selecting with an index: `[n]`, where N is in `[0; n)`.
    ///
    /// An empty selector will return this Value.
    ///
    /// ## Examples
    ///
    /// - `.bar`: Selects key `bar` in the root.
    /// - `.buz.fghij[1]`: Selects key `buz` (a dict) in the root, then key `fghij` (a list),
    /// then index number 1.
    ///
    /// # Errors
    ///
    /// - `SelectError::Primitive(context)`: This Value is not a container.
    /// - `SelectError::Indexable(context)`: The current Value is not indexable (is a dict).
    /// - `SelectError::Subscriptable(context)`: The current Value is not subscriptable (is a list).
    /// - `SelectError::Key(context, key)`: The current dict does not have key `key`.
    /// - `SelectError::Index(context, number)`: `number` is out of bounds for the current list.
    /// - `SelectError::Syntax(where, why)`: There was an error parsing the selector string.
    /// - `SelectError::End`: Reached end of selector while trying to parse a key or index
    ///
    /// `context` will contain the selector up until where the error occurred.
    pub fn select(&self, mut selector: &str) -> Result<&Value, SelectError> {
        if !self.is_dict() && !self.is_list() && !self.is_ref() {
            return Err(SelectError::Primitive("<root>".into()));
        }

        if selector.is_empty() {
            return Ok(self);
        }

        let full_selector = &selector[..];
        let mut state = TraverseState::Root;
        let mut current_dict = None;
        let mut current_list = None;
        let mut value = self;

        macro_rules! context {
            () => {{
                let c = &full_selector[..context!(pos)];

                if !c.is_empty() {
                    c.into()
                } else {
                    "<root>".into()
                }
            }};

            (pos) => {
                full_selector.len() - selector.len() + 1;
            };
        }

        while state != TraverseState::Done {
            match state {
                TraverseState::Root => {
                    if let Some(m) = value.to_map() {
                        state = TraverseState::Dict;
                        current_dict = Some(m.iter());
                    } else if let Some(v) = value.to_vec() {
                        state = TraverseState::List;
                        current_list = Some(v.iter());
                    }
                }

                TraverseState::Dict => {
                    if selector.chars().next().unwrap() == '[' {
                        return Err(SelectError::Indexable(context!()));
                    }

                    let (rest, key) = Self::parse_key_selector(selector, full_selector)?;
                    let val = current_dict.take().unwrap().find_map(|(k, v)| {
                        k.to_str().and_then(|k| if k == key {
                            Some(v)
                        } else {
                            None
                        })
                    });
                    selector = rest;

                    if let Some(val) = val {
                        if selector.is_empty() {
                            value = val;
                            state = TraverseState::Done;
                        } else if let Some(m) = val.to_map() {
                            current_dict = Some(m.iter());
                        } else if let Some(v) = val.to_vec() {
                            current_list = Some(v.iter());
                            state = TraverseState::List;
                        } else {
                            return Err(SelectError::Primitive(context!()));
                        }
                    } else {
                        return Err(SelectError::Key(context!(), key.into()));
                    }
                }

                TraverseState::List => {
                    if selector.chars().next().unwrap()  == '.' {
                        return Err(SelectError::Subscriptable(context!()));
                    }

                    let (rest, index) = Self::parse_index_selector(selector, full_selector)?;
                    selector = rest;

                    if let Some(val) = current_list.take().unwrap().nth(index) {
                        if selector.is_empty() {
                            value = val;
                            state = TraverseState::Done;
                        } else if let Some(m) = val.to_map() {
                            current_dict = Some(m.iter());
                            state = TraverseState::Dict;
                        } else if let Some(v) = val.to_vec() {
                            current_list = Some(v.iter());
                        } else {
                            return Err(SelectError::Primitive(context!()));
                        }
                    } else {
                        return Err(SelectError::Index(context!(), index));
                    }
                }

                // Done
                _ => unreachable!(),
            }
        }

        Ok(value)
    }

    fn parse_key_selector<'a>(input: &'a str, full_input: &str) -> Result<(&'a str, String), SelectError> {
        const END_CHARS: &[char] = &['.', '['];

        let mut buf = String::new();
        let mut escaped = false;
        let pos = || full_input.len() - input.len() + 1;
        let context = || {
            let c = &full_input[..pos()];

            if !c.is_empty() {
                c.into()
            } else {
                "<root>".into()
            }
        };
        let mut chars = input.chars();

        if chars.next().unwrap() != '.' {
            return Err(SelectError::Syntax(context(), pos(), "Expected dot".into()));
        }

        let mut input = &input[1..];

        for (i, c) in chars.enumerate() {
            #[cfg(test)] eprintln!("i,c: {}, {}", i, c);

            if END_CHARS.contains(&c) {
                if escaped {
                    buf.push(c);
                    input = &input[(i + 1)..];
                    escaped = false;
                } else {
                    buf.push_str(&input[..i]);
                    input = &input[i..];
                    #[cfg(test)] eprintln!("last_input: {}", input);
                    break;
                }
            } else if c == '\\' {
                if escaped {
                    buf.push(c);
                    input = &input[(i + 1)..];
                    escaped = false;
                } else {
                    buf.push_str(&input[..i]);
                    input = &input[(i + 1)..];
                    escaped = true;
                }
            } else if escaped {
                return Err(SelectError::Syntax(context(), pos(), format!("Cannot escape '{}'", c)));
            }
        }

        if escaped {
            return Err(SelectError::Syntax(context(), pos(), "Trailing escape".into()));
        }

        if buf.is_empty() && !input.is_empty() {
            buf.push_str(input);
            input = &input[input.len()..];
        }

        Ok((input, buf))
    }

    fn parse_index_selector<'a>(input: &'a str, full_input: &str) -> Result<(&'a str, usize), SelectError> {
        let pos = || full_input.len() - input.len() + 1;
        let context = || {
            let c = &full_input[..pos()];

            if !c.is_empty() {
                c.into()
            } else {
                "<root>".into()
            }
        };

        let mut chars = input.chars();
        let mut index = None;
        let first_char = chars.next().unwrap();

        if first_char != '[' {
            return Err(SelectError::Syntax(context(), pos(), "Expected '['".into()));
        }

        let mut input = &input[1..];

        for (i, c) in chars.enumerate() {
            if c == ']' {
                let index_ = input[..i].parse::<usize>().map_err(|_| {
                    SelectError::Syntax(context(), pos(), "Not a number".into())
                })?;
                input = &input[(i + 1)..];
                index = Some(index_);
                break;
            } else if c == '-' {
                return Err(SelectError::Syntax(context(), pos(), "Unexpected '-'".into()));
            }
        }

        if index.is_none() {
            return Err(SelectError::End);
        }

        Ok((input, index.unwrap()))
    }
}

impl<'a> ValueDisplay<'a> {
    pub fn new(root: &'a Value, indent_size: usize) -> Self {
        Self(root, indent_size)
    }

    pub fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        const RUN_LIMIT: usize = 2000;
        const STACK_LIMIT: usize = 5;

        let mut runs = 0;
        let mut indent = 0;
        let mut state = TraverseState::Root;
        let mut next_state = Vec::new();
        let mut dict_stack = Vec::new();
        let mut list_stack = Vec::new();
        let mut stack_count = 0;
        let ValueDisplay(root, indent_size) = self;


        while state != TraverseState::Done {
            match state {
                TraverseState::Root => {
                    if let Some(i) = root.to_i64() {
                        write!(f, "{}", i)?;
                        state = TraverseState::Done;
                    } else if let Some(s) = root.to_str() {
                        write!(f, "{:?}", s)?;
                        state = TraverseState::Done;
                    } else if let Some(b) = root.to_bytes() {
                        write!(f, "{}", repr_bytes(b, 32))?;
                        state = TraverseState::Done;
                    } else if let Some(m) = root.to_map() {
                        write!(f, "{{\n")?;
                        dict_stack.push(m.iter().peekable());
                        write!(f, "{:indent$}", "", indent = indent * indent_size)?;
                        indent += 1;
                        state = TraverseState::Dict;
                        next_state.push(TraverseState::Done);
                    } else if let Some(v) = root.to_vec() {
                        write!(f, "[")?;
                        list_stack.push(v.iter().peekable());
                        state = TraverseState::List;
                        next_state.push(TraverseState::Done);
                    } else {
                        return Err(fmt::Error);
                    }
                }

                TraverseState::Dict => {
                    let it = dict_stack.last_mut().unwrap();
                    let next = it.next();

                    if let Some((key, val)) = next {
                        write!(f, "{:indent$}", "", indent = indent * indent_size)?;
                        write!(f, "{}: ", key)?;

                        if let Some(i) = val.to_i64() {
                            write!(f, "{}", i)?;
                            write!(f, ",\n")?;
                        } else if let Some(s) = val.to_str() {
                            write!(f, "{:?}", s)?;
                            write!(f, ",\n")?;
                        } else if let Some(b) = val.to_bytes() {
                            write!(f, "{}", repr_bytes(b, 8))?;
                            write!(f, ",\n")?;
                        } else if let Some(m) = val.to_map() {
                            if m.is_empty() {
                                write!(f, "{{}},\n")?;
                            } else if stack_count < STACK_LIMIT {
                                write!(f, "{{\n")?;
                                dict_stack.push(m.iter().peekable());
                                indent += 1;
                                next_state.push(TraverseState::Dict);
                                stack_count += 1;
                            } else {
                                write!(f, "{{...}},\n")?;
                            }
                        } else if let Some(v) = val.to_vec() {
                            if v.is_empty() {
                                write!(f, "[],\n")?;
                            } else if stack_count < STACK_LIMIT {
                                write!(f, "[")?;
                                list_stack.push(v.iter().peekable());
                                state = TraverseState::List;
                                next_state.push(TraverseState::Dict);
                                stack_count += 1;
                            } else {
                                write!(f, "[...],\n")?;
                            }
                        } else {
                            return Err(fmt::Error);
                        }

                    } else {
                        indent -= 1;
                        write!(f, "{:indent$}", "", indent = indent * indent_size)?;
                        write!(f, "}}")?;
                        let _ = dict_stack.pop().ok_or(fmt::Error)?;
                        state = next_state.pop().ok_or(fmt::Error)?;

                        if state == TraverseState::Dict {
                            write!(f, ",\n")?;
                        }
                    }
                }

                TraverseState::List => {
                    let it = list_stack.last_mut().unwrap();
                    let next = it.next();
                    let is_last = it.peek().is_none();

                    if let Some(val) = next {
                        if let Some(i) = val.to_i64() {
                            write!(f, "{}", i)?;
                            if !is_last { write!(f, ", ")? };
                        } else if let Some(s) = val.to_str() {
                            write!(f, "{:?}", s)?;
                            if !is_last { write!(f, ", ")? };
                        } else if let Some(b) = val.to_bytes() {
                            write!(f, "{}", repr_bytes(b, 8))?;
                            if !is_last { write!(f, ", ")? };
                        } else if let Some(m) = val.to_map() {
                            if m.is_empty() {
                                write!(f, "{{}}")?;
                            } else if stack_count < STACK_LIMIT {
                                write!(f, "{{\n")?;
                                dict_stack.push(m.iter().peekable());
                                indent += 1;
                                state = TraverseState::Dict;
                                next_state.push(TraverseState::List);
                                stack_count += 1;
                            } else {
                                write!(f, "{{...}}")?;
                            }
                        } else if let Some(v) = val.to_vec() {
                            if v.is_empty() {
                                write!(f, "[]")?;
                            } else if stack_count < STACK_LIMIT {
                                write!(f, "[")?;
                                list_stack.push(v.iter().peekable());
                                next_state.push(TraverseState::List);
                                stack_count += 1;
                            } else {
                                write!(f, "[...]")?;
                            }
                        } else {
                            return Err(fmt::Error);
                        }
                    } else {
                        write!(f, "]")?;
                        let _ = list_stack.pop().ok_or(fmt::Error)?;
                        state = next_state.pop().ok_or(fmt::Error)?;

                        if state == TraverseState::Dict {
                            write!(f, ",\n")?;
                        } else if state == TraverseState::List {
                            let count = list_stack.last().unwrap().clone().count();

                            if count > 0 {
                                write!(f, ", ")?;
                            }
                        }
                    }
                }

                // Done
                _ => unreachable!(),
            }

            runs += 1;

            if runs == RUN_LIMIT {
                write!(f, "\n<truncating as output would be too big...>")?;
                break;
            }
        }

        Ok(())
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        ValueDisplay::new(self, 2).fmt(f)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "IO Error: {}", e),
            Error::Empty => write!(f, "Empty file"),
            Error::Syntax(n, s) => write!(f, "Syntax error at {}: {}", n + 1, s),
            Error::Eof => write!(f, "Unexpected end of file reached"),
            Error::StackUnderflow => write!(f, "Stack underflow"),
            Error::UnexpectedState => write!(f, "Unexpected state in main loop"),
            Error::BigInt => write!(f, "Integer too big"),
        }
    }
}

impl fmt::Display for SelectError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SelectError::Key(ctx, key) => write!(f, "[{:?}] Unknown key '{}'", ctx, key),
            SelectError::Index(ctx, index) => write!(f, "[{:?}] Index {} out of bounds", ctx, index),
            SelectError::Subscriptable(ctx) => write!(f, "[{:?}] Current value cannot be subscripted", ctx),
            SelectError::Indexable(ctx) => write!(f, "[{:?}] Current value cannot be indexed", ctx),
            SelectError::Primitive(ctx) => write!(f, "[{:?}] Current value is not selectable!", ctx),
            SelectError::Syntax(ctx, pos, msg) => write!(f, "[{:?}] Syntax error at {}: {}", ctx, pos, msg),
            SelectError::End => write!(f, "Reached end of selector prematurely"),
        }
    }
}

impl From<IoError> for Error {
    fn from(e: IoError) -> Self {
        Self::Io(e)
    }
}

impl Into<u8> for Token {
    fn into(self) -> u8 {
        match self {
            Self::Dict => 'd' as u8,
            Self::Int => 'i' as u8,
            Self::List => 'l' as u8,
            Self::Colon => ':' as u8,
            Self::End => 'e' as u8,
        }
    }
}

impl TryFrom<u8> for Token {
    type Error = ();

    fn try_from(c: u8) ->  Result<Token, Self::Error> {
        const D: u8 = 'd' as u8;
        const I: u8 = 'i' as u8;
        const L: u8 = 'l' as u8;
        const C: u8 = ':' as u8;
        const E: u8 = 'e' as u8;

        match c {
            D => Ok(Token::Dict),
            I => Ok(Token::Int),
            L => Ok(Token::List),
            C => Ok(Token::Colon),
            E => Ok(Token::End),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BTreeMap, Value};

    const DICT_VAL_INT: &'static str = "d3:fooi0e3:bari1e3:bazi2ee";
    const LIST_VAL_STR: &'static str = "l3:foo3:bar3:baze";
    const LIST_VAL_INT: &'static str = "li0ei1ei2ee";
    const LIST_NESTED: &'static str = "lli0ei1ei2eeli3ei4ei5eeli6ei7ei8eee";
    const DICT_MIXED: &'static str = "d3:fooi0e3:bari1e3:bazi2e3:buzd3:boz3:bez\
5:abcde5:fghij5:fghijl6:klmnop6:qrstuvd4:wxyzi0eeee3:zyxli0ei1ei2eee";

    fn check_value(source: &'static str, value: Value) {
        match super::load_str(source) {
            Ok(v) => assert_eq!(v, value),
            Err(e) => panic!("Got {:?}", e),
        }
    }

    #[test]
    fn load_primitive_int() {
        check_value("i123456e", Value::Int(123456));
    }

    #[test]
    fn load_primitive_str() {
        check_value("6:foobar", Value::Str("foobar".into()));
    }

    #[test]
    fn load_dict_val_int() {
        let mut map = BTreeMap::new();
        map.insert(Value::Str("foo".into()), Value::Int(0));
        map.insert(Value::Str("bar".into()), Value::Int(1));
        map.insert(Value::Str("baz".into()), Value::Int(2));

        check_value(DICT_VAL_INT, Value::Dict(map));
    }

    #[test]
    fn load_list_val_str() {
        let list = Value::List(vec![
            Value::Str("foo".into()),
            Value::Str("bar".into()),
            Value::Str("baz".into())
        ]);

        check_value(LIST_VAL_STR, list);
    }

    #[test]
    fn load_list_val_int() {
        let list = Value::List(vec![
            Value::Int(0),
            Value::Int(1),
            Value::Int(2),
        ]);

        check_value(LIST_VAL_INT, list);
    }

    #[test]
    fn load_list_nested() {
        let list_1 = Value::List(vec![Value::Int(0), Value::Int(1), Value::Int(2)]);
        let list_2 = Value::List(vec![Value::Int(3), Value::Int(4), Value::Int(5)]);
        let list_3 = Value::List(vec![Value::Int(6), Value::Int(7), Value::Int(8)]);
        let list = Value::List(vec![list_1, list_2, list_3]);

        check_value(LIST_NESTED, list);
    }

    #[test]
    fn load_dict_mixed() {
        let mut root_map = BTreeMap::new();
        let mut buz_map = BTreeMap::new();
        let mut fghij_map = BTreeMap::new();

        fghij_map.insert(Value::Str("wxyz".into()), Value::Int(0));

        let fghij_list = Value::List(vec![
            Value::Str("klmnop".into()), Value::Str("qrstuv".into()), Value::Dict(fghij_map),
        ]);
        let zyx_list = Value::List(vec![Value::Int(0), Value::Int(1), Value::Int(2)]);

        buz_map.insert(Value::Str("abcde".into()), Value::Str("fghij".into()));
        buz_map.insert(Value::Str("boz".into()), Value::Str("bez".into()));
        buz_map.insert(Value::Str("fghij".into()), fghij_list);
        root_map.insert(Value::Str("foo".into()), Value::Int(0));
        root_map.insert(Value::Str("bar".into()), Value::Int(1));
        root_map.insert(Value::Str("baz".into()), Value::Int(2));
        root_map.insert(Value::Str("buz".into()), Value::Dict(buz_map));
        root_map.insert(Value::Str("zyx".into()), zyx_list);

        check_value(DICT_MIXED, Value::Dict(root_map));
    }

    #[test]
    fn select_dict_simple() {
        let mut map = BTreeMap::new();
        map.insert(Value::Str("foo".into()), Value::Int(0));
        map.insert(Value::Str("bar".into()), Value::Int(1));
        map.insert(Value::Str("baz".into()), Value::Int(2));
        let dict = Value::Dict(map);

        assert_eq!(dict.select(".foo").unwrap(), &Value::Int(0));
        assert_eq!(dict.select(".bar").unwrap(), &Value::Int(1));
        assert_eq!(dict.select(".baz").unwrap(), &Value::Int(2));
    }

    #[test]
    fn select_list_nested() {
        let list_1 = Value::List(vec![Value::Int(0), Value::Int(1), Value::Int(2)]);
        let list_2 = Value::List(vec![Value::Int(3), Value::Int(4), Value::Int(5)]);
        let list_3 = Value::List(vec![Value::Int(6), Value::Int(7), Value::Int(8)]);
        let list = Value::List(vec![list_1.clone(), list_2.clone(), list_3.clone()]);

        assert_eq!(list.select("[0]").unwrap(), &list_1);
        assert_eq!(list.select("[1]").unwrap(), &list_2);
        assert_eq!(list.select("[2]").unwrap(), &list_3);
        assert_eq!(list.select("[0][0]").unwrap(), &Value::Int(0));
        assert_eq!(list.select("[0][1]").unwrap(), &Value::Int(1));
        assert_eq!(list.select("[0][2]").unwrap(), &Value::Int(2));
        assert_eq!(list.select("[1][0]").unwrap(), &Value::Int(3));
        assert_eq!(list.select("[1][1]").unwrap(), &Value::Int(4));
        assert_eq!(list.select("[1][2]").unwrap(), &Value::Int(5));
        assert_eq!(list.select("[2][0]").unwrap(), &Value::Int(6));
        assert_eq!(list.select("[2][1]").unwrap(), &Value::Int(7));
        assert_eq!(list.select("[2][2]").unwrap(), &Value::Int(8));
    }

    #[test]
    fn select_dict_mixed() {
        let mut root_map = BTreeMap::new();
        let mut buz_map = BTreeMap::new();
        let mut fghij_map = BTreeMap::new();

        fghij_map.insert(Value::Str("wxyz".into()), Value::Int(0));

        let fghij_list = Value::List(vec![
            Value::Str("klmnop".into()), Value::Str("qrstuv".into()), Value::Dict(fghij_map.clone()),
        ]);
        let zyx_list = Value::List(vec![Value::Int(0), Value::Int(1), Value::Int(2)]);

        buz_map.insert(Value::Str("abcde".into()), Value::Str("fghij".into()));
        buz_map.insert(Value::Str("boz".into()), Value::Str("bez".into()));
        buz_map.insert(Value::Str("fghij".into()), fghij_list.clone());
        root_map.insert(Value::Str("foo".into()), Value::Int(0));
        root_map.insert(Value::Str("bar".into()), Value::Int(1));
        root_map.insert(Value::Str("baz".into()), Value::Int(2));
        root_map.insert(Value::Str("buz".into()), Value::Dict(buz_map.clone()));
        root_map.insert(Value::Str("zyx".into()), zyx_list);
        let dict = Value::Dict(root_map);

        assert_eq!(dict.select(".foo").unwrap(), &Value::Int(0));
        assert_eq!(dict.select(".bar").unwrap(), &Value::Int(1));
        assert_eq!(dict.select(".baz").unwrap(), &Value::Int(2));
        assert_eq!(dict.select(".buz").unwrap(), &Value::Dict(buz_map));
        assert_eq!(dict.select(".buz.abcde").unwrap(), &Value::Str("fghij".into()));
        assert_eq!(dict.select(".buz.boz").unwrap(), &Value::Str("bez".into()));
        assert_eq!(dict.select(".buz.fghij").unwrap(), &fghij_list);
        assert_eq!(dict.select(".buz.fghij[0]").unwrap(), &Value::Str("klmnop".into()));
        assert_eq!(dict.select(".buz.fghij[1]").unwrap(), &Value::Str("qrstuv".into()));
        assert_eq!(dict.select(".buz.fghij[2]").unwrap(), &Value::Dict(fghij_map));
        assert_eq!(dict.select(".buz.fghij[2].wxyz").unwrap(), &Value::Int(0));
    }
}
