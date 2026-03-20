use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperatingSystem {
    Linux,
    Mac,
    Windows,
}

impl OperatingSystem {
    pub fn current() -> Result<Self, PlatformError> {
        match std::env::consts::OS {
            "linux" => Ok(Self::Linux),
            "macos" => Ok(Self::Mac),
            "windows" => Ok(Self::Windows),
            other => Err(PlatformError::UnsupportedOs(other.to_owned())),
        }
    }

    pub fn as_adoptium(&self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Mac => "mac",
            Self::Windows => "windows",
        }
    }
}

impl Display for OperatingSystem {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Linux => "linux",
            Self::Mac => "macos",
            Self::Windows => "windows",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Architecture {
    X64,
    Aarch64,
}

impl Architecture {
    pub fn current() -> Result<Self, PlatformError> {
        match std::env::consts::ARCH {
            "x86_64" => Ok(Self::X64),
            "aarch64" => Ok(Self::Aarch64),
            other => Err(PlatformError::UnsupportedArch(other.to_owned())),
        }
    }

    pub fn as_adoptium(&self) -> &'static str {
        match self {
            Self::X64 => "x64",
            Self::Aarch64 => "aarch64",
        }
    }
}

impl Display for Architecture {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::X64 => "x64",
            Self::Aarch64 => "aarch64",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Platform {
    pub os: OperatingSystem,
    pub arch: Architecture,
}

impl Platform {
    pub fn current() -> Result<Self, PlatformError> {
        Ok(Self {
            os: OperatingSystem::current()?,
            arch: Architecture::current()?,
        })
    }
}

impl Display for Platform {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}-{}", self.os, self.arch)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("unsupported operating system: {0}")]
    UnsupportedOs(String),
    #[error("unsupported architecture: {0}")]
    UnsupportedArch(String),
}

#[cfg(test)]
mod tests {
    use super::{Architecture, OperatingSystem};

    #[test]
    fn adoptium_os_names_match_expected_values() {
        assert_eq!(OperatingSystem::Linux.as_adoptium(), "linux");
        assert_eq!(OperatingSystem::Mac.as_adoptium(), "mac");
        assert_eq!(OperatingSystem::Windows.as_adoptium(), "windows");
    }

    #[test]
    fn adoptium_arch_names_match_expected_values() {
        assert_eq!(Architecture::X64.as_adoptium(), "x64");
        assert_eq!(Architecture::Aarch64.as_adoptium(), "aarch64");
    }
}