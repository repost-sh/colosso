use std::fmt;
use std::io;

#[derive(Debug)]
pub struct Failure {
    pub subject: String,
    pub reason: String,
}

impl Failure {
    pub fn new(subject: impl fmt::Display, reason: impl fmt::Display) -> Self {
        Self {
            subject: subject.to_string(),
            reason: reason.to_string(),
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.subject, self.reason)
    }
}

impl From<io::Error> for Failure {
    fn from(failure: io::Error) -> Self {
        Self::new("output", failure)
    }
}
