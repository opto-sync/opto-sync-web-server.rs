#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::env_map::{merge_env, EnvMap};
use flags2env::BundledFlags2Env;

const DOMAIN_CONFIG_SOURCE: &str = include_str!("../.opto-sync.toml");
const DOMAIN_CONFIG_NAME: &str = ".opto-sync.toml";
const MAX_DOMAIN_CONFIG_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
struct DomainConfig {
    contract: String,
    bindings: Vec<DomainBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DomainBinding {
    name: String,
    key: String,
    kind: String,
    required: bool,
    secret: bool,
    default: Option<String>,
}

#[derive(Default)]
struct BindingBuilder {
    name: Option<String>,
    key: Option<String>,
    kind: Option<String>,
    required: Option<bool>,
    secret: Option<bool>,
    default: Option<String>,
}

impl DomainConfig {
    fn embedded() -> Result<Self, String> {
        Self::parse(DOMAIN_CONFIG_SOURCE)
    }

    fn parse(source: &str) -> Result<Self, String> {
        if source.len() > MAX_DOMAIN_CONFIG_BYTES || source.as_bytes().contains(&0) {
            return Err(format!("{DOMAIN_CONFIG_NAME} is invalid"));
        }
        let mut section = Section::Root;
        let mut version = None;
        let mut mode = None;
        let mut strict = None;
        let mut contract = None;
        let mut require_audit = None;
        let mut precedence = None;
        let mut bindings = Vec::new();
        let mut current = None;
        for raw_line in source.lines() {
            let line = strip_comment(raw_line)?.trim();
            if line.is_empty() { continue; }
            if line == "[[env]]" {
                finish_binding(&mut current, &mut bindings)?;
                current = Some(BindingBuilder::default());
                section = Section::Env;
                continue;
            }
            if line == "[flags2env]" {
                finish_binding(&mut current, &mut bindings)?;
                section = Section::Flags;
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                finish_binding(&mut current, &mut bindings)?;
                section = Section::Other;
                continue;
            }
            let (key, value) = line.split_once('=').ok_or_else(|| format!("{DOMAIN_CONFIG_NAME} is invalid"))?;
            let key = key.trim();
            let value = value.trim();
            match section {
                Section::Root => match key {
                    "version" => set_once(&mut version, parse_u32(value)?)?,
                    "mode" => set_once(&mut mode, parse_string(value)?)?,
                    "strict" => set_once(&mut strict, parse_bool(value)?)?,
                    _ => {}
                },
                Section::Flags => match key {
                    "contract" => set_once(&mut contract, parse_string(value)?)?,
                    "require_audit" => set_once(&mut require_audit, parse_bool(value)?)?,
                    "precedence" => set_once(&mut precedence, parse_string(value)?)?,
                    _ => {}
                },
                Section::Env => current.as_mut().ok_or_else(|| format!("{DOMAIN_CONFIG_NAME} is invalid"))?.assign(key, value)?,
                Section::Other => {}
            }
        }
        finish_binding(&mut current, &mut bindings)?;
        if version != Some(1) || mode.as_deref() != Some("server") || strict != Some(true) {
            return Err(format!("{DOMAIN_CONFIG_NAME} must be strict version-1 server config"));
        }
        let contract = contract.ok_or_else(|| format!("{DOMAIN_CONFIG_NAME} has no flags contract"))?;
        if contract != ".cli-flags.toml" || require_audit != Some(true) || precedence.as_deref() != Some("argv-over-env") {
            return Err(format!("{DOMAIN_CONFIG_NAME} must select audited .cli-flags.toml with argv-over-env precedence"));
        }
        validate_bindings(&bindings)?;
        Ok(Self { contract, bindings })
    }

