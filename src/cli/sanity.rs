use acton_service::prelude::*;
use acton_service::session::SessionStorage;

use crate::config::MissiveConfig;
use crate::jmap::{JmapUrl, MailboxInfo};

/// Status of an individual check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckStatus {
    Passed,
    Failed { message: String },
}

impl CheckStatus {
    pub fn passed(&self) -> bool {
        matches!(self, Self::Passed)
    }
}

/// Result of a single sanity check.
#[derive(Debug, Clone)]
pub struct CheckResult {
    /// Name of the check (e.g., "jmap_url").
    pub name: String,
    /// Whether the check passed.
    pub passed: bool,
    /// Human-readable message explaining the result.
    pub message: String,
}

/// Result of checking a required mailbox role.
#[derive(Debug, Clone)]
pub struct MailboxCheck {
    pub role: String,
    pub status: CheckStatus,
}

/// Result of JMAP connectivity sanity checks.
#[derive(Debug, Clone)]
pub struct JmapSanityResult {
    pub connection: CheckStatus,
    pub mailbox_fetch: CheckStatus,
    pub identity_fetch: CheckStatus,
    pub required_mailboxes: Vec<MailboxCheck>,
    pub identity_count: usize,
}

/// Aggregated sanity report for the entire configuration.
#[derive(Debug, Clone)]
pub struct SanityReport {
    /// Checks that were performed.
    pub checks: Vec<CheckResult>,
}

impl SanityReport {
    /// Returns `true` if all checks passed.
    pub fn all_passed(&self) -> bool {
        self.checks.iter().all(|c| c.passed)
    }

    /// Returns the number of failed checks.
    pub fn failure_count(&self) -> usize {
        self.checks.iter().filter(|c| !c.passed).count()
    }
}

const REQUIRED_MAILBOX_ROLES: &[&str] = &["inbox", "drafts", "sent", "trash"];

/// Check that required mailbox roles are present.
///
/// This is a pure function that can be tested without a JMAP connection.
pub fn check_required_mailboxes(mailboxes: &[MailboxInfo]) -> Vec<MailboxCheck> {
    REQUIRED_MAILBOX_ROLES
        .iter()
        .map(|&role| {
            let found = mailboxes.iter().any(|m| m.role.eq_ignore_ascii_case(role));
            MailboxCheck {
                role: role.to_string(),
                status: if found {
                    CheckStatus::Passed
                } else {
                    CheckStatus::Failed {
                        message: format!("no mailbox with role '{role}' found on server"),
                    }
                },
            }
        })
        .collect()
}

/// Run JMAP connectivity sanity checks against a server.
///
/// Tests connection, mailbox fetching, required mailbox roles, and identities.
pub async fn run_jmap_sanity_checks(
    jmap_url: &JmapUrl,
    username: &str,
    password: &str,
) -> JmapSanityResult {
    let client = match crate::jmap::create_client(jmap_url, username, password).await {
        Ok(c) => c,
        Err(e) => {
            return JmapSanityResult {
                connection: CheckStatus::Failed {
                    message: e.to_string(),
                },
                mailbox_fetch: CheckStatus::Failed {
                    message: "skipped (connection failed)".to_string(),
                },
                identity_fetch: CheckStatus::Failed {
                    message: "skipped (connection failed)".to_string(),
                },
                required_mailboxes: REQUIRED_MAILBOX_ROLES
                    .iter()
                    .map(|role| MailboxCheck {
                        role: role.to_string(),
                        status: CheckStatus::Failed {
                            message: "skipped (connection failed)".to_string(),
                        },
                    })
                    .collect(),
                identity_count: 0,
            };
        }
    };

    let connection = CheckStatus::Passed;

    let (mailbox_fetch, required_mailboxes) =
        match crate::jmap::fetch_mailboxes(&client).await {
            Ok(mailboxes) => {
                let checks = check_required_mailboxes(&mailboxes);
                (CheckStatus::Passed, checks)
            }
            Err(e) => (
                CheckStatus::Failed {
                    message: e.to_string(),
                },
                REQUIRED_MAILBOX_ROLES
                    .iter()
                    .map(|role| MailboxCheck {
                        role: role.to_string(),
                        status: CheckStatus::Failed {
                            message: "skipped (mailbox fetch failed)".to_string(),
                        },
                    })
                    .collect(),
            ),
        };

    let (identity_fetch, identity_count) = match crate::jmap::fetch_identities(&client).await {
        Ok(identities) => {
            let count = identities.len();
            if count == 0 {
                (
                    CheckStatus::Failed {
                        message: "no sending identities found on server".to_string(),
                    },
                    0,
                )
            } else {
                (CheckStatus::Passed, count)
            }
        }
        Err(e) => (
            CheckStatus::Failed {
                message: e.to_string(),
            },
            0,
        ),
    };

    JmapSanityResult {
        connection,
        mailbox_fetch,
        identity_fetch,
        required_mailboxes,
        identity_count,
    }
}

