use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

use reqwest::Url;

use crate::error::{AppError, AppResult};

const HOST_NAME_KEY: &str = "cloudflare_host_name";
const TOKEN_KEY: &str = "cloudflare_token";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalEndpointSet {
    origin: String,
    mcp: String,
    protected_resource_metadata: String,
}

impl CanonicalEndpointSet {
    pub fn parse(value: &str) -> AppResult<Self> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(missing_variable(HOST_NAME_KEY));
        }

        let candidate = if trimmed.contains("://") {
            trimmed.to_string()
        } else {
            format!("https://{trimmed}")
        };
        let parsed = Url::parse(&candidate).map_err(|_| invalid_host_name())?;
        if parsed.scheme() != "https"
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.path() != "/"
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(invalid_host_name());
        }

        let origin = parsed.as_str().trim_end_matches('/').to_string();
        Ok(Self {
            mcp: format!("{origin}/mcp"),
            protected_resource_metadata: format!(
                "{origin}/.well-known/oauth-protected-resource/mcp"
            ),
            origin,
        })
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn mcp(&self) -> &str {
        &self.mcp
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn protected_resource_metadata(&self) -> &str {
        &self.protected_resource_metadata
    }
}

#[derive(Clone)]
pub struct FixedDomainConfig {
    pub endpoints: CanonicalEndpointSet,
    token: String,
}

impl FixedDomainConfig {
    pub fn token(&self) -> &str {
        &self.token
    }
}

impl fmt::Debug for FixedDomainConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FixedDomainConfig")
            .field("endpoints", &self.endpoints)
            .field("token", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct FixedDomainConfigProvider {
    project_root: PathBuf,
}

impl FixedDomainConfigProvider {
    pub fn new(project_root: &Path) -> Self {
        Self {
            project_root: project_root.to_path_buf(),
        }
    }

    pub fn resolve(
        &self,
        fallback_hostname: Option<&str>,
        fallback_token: Option<&str>,
    ) -> AppResult<FixedDomainConfig> {
        self.resolve_with_process(&process_values(), fallback_hostname, fallback_token)
    }

    pub fn resolve_hostname(
        &self,
        fallback_hostname: Option<&str>,
    ) -> AppResult<Option<CanonicalEndpointSet>> {
        let process = process_values();
        let dotenv = read_dotenv_values(&self.project_root)?;
        let Some(hostname) = value_from_sources(
            HOST_NAME_KEY,
            &process,
            &dotenv,
            fallback_hostname,
        ) else {
            return Ok(None);
        };
        CanonicalEndpointSet::parse(&hostname).map(Some)
    }

    fn resolve_with_process(
        &self,
        process: &HashMap<String, String>,
        fallback_hostname: Option<&str>,
        fallback_token: Option<&str>,
    ) -> AppResult<FixedDomainConfig> {
        let dotenv = read_dotenv_values(&self.project_root)?;
        resolve_from_sources(process, &dotenv, fallback_hostname, fallback_token)
    }
}

fn resolve_from_sources(
    process: &HashMap<String, String>,
    dotenv: &HashMap<String, String>,
    fallback_hostname: Option<&str>,
    fallback_token: Option<&str>,
) -> AppResult<FixedDomainConfig> {
    let hostname = value_from_sources(
        HOST_NAME_KEY,
        process,
        dotenv,
        fallback_hostname,
    )
    .ok_or_else(|| missing_variable(HOST_NAME_KEY))?;
    let token = value_from_sources(TOKEN_KEY, process, dotenv, fallback_token)
        .ok_or_else(|| missing_variable(TOKEN_KEY))?;

    Ok(FixedDomainConfig {
        endpoints: CanonicalEndpointSet::parse(&hostname)?,
        token,
    })
}

fn value_from_sources(
    key: &str,
    process: &HashMap<String, String>,
    dotenv: &HashMap<String, String>,
    fallback: Option<&str>,
) -> Option<String> {
    process
        .get(key)
        .and_then(non_empty_owned)
        .or_else(|| dotenv.get(key).and_then(non_empty_owned))
        .or_else(|| fallback.and_then(non_empty))
}

fn non_empty_owned(value: &String) -> Option<String> {
    non_empty(value)
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn process_values() -> HashMap<String, String> {
    [HOST_NAME_KEY, TOKEN_KEY]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| (key.to_string(), value)))
        .collect()
}