    fn apply(&self, env: &mut EnvMap) -> Result<(), String> {
        for binding in &self.bindings {
            let present = env.get(&binding.key).is_some_and(|value| !value.trim().is_empty());
            if present { continue; }
            if let Some(default) = &binding.default {
                env.insert(binding.key.clone(), default.clone());
            } else if binding.required {
                return Err(format!("{DOMAIN_CONFIG_NAME} required binding {} is missing", binding.name));
            }
        }
        Ok(())
    }
}

impl BindingBuilder {
    fn assign(&mut self, key: &str, value: &str) -> Result<(), String> {
        match key {
            "name" => set_once(&mut self.name, parse_string(value)?),
            "key" => set_once(&mut self.key, parse_string(value)?),
            "kind" => set_once(&mut self.kind, parse_string(value)?),
            "required" => set_once(&mut self.required, parse_bool(value)?),
            "secret" => set_once(&mut self.secret, parse_bool(value)?),
            "default" => set_once(&mut self.default, parse_string(value)?),
            _ => Ok(()),
        }
    }
    fn finish(self) -> Result<DomainBinding, String> {
        let name = self.name.ok_or_else(|| format!("{DOMAIN_CONFIG_NAME} env binding has no name"))?;
        let binding = DomainBinding {
            key: self.key.ok_or_else(|| format!("{DOMAIN_CONFIG_NAME} binding {name} has no key"))?,
            kind: self.kind.ok_or_else(|| format!("{DOMAIN_CONFIG_NAME} binding {name} has no kind"))?,
            required: self.required.ok_or_else(|| format!("{DOMAIN_CONFIG_NAME} binding {name} has no required flag"))?,
            secret: self.secret.ok_or_else(|| format!("{DOMAIN_CONFIG_NAME} binding {name} has no secret flag"))?,
            default: self.default,
            name,
        };
        if binding.secret && binding.default.is_some() {
            return Err(format!("{DOMAIN_CONFIG_NAME} secret binding {} may not have a plaintext default", binding.name));
        }
        Ok(binding)
    }
}

#[derive(Clone, Copy)]
enum Section { Root, Flags, Env, Other }

fn finish_binding(current: &mut Option<BindingBuilder>, bindings: &mut Vec<DomainBinding>) -> Result<(), String> {
    if let Some(builder) = current.take() { bindings.push(builder.finish()?); }
    Ok(())
}

fn validate_bindings(bindings: &[DomainBinding]) -> Result<(), String> {
    let mut names = HashSet::new();
    let mut keys = HashSet::new();
    for binding in bindings {
        if !valid_name(&binding.name) || !valid_env_key(&binding.key) || !matches!(binding.kind.as_str(), "string" | "bool" | "integer" | "double" | "json" | "url") {
            return Err(format!("{DOMAIN_CONFIG_NAME} binding {} is invalid", binding.name));
        }
        if !names.insert(binding.name.as_str()) || !keys.insert(binding.key.as_str()) {
            return Err(format!("{DOMAIN_CONFIG_NAME} binding {} is duplicated or aliases another binding", binding.name));
        }
    }
    Ok(())
}

fn valid_name(value: &str) -> bool {
    let Some(first) = value.bytes().next() else { return false; };
    first.is_ascii_lowercase() && value.len() <= 64 && value.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}
fn valid_env_key(value: &str) -> bool {
    let Some(first) = value.bytes().next() else { return false; };
    (first.is_ascii_uppercase() || first == b'_') && value.len() <= 128 && value.bytes().all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}
fn set_once<T>(slot: &mut Option<T>, value: T) -> Result<(), String> {
    if slot.is_some() { return Err(format!("{DOMAIN_CONFIG_NAME} contains a duplicate field")); }
    *slot = Some(value); Ok(())
}
fn parse_u32(value: &str) -> Result<u32, String> { value.parse::<u32>().map_err(|_| format!("{DOMAIN_CONFIG_NAME} contains an invalid integer")) }
fn parse_bool(value: &str) -> Result<bool, String> { match value { "true" => Ok(true), "false" => Ok(false), _ => Err(format!("{DOMAIN_CONFIG_NAME} contains an invalid boolean")) } }
fn parse_string(value: &str) -> Result<String, String> {
    let value = value.strip_prefix('"').and_then(|candidate| candidate.strip_suffix('"')).ok_or_else(|| format!("{DOMAIN_CONFIG_NAME} common envelope requires quoted strings"))?;
    if value.is_empty() || value.chars().any(|character| character.is_control() || character == '\\') { return Err(format!("{DOMAIN_CONFIG_NAME} contains an unsafe string")); }
    Ok(value.to_owned())
}
fn strip_comment(line: &str) -> Result<&str, String> {
    let mut quoted = false;
    for (index, byte) in line.bytes().enumerate() {
        if byte == b'"' { quoted = !quoted; }
        else if byte == b'#' && !quoted { return Ok(&line[..index]); }
        else if byte == b'\\' && quoted { return Err(format!("{DOMAIN_CONFIG_NAME} common envelope does not permit escaped strings")); }
    }
    if quoted { Err(format!("{DOMAIN_CONFIG_NAME} has an unterminated string")) } else { Ok(line) }
}

pub fn parse_cli_flags(argv: &[String], config_path: &Path) -> Result<HashMap<String, String>, String> {
    let config_path = config_path.to_str().ok_or_else(|| ".cli-flags.toml path is not valid UTF-8".to_string())?;
    let parser = BundledFlags2Env::new();
    parser.audit_config(Some(config_path)).map_err(|error| format!("flags-2-env configuration audit failed: {error}"))?;
    let parsed = parser.parse_structured(argv, Some(config_path)).map_err(|error| format!("flags-2-env parse failed: {error}"))?;
    if !parsed.unknown_options.is_empty() { return Err(format!("unknown command-line option(s): {}", parsed.unknown_options.join(", "))); }
    if !parsed.errors.is_empty() { return Err(format!("invalid command-line value(s): {}", parsed.errors.join("; "))); }
    Ok(parsed.flags)
}

pub fn apply_cli_flags() -> Result<EnvMap, String> {
    let domain = DomainConfig::embedded()?;
    apply_cli_flags_from(std::env::args().collect(), std::env::vars().collect(), Path::new(&domain.contract))
}

pub fn apply_cli_flags_from(argv: Vec<String>, initial: EnvMap, config_path: &Path) -> Result<EnvMap, String> {
    let domain = DomainConfig::embedded()?;
    if config_path.file_name().and_then(|value| value.to_str()) != Some(domain.contract.as_str()) {
        return Err(format!("{DOMAIN_CONFIG_NAME} flags2env contract does not match the executable contract path"));
    }
    let mut env = merge_env(initial, parse_cli_flags(&argv, config_path)?);
    domain.apply(&mut env)?;
    Ok(env)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env_map::value;
    fn config_path() -> std::path::PathBuf { Path::new(env!("CARGO_MANIFEST_DIR")).join(".cli-flags.toml") }

    #[test]
    fn domain_manifest_controls_runtime_default() {
        let env = apply_cli_flags_from(vec!["svc".into()], EnvMap::new(), &config_path()).expect("embedded Opto Sync config must be executable");
        assert_eq!(value(&env, "OPTO_SYNC_WEB_BIND"), Some("127.0.0.1:8081"));
    }
    #[test]
    fn cli_overrides_merge_into_map_without_mutating_process_env() {
        let before = std::env::var_os("ENV_MAP_PROBE");
        let env = apply_cli_flags_from(vec!["svc".into()], EnvMap::from([("ENV_MAP_PROBE".into(), "before".into())]), &config_path()).expect("valid flags");
        assert_eq!(value(&env, "ENV_MAP_PROBE"), Some("before"));
        assert_eq!(std::env::var_os("ENV_MAP_PROBE"), before);
    }
    #[test]
    fn domain_manifest_rejects_wrong_mode_and_secret_defaults() {
        let wrong_mode = DOMAIN_CONFIG_SOURCE.replacen("mode = \"server\"", "mode = \"client\"", 1);
        assert!(DomainConfig::parse(&wrong_mode).is_err());
        let secret_default = DOMAIN_CONFIG_SOURCE.replacen("secret = false\ndefault = \"127.0.0.1:8081\"", "secret = true\ndefault = \"plaintext\"", 1);
        assert!(DomainConfig::parse(&secret_default).is_err());
    }
    #[test]
    fn parse_failure_does_not_mutate_process_environment() {
        let before = std::env::var_os("ENV_MAP_PROBE");
        let initial = EnvMap::from([("ENV_MAP_PROBE".into(), "keep".into())]);
        assert!(apply_cli_flags_from(vec!["svc".into(), "--this-flag-is-not-declared".into()], initial, &config_path()).is_err());
        assert_eq!(std::env::var_os("ENV_MAP_PROBE"), before);
    }
    #[test]
    fn source_does_not_mutate_process_environment() {
        const SRC: &str = include_str!("flags.rs");
        let production = SRC.split("#[cfg(test)]").next().unwrap_or(SRC);
        assert!(!production.contains("std::env::set_var"));
        assert!(!production.contains("env::set_var"));
    }
}
