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

#[derive(Debug)]
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Value {
    Int(i64),
    Str(String),
    Dict(BTreeMap<String, Value>),
    List(Vec<Value>),
    DictRef(Rc<RefCell<BTreeMap<String, Value>>>),
    ListRef(Rc<RefCell<Vec<Value>>>),
}

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
    let mut buf = String::new();
    let mut buf_chars = buf.chars().peekable();
    let mut buf_str = String::new();
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
            stream.take(CHUNK_SIZE).read_to_string(&mut buf)?;
            buf_chars = buf.chars().peekable();
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
                let c = *buf_chars.peek().unwrap();
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
                    Ok(a) => return Err(Error::Syntax(real_index as usize, format!("Unexpected '{}' token", Into::<char>::into(a)))),
                }
            }

            // Root int value
            // Just increase buf_index here so the loop can be broken
            State::RootValInt => {
                buf_index += 1;
            }

            // Read dict key
            State::DictKey => {
                if buf_str.len() == 0 {
                    state = State::Str;
                    next_state.push(State::DictKey);
                } else {
                    key_stack[dict_i as usize] = Some(buf_str.clone());
                    buf_str.clear();
                    state = State::DictVal;
                }
            }

            // Read dict value
            State::DictVal => {
                let c = *buf_chars.peek().ok_or(Error::Eof)?;

                match c.try_into() {
                    // End of dict
                    Ok(Token::End) => {
                        buf_chars.next();
                        buf_index += 1;
                        state = next_state.pop().ok_or(Error::StackUnderflow)?;
                    }

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

                    // Colon
                    _ => return Err(Error::Syntax(real_index as usize, "Unexpected ':' token".into())),
                }
            }

            // Process current dict value as str
            State::DictValStr => {
                val_stack[dict_i as usize] = Some(Value::Str(buf_str.clone()));
                buf_str.clear();
                state = State::DictFlush;
            }

            // Process current dict value as int
            State::DictValInt => {
                // Unwrap here because Int state already checks for EOF
                let c = buf_chars.next().unwrap();

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

                let c = *buf_chars.peek().ok_or(Error::Eof)?;

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
                let c = *buf_chars.peek().ok_or(Error::Eof)?;

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
                item_stack[list_i as usize] = Some(Value::Str(buf_str.clone()));
                buf_str.clear();
                state = State::ListFlush;
            }

            // Process current list value as int
            State::ListValInt => {
                // Unwrap here because Int state already checks for EOF
                let c = buf_chars.next().unwrap();

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

                let c = *buf_chars.peek().unwrap();

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
                    let c = buf_chars.next().ok_or(Error::Eof)?;
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

                let c = *buf_chars.peek().ok_or(Error::Eof)?;
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
            let c = buf_chars.next().unwrap();

            if c != Token::End.into() {
                return Err(Error::Syntax(file_size as usize - 1, "Expected 'e' token".into()));
            }

            let val = buf_int.parse::<i64>()
                .map_err(|_| Error::Syntax(file_index as usize + buf_index,
                                           "Invalid integer".into()))?;
            root = Some(Value::Int(val));
        }

        State::RootValStr => root = Some(Value::Str(buf_str)),

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

impl Value {
    /// Transforms possible references (Dict/ListRef) into owned values
    pub fn unref(self) -> Value {
        match self {
            Value::DictRef(rc) => Value::Dict(rc.borrow().clone()),
            Value::ListRef(rc) => Value::List(rc.borrow().clone()),
            a => a,
        }
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

impl From<IoError> for Error {
    fn from(e: IoError) -> Self {
        Self::Io(e)
    }
}

impl Into<char> for Token {
    fn into(self) -> char {
        match self {
            Self::Dict => 'd',
            Self::Int => 'i',
            Self::List => 'l',
            Self::Colon => ':',
            Self::End => 'e',
        }
    }
}

impl TryFrom<char> for Token {
    type Error = ();

    fn try_from(c: char) ->  Result<Token, Self::Error> {
        match c {
            'd' => Ok(Token::Dict),
            'i' => Ok(Token::Int),
            'l' => Ok(Token::List),
            ':' => Ok(Token::Colon),
            'e' => Ok(Token::End),
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
        map.insert("foo".into(), Value::Int(0));
        map.insert("bar".into(), Value::Int(1));
        map.insert("baz".into(), Value::Int(2));

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

        fghij_map.insert("wxyz".into(), Value::Int(0));

        let fghij_list = Value::List(vec![
            Value::Str("klmnop".into()), Value::Str("qrstuv".into()), Value::Dict(fghij_map),
        ]);
        let zyx_list = Value::List(vec![Value::Int(0), Value::Int(1), Value::Int(2)]);

        buz_map.insert("abcde".into(), Value::Str("fghij".into()));
        buz_map.insert("boz".into(), Value::Str("bez".into()));
        buz_map.insert("fghij".into(), fghij_list);
        root_map.insert("foo".into(), Value::Int(0));
        root_map.insert("bar".into(), Value::Int(1));
        root_map.insert("baz".into(), Value::Int(2));
        root_map.insert("buz".into(), Value::Dict(buz_map));
        root_map.insert("zyx".into(), zyx_list);

        check_value(DICT_MIXED, Value::Dict(root_map));
    }
}