/// Print JMAP sanity check results to stdout.
pub fn print_jmap_results(result: &JmapSanityResult) {
    print_status("Connection", &result.connection);
    print_status("Mailbox/get", &result.mailbox_fetch);

    for check in &result.required_mailboxes {
        let label = format!("Mailbox role '{}'", check.role);
        print_status(&label, &check.status);
    }

    print_status("Identity/get", &result.identity_fetch);
    if result.identity_count > 0 {
        println!("  Found {} sending identity(ies)", result.identity_count);
    }
}

fn print_status(label: &str, status: &CheckStatus) {
    match status {
        CheckStatus::Passed => println!("  [PASS] {label}"),
        CheckStatus::Failed { message } => println!("  [FAIL] {label}: {message}"),
    }
}

/// Run sanity checks against the loaded configuration.
///
/// Validates that the configuration values are internally consistent
/// and reasonable without starting the server.
pub fn check_config(config: &Config<MissiveConfig>) -> SanityReport {
    let checks = vec![
        check_jmap_url(config),
        check_service_port(config),
        check_page_size(config),
        check_session_config(config),
    ];

    SanityReport { checks }
}

/// Print the sanity report to stdout with pass/fail indicators.
pub fn print_report(report: &SanityReport) {
    for check in &report.checks {
        let indicator = if check.passed { "PASS" } else { "FAIL" };
        println!("[{indicator}] {}: {}", check.name, check.message);
    }

    println!();
    if report.all_passed() {
        println!(
            "All {} checks passed. Configuration is valid.",
            report.checks.len()
        );
    } else {
        println!(
            "{} of {} checks failed.",
            report.failure_count(),
            report.checks.len()
        );
    }
}

fn check_jmap_url(config: &Config<MissiveConfig>) -> CheckResult {
    let url = &config.custom.jmap_url;

    if url.is_empty() {
        return CheckResult {
            name: "jmap_url".to_string(),
            passed: false,
            message: "JMAP URL is empty; set jmap_url in config.toml or ACTON_JMAP_URL env var"
                .to_string(),
        };
    }

    match url.validate() {
        Ok(()) => CheckResult {
            name: "jmap_url".to_string(),
            passed: true,
            message: format!("JMAP URL is valid: {url}"),
        },
        Err(e) => CheckResult {
            name: "jmap_url".to_string(),
            passed: false,
            message: format!("JMAP URL is invalid: {e}"),
        },
    }
}

fn check_service_port(config: &Config<MissiveConfig>) -> CheckResult {
    let port = config.service.port;

    if port == 0 {
        return CheckResult {
            name: "service_port".to_string(),
            passed: false,
            message: "service port is 0; this will cause the OS to assign a random port"
                .to_string(),
        };
    }

    CheckResult {
        name: "service_port".to_string(),
        passed: true,
        message: format!("service port is {port}"),
    }
}

fn check_page_size(config: &Config<MissiveConfig>) -> CheckResult {
    let page_size = config.custom.page_size;

    if page_size == 0 {
        return CheckResult {
            name: "page_size".to_string(),
            passed: false,
            message: "page_size is 0; must be at least 1".to_string(),
        };
    }

    if page_size > 1000 {
        return CheckResult {
            name: "page_size".to_string(),
            passed: false,
            message: format!(
                "page_size is {page_size}; values above 1000 may cause JMAP server timeouts"
            ),
        };
    }

    CheckResult {
        name: "page_size".to_string(),
        passed: true,
        message: format!("page_size is {page_size}"),
    }
}

