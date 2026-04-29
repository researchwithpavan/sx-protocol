use crate::error::{SxError, SxErrorCode, SxResult};

/// Path segment in an SX path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SxPathSegment {
    Key(String),
    Index(usize),
}

/// Canonical path for SX values.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct SxPath {
    pub segments: Vec<SxPathSegment>,
}

impl SxPath {
    /// Creates an empty root path.
    pub fn root() -> Self {
        Self {
            segments: Vec::new(),
        }
    }

    /// Parses a slash-separated path such as `/user/name`.
    pub fn parse(input: &str) -> SxResult<Self> {
        if input.is_empty() || input == "/" {
            return Ok(Self::root());
        }
        if !input.starts_with('/') {
            return Err(SxError::new(
                SxErrorCode::InvalidPath,
                "path must start with '/'",
            ));
        }
        let mut out = Vec::new();
        for p in input.split('/').skip(1) {
            if p.is_empty() {
                continue;
            }
            if let Ok(i) = p.parse::<usize>() {
                out.push(SxPathSegment::Index(i));
            } else {
                out.push(SxPathSegment::Key(p.to_string()));
            }
        }
        Ok(Self { segments: out })
    }

    /// Returns child path with appended key.
    pub fn key(&self, key: impl Into<String>) -> Self {
        let mut next = self.clone();
        next.segments.push(SxPathSegment::Key(key.into()));
        next
    }

    /// Returns child path with appended index.
    pub fn index(&self, index: usize) -> Self {
        let mut next = self.clone();
        next.segments.push(SxPathSegment::Index(index));
        next
    }
}

impl std::fmt::Display for SxPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.segments.is_empty() {
            return write!(f, "/");
        }
        for seg in &self.segments {
            write!(f, "/")?;
            match seg {
                SxPathSegment::Key(k) => write!(f, "{k}")?,
                SxPathSegment::Index(i) => write!(f, "{i}")?,
            }
        }
        Ok(())
    }
}
