use std::io::Write;
use std::path::PathBuf;
use std::sync::LazyLock;

use chrono::{DateTime, FixedOffset, SecondsFormat, Utc};
use regex::Regex;

use crate::platform::platform;

static AUTHORIZATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(authorization\s*:)\s*[^\r\n]+").expect("authorization sanitizer regex")
});
static BEARER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bbearer\s+[^\s,;]+").expect("bearer sanitizer regex"));
static TOKEN_ARGUMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(--token(?:\s+|=))[^\s]+").expect("token argument sanitizer regex")
});
static SECRET_ASSIGNMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)\b([a-z0-9_]*(?:token|secret|password)[a-z0-9_]*)\s*([:=])\s*(?:"[^"]*"|'[^']*'|[^\s,;]+)"#,
    )
    .expect("secret assignment sanitizer regex")
});
static OAUTH_QUERY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)([?&](?:code|client_secret|access_token)=)[^&\s]+")
        .expect("OAuth query sanitizer regex")
});
static URL_USERINFO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(https?://)[^/\s:@]+:[^@\s/]+@").expect("URL userinfo sanitizer regex")
});

fn utc_plus_8() -> FixedOffset {
    FixedOffset::east_opt(8 * 60 * 60).expect("UTC+08:00 is a valid fixed offset")
}

fn utc_plus_8_timestamp() -> String {
    DateTime::<Utc>::from(std::time::SystemTime::now())
        .with_timezone(&utc_plus_8())
        .to_rfc3339_opts(SecondsFormat::Secs, false)
}

pub fn sanitize_log_line(line: &str) -> String {
    let line = AUTHORIZATION.replace_all(line, "$1 <redacted>");
    let line = BEARER.replace_all(&line, "Bearer <redacted>");
    let line = TOKEN_ARGUMENT.replace_all(&line, "$1<redacted>");
    let line = SECRET_ASSIGNMENT.replace_all(&line, "$1$2<redacted>");
    let line = OAUTH_QUERY.replace_all(&line, "$1<redacted>");
    URL_USERINFO
        .replace_all(&line, "$1<redacted>@")
        .into_owned()
}

pub(crate) fn format_cloudflared_log_line(line: &str) -> String {
    let sanitized = sanitize_log_line(line);
    let Some((timestamp, remainder)) = sanitized.split_once(' ') else {
        return sanitized;
    };
    let Ok(parsed) = DateTime::parse_from_rfc3339(timestamp) else {
        return sanitized;
    };
    let localized = parsed
        .with_timezone(&utc_plus_8())
        .to_rfc3339_opts(SecondsFormat::AutoSi, false);
    format!("{localized} {remainder}")
}

pub(crate) fn timestamped_log_line(line: &str) -> String {
    format!("{} {}", utc_plus_8_timestamp(), sanitize_log_line(line))
}

pub fn new_trace_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

pub fn log_dir_for_profile(profile_id: &str) -> PathBuf {
    platform()
        .app_config_dir()
        .map(|home| home.join("logs").join(profile_id))
        .unwrap_or_else(|_| PathBuf::from("logs").join(profile_id))
}

pub fn append_profile_log(profile_id: &str, file_name: &str, line: &str) {
    let log_dir = log_dir_for_profile(profile_id);
    if std::fs::create_dir_all(&log_dir).is_err() {
        return;
    }
    let path = log_dir.join(file_name);
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{}", timestamped_log_line(line));
    }
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;

    use super::{format_cloudflared_log_line, sanitize_log_line, timestamped_log_line};

    #[test]
    fn redacts_known_secret_shapes_before_logging() {
        for (input, forbidden) in [
            (
                "Authorization: Bearer bearer-placeholder",
                "bearer-placeholder",
            ),
            (
                "cloudflared tunnel run --token token-placeholder",
                "token-placeholder",
            ),
            (
                "cloudflare_token='dotenv-token-placeholder'",
                "dotenv-token-placeholder",
            ),
            (
                "oauth_client_secret=client-secret-placeholder",
                "client-secret-placeholder",
            ),
            (
                "callback?code=oauth-code-placeholder&state=ok",
                "oauth-code-placeholder",
            ),
            (
                "https://user:password-placeholder@example.invalid/mcp",
                "password-placeholder",
            ),
        ] {
            let sanitized = sanitize_log_line(input);
            assert!(!sanitized.contains(forbidden), "{sanitized}");
            assert!(sanitized.contains("<redacted>"), "{sanitized}");
        }
    }

    #[test]
    fn preserves_normal_diagnostic_errors() {
        let input = "cloudflared exited with status 1 after edge registration timeout";
        assert_eq!(sanitize_log_line(input), input);
    }

    #[test]
    fn cloudflared_timestamp_is_converted_to_utc_plus_8() {
        let line = "2026-08-31T18:43:16Z ERR edge connection failed";

        assert_eq!(
            format_cloudflared_log_line(line),
            "2026-09-01T02:43:16+08:00 ERR edge connection failed"
        );
    }

    #[test]
    fn application_log_line_has_utc_plus_8_timestamp_and_redaction() {
        let line = timestamped_log_line("token=secret-placeholder stage=public-error");
        let (timestamp, message) = line.split_once(' ').expect("timestamped log line");
        let parsed = DateTime::parse_from_rfc3339(timestamp).expect("RFC3339 timestamp");

        assert_eq!(parsed.offset().local_minus_utc(), 8 * 60 * 60);
        assert_eq!(message, "token=<redacted> stage=public-error");
    }
}
