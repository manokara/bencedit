use std::{
    fmt,
    io::Error as IoError,
    path::Path,
};

pub enum Error {
    Io(IoError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO Error: {}", e),
        }
    }
}

pub fn batch<P>(files: Vec<P>) -> Result<(), Error> where P: AsRef<Path> {
    Ok(())
}