fn read_dotenv_values(project_root: &Path) -> AppResult<HashMap<String, String>> {
    let path = project_root.join(".env");
    if !path.is_file() {
        return Ok(HashMap::new());
    }

    let iter = dotenvy::from_path_iter(path).map_err(|_| dotenv_parse_error())?;
    let mut values = HashMap::new();
    for item in iter {
        let (key, value) = item.map_err(|_| dotenv_parse_error())?;
        if key == HOST_NAME_KEY || key == TOKEN_KEY {
            values.insert(key, value);
        }
    }
    Ok(values)
}

fn missing_variable(key: &str) -> AppError {
    AppError::Message(format!("Cloudflare 命名隧道缺少配置变量：{key}"))
}

fn invalid_host_name() -> AppError {
    AppError::Message(format!(
        "配置变量 {HOST_NAME_KEY} 必须是裸主机名或无路径的 HTTPS origin"
    ))
}

fn dotenv_parse_error() -> AppError {
    AppError::Message(format!(
        "项目根 .env 解析失败，请检查 {HOST_NAME_KEY} 与 {TOKEN_KEY} 的格式"
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;

    use super::{
        read_dotenv_values, resolve_from_sources, CanonicalEndpointSet,
        FixedDomainConfigProvider,
    };

    fn values(entries: &[(&str, &str)]) -> HashMap<String, String> {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn process_environment_wins_over_dotenv_and_workspace_fallback() {
        let process = values(&[
            ("cloudflare_host_name", "process.example.invalid"),
            ("cloudflare_token", "process-token-placeholder"),
        ]);
        let dotenv = values(&[
            ("cloudflare_host_name", "dotenv.example.invalid"),
            ("cloudflare_token", "dotenv-token-placeholder"),
        ]);

        let config = resolve_from_sources(
            &process,
            &dotenv,
            Some("fallback.example.invalid"),
            Some("fallback-token-placeholder"),
        )
        .expect("resolve fixed domain config");

        assert_eq!(config.endpoints.origin(), "https://process.example.invalid");
        assert_eq!(config.token(), "process-token-placeholder");
    }

    #[test]
    fn dotenv_parser_handles_quotes_without_mutating_process_environment() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::write(
            temp.path().join(".env"),
            "cloudflare_host_name=\"dotenv.example.invalid\"\ncloudflare_token='dotenv-token-placeholder'\n",
        )
        .expect("write dotenv fixture");

        let dotenv = read_dotenv_values(temp.path()).expect("read dotenv");
        let config = resolve_from_sources(
            &HashMap::new(),
            &dotenv,
            Some("fallback.example.invalid"),
            Some("fallback-token-placeholder"),
        )
        .expect("resolve fixed domain config");

        assert_eq!(config.endpoints.origin(), "https://dotenv.example.invalid");
        assert_eq!(config.token(), "dotenv-token-placeholder");
    }

    #[test]
    fn canonical_endpoints_reject_non_origin_inputs() {
        for invalid in [
            "http://mcp.example.invalid",
            "https://user@mcp.example.invalid",
            "https://mcp.example.invalid/mcp",
            "https://mcp.example.invalid?token=placeholder",
            "https://mcp.example.invalid#fragment",
        ] {
            let error = CanonicalEndpointSet::parse(invalid).unwrap_err();
            assert!(error.to_string().contains("cloudflare_host_name"));
            assert!(!error.to_string().contains(invalid));
        }
    }

    #[test]
    fn canonical_endpoints_derive_stable_mcp_and_oauth_urls() {
        let endpoints = CanonicalEndpointSet::parse("HTTPS://MCP.EXAMPLE.INVALID/")
            .expect("canonical endpoint");

        assert_eq!(endpoints.origin(), "https://mcp.example.invalid");
        assert_eq!(endpoints.mcp(), "https://mcp.example.invalid/mcp");
        assert_eq!(
            endpoints.protected_resource_metadata(),
            "https://mcp.example.invalid/.well-known/oauth-protected-resource/mcp"
        );
    }

    #[test]
    fn missing_token_error_names_variable_without_exposing_other_values() {
        let provider = FixedDomainConfigProvider::new(std::path::Path::new("missing-project"));
        let error = provider
            .resolve_with_process(
                &values(&[("cloudflare_host_name", "private-host.example.invalid")]),
                None,
                None,
            )
            .unwrap_err();
        let message = error.to_string();

        assert!(message.contains("cloudflare_token"));
        assert!(!message.contains("private-host.example.invalid"));
    }

}
