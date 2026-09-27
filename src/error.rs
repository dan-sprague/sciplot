use std::fmt;

/// `sciplot::Result<T>`.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Failures that come from the environment (I/O, GPU, window system), not from misuse.
#[non_exhaustive]
#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Encode(String),
    UnsupportedFormat(String),
    NoGpuAdapter(String),
    Gpu(String),
    /// Windows must be opened from the main thread (macOS requirement).
    NotMainThread,
    /// `show()` was called from inside a running window event loop.
    Reentrant,
    EventLoop(String),
    WorkerPanicked(String),
    Font(String),
    Parse(String),
    TooLarge(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::Encode(s) => write!(f, "encoding error: {s}"),
            Error::UnsupportedFormat(s) => {
                write!(f, "unsupported output format {s:?}; use .png or .svg")
            }
            Error::NoGpuAdapter(s) => write!(f, "no GPU adapter available: {s}"),
            Error::Gpu(s) => write!(f, "GPU error: {s}"),
            Error::NotMainThread => write!(
                f,
                "windows must be opened on the main thread; to run a simulation while showing a window use `fig.show_live(|live| ...)`"
            ),
            Error::Reentrant => write!(f, "show() called from inside a running window event loop"),
            Error::EventLoop(s) => write!(f, "window event loop error: {s}"),
            Error::WorkerPanicked(s) => write!(f, "the simulation closure panicked: {s}"),
            Error::Font(s) => write!(f, "font error: {s}"),
            Error::Parse(s) => write!(f, "{s}"),
            Error::TooLarge(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}
