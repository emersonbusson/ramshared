#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UringError {
    TryAgain,
    BadFd,
    NoMem,
    NoDev,
    Other(i32),
}

impl std::fmt::Display for UringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TryAgain => write!(f, "EAGAIN: Try again"),
            Self::BadFd => write!(f, "EBADF: Bad file descriptor"),
            Self::NoMem => write!(f, "ENOMEM: Out of memory"),
            Self::NoDev => write!(f, "ENODEV: No such device"),
            Self::Other(c) => write!(f, "Unknown io_uring error: {}", c),
        }
    }
}