fn check_session_config(config: &Config<MissiveConfig>) -> CheckResult {
    let session = config.session.clone().unwrap_or_default();

    match session.storage {
        SessionStorage::Memory => CheckResult {
            name: "session".to_string(),
            passed: true,
            message: "using in-memory session storage".to_string(),
        },
        SessionStorage::Redis => {
            if session.redis_url.is_none() {
                CheckResult {
                    name: "session".to_string(),
                    passed: false,
                    message:
                        "session storage is 'redis' but redis_url is not set; add [session] redis_url to config"
                            .to_string(),
                }
            } else {
                CheckResult {
                    name: "session".to_string(),
                    passed: true,
                    message: "using Redis session storage".to_string(),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::jmap::JmapUrl;

    /// Helper to build a `Config<MissiveConfig>` with overrides for testing.
    fn test_config(jmap_url: &str, port: u16, page_size: usize) -> Config<MissiveConfig> {
        let mut config = Config::<MissiveConfig>::default();
        config.custom.jmap_url = JmapUrl::from(jmap_url);
        config.service.port = port;
        config.custom.page_size = page_size;
        config
    }

    #[test]
    fn valid_config_passes_all_checks() {
        let config = test_config("https://mail.example.com", 8080, 50);
        let report = check_config(&config);
        assert!(
            report.all_passed(),
            "Expected all checks to pass, but {} failed",
            report.failure_count()
        );
    }

    #[test]
    fn empty_jmap_url_fails() {
        let config = test_config("", 8080, 50);
        let report = check_config(&config);
        let jmap_check = report.checks.iter().find(|c| c.name == "jmap_url").unwrap();
        assert!(!jmap_check.passed);
        assert!(jmap_check.message.contains("empty"));
    }

    #[test]
    fn invalid_jmap_url_fails() {
        let config = test_config("not-a-url", 8080, 50);
        let report = check_config(&config);
        let jmap_check = report.checks.iter().find(|c| c.name == "jmap_url").unwrap();
        assert!(!jmap_check.passed);
        assert!(jmap_check.message.contains("invalid"));
    }

    #[test]
    fn port_zero_fails() {
        let config = test_config("https://mail.example.com", 0, 50);
        let report = check_config(&config);
        let port_check = report
            .checks
            .iter()
            .find(|c| c.name == "service_port")
            .unwrap();
        assert!(!port_check.passed);
    }

    #[test]
    fn valid_port_passes() {
        let config = test_config("https://mail.example.com", 443, 50);
        let report = check_config(&config);
        let port_check = report
            .checks
            .iter()
            .find(|c| c.name == "service_port")
            .unwrap();
        assert!(port_check.passed);
    }

    #[test]
    fn page_size_zero_fails() {
        let config = test_config("https://mail.example.com", 8080, 0);
        let report = check_config(&config);
        let check = report
            .checks
            .iter()
            .find(|c| c.name == "page_size")
            .unwrap();
        assert!(!check.passed);
        assert!(check.message.contains("at least 1"));
    }

    #[test]
    fn page_size_above_1000_fails() {
        let config = test_config("https://mail.example.com", 8080, 1001);
        let report = check_config(&config);
        let check = report
            .checks
            .iter()
            .find(|c| c.name == "page_size")
            .unwrap();
        assert!(!check.passed);
        assert!(check.message.contains("1001"));
    }

    #[test]
    fn page_size_1000_passes() {
        let config = test_config("https://mail.example.com", 8080, 1000);
        let report = check_config(&config);
        let check = report
            .checks
            .iter()
            .find(|c| c.name == "page_size")
            .unwrap();
        assert!(check.passed);
    }

    #[test]
    fn memory_session_passes() {
        let config = test_config("https://mail.example.com", 8080, 50);
        // Default session is memory
        let report = check_config(&config);
        let check = report.checks.iter().find(|c| c.name == "session").unwrap();
        assert!(check.passed);
        assert!(check.message.contains("in-memory"));
    }

    #[test]
    fn report_failure_count() {
        let config = test_config("", 0, 0);
        let report = check_config(&config);
        assert!(report.failure_count() >= 3);
        assert!(!report.all_passed());
    }

    #[test]
    fn report_all_passed_with_valid_config() {
        let config = test_config("https://mail.example.com", 8080, 50);
        let report = check_config(&config);
        assert!(report.all_passed());
        assert_eq!(report.failure_count(), 0);
    }

    // --- check_required_mailboxes tests ---

    use crate::jmap::MailboxId;

    fn make_mailbox(role: &str) -> MailboxInfo {
        MailboxInfo {
            id: MailboxId::from("test-id"),
            name: role.to_string(),
            role: role.to_string(),
            unread_count: 0,
        }
    }

    #[test]
    fn all_required_mailboxes_present() {
        let mailboxes = vec![
            make_mailbox("inbox"),
            make_mailbox("drafts"),
            make_mailbox("sent"),
            make_mailbox("trash"),
            make_mailbox("archive"),
        ];
        let checks = check_required_mailboxes(&mailboxes);
        assert_eq!(checks.len(), 4);
        assert!(checks.iter().all(|c| c.status.passed()));
    }

    #[test]
    fn missing_mailbox_roles_detected() {
        let mailboxes = vec![make_mailbox("inbox"), make_mailbox("sent")];
        let checks = check_required_mailboxes(&mailboxes);

        let drafts = checks.iter().find(|c| c.role == "drafts").unwrap();
        assert!(!drafts.status.passed());

        let trash = checks.iter().find(|c| c.role == "trash").unwrap();
        assert!(!trash.status.passed());

        let inbox = checks.iter().find(|c| c.role == "inbox").unwrap();
        assert!(inbox.status.passed());
    }

    #[test]
    fn empty_mailbox_list_fails_all() {
        let checks = check_required_mailboxes(&[]);
        assert_eq!(checks.len(), 4);
        assert!(checks.iter().all(|c| !c.status.passed()));
    }

    #[test]
    fn mailbox_role_matching_is_case_insensitive() {
        let mailboxes = vec![
            make_mailbox("INBOX"),
            make_mailbox("Drafts"),
            make_mailbox("Sent"),
            make_mailbox("TRASH"),
        ];
        let checks = check_required_mailboxes(&mailboxes);
        assert!(checks.iter().all(|c| c.status.passed()));
    }

    #[test]
    fn check_status_passed_helper() {
        assert!(CheckStatus::Passed.passed());
        assert!(!CheckStatus::Failed {
            message: "test".to_string()
        }
        .passed());
    }
}
