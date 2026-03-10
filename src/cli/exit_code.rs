/// Process exit code newtype for type-safe exit status reporting.
///
/// Wraps a `u8` value representing the process exit code.
/// Provides named constants for common exit scenarios.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitCode(u8);

impl ExitCode {
    /// Successful execution.
    pub const SUCCESS: Self = Self(0);

    /// Configuration file is missing, unreadable, or contains invalid values.
    pub const CONFIG_ERROR: Self = Self(1);

    /// User cancelled interactive setup.
    pub const SETUP_CANCELLED: Self = Self(2);

    /// File I/O operation failed.
    pub const IO_ERROR: Self = Self(3);

    /// Web server failed to start or crashed.
    pub const SERVE_FAILED: Self = Self(4);

    /// Returns the raw exit code value.
    pub fn code(self) -> u8 {
        self.0
    }
}

impl std::fmt::Display for ExitCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<ExitCode> for std::process::ExitCode {
    fn from(code: ExitCode) -> Self {
        std::process::ExitCode::from(code.0)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn success_code_is_zero() {
        assert_eq!(ExitCode::SUCCESS.code(), 0);
    }

    #[test]
    fn config_error_code_is_one() {
        assert_eq!(ExitCode::CONFIG_ERROR.code(), 1);
    }

    #[test]
    fn setup_cancelled_code_is_two() {
        assert_eq!(ExitCode::SETUP_CANCELLED.code(), 2);
    }

    #[test]
    fn io_error_code_is_three() {
        assert_eq!(ExitCode::IO_ERROR.code(), 3);
    }

    #[test]
    fn serve_failed_code_is_four() {
        assert_eq!(ExitCode::SERVE_FAILED.code(), 4);
    }

    #[test]
    fn display_shows_numeric_value() {
        assert_eq!(ExitCode::SUCCESS.to_string(), "0");
        assert_eq!(ExitCode::CONFIG_ERROR.to_string(), "1");
        assert_eq!(ExitCode::SERVE_FAILED.to_string(), "4");
    }

    #[test]
    fn clone_produces_equal_value() {
        let original = ExitCode::CONFIG_ERROR;
        let cloned = original;
        assert_eq!(original, cloned);
    }

    #[test]
    fn into_process_exit_code() {
        let code = ExitCode::SUCCESS;
        let _process_code: std::process::ExitCode = code.into();
        // std::process::ExitCode doesn't expose its value, so we just verify conversion compiles
    }

    #[test]
    fn all_constants_are_distinct() {
        let codes = [
            ExitCode::SUCCESS,
            ExitCode::CONFIG_ERROR,
            ExitCode::SETUP_CANCELLED,
            ExitCode::IO_ERROR,
            ExitCode::SERVE_FAILED,
        ];
        for (i, a) in codes.iter().enumerate() {
            for (j, b) in codes.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "ExitCode constants at index {i} and {j} must differ");
                }
            }
        }
    }
}
