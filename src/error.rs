use std::{error, fmt};

pub type Error = Box<dyn error::Error>;
pub type Result<T> = std::result::Result<T, Error>;

pub fn message(message: impl Into<String>) -> Error {
    Box::new(MessageError(message.into()))
}

pub trait ResultExt<T> {
    fn context(self, context: impl Into<String>) -> Result<T>;
}

impl<T, E> ResultExt<T> for std::result::Result<T, E>
where
    E: error::Error + 'static,
{
    fn context(self, context: impl Into<String>) -> Result<T> {
        self.map_err(|source| {
            Box::new(ContextError {
                context: context.into(),
                source: Box::new(source),
            }) as Error
        })
    }
}

pub trait OptionExt<T> {
    fn context(self, context: impl Into<String>) -> Result<T>;
}

impl<T> OptionExt<T> for Option<T> {
    fn context(self, context: impl Into<String>) -> Result<T> {
        self.ok_or_else(|| message(context.into()))
    }
}

#[derive(Debug)]
struct MessageError(String);

impl fmt::Display for MessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl error::Error for MessageError {}

#[derive(Debug)]
struct ContextError {
    context: String,
    source: Error,
}

impl fmt::Display for ContextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.context, self.source)
    }
}

impl error::Error for ContextError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}
