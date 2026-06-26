use std::{error, fmt};

pub type Error = Box<dyn error::Error>;
pub type Result<T> = std::result::Result<T, Error>;

pub fn message(message: impl Into<String>) -> Error {
    Box::new(MessageError(message.into()))
}

#[derive(Debug)]
struct MessageError(String);

impl fmt::Display for MessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl error::Error for MessageError {}
