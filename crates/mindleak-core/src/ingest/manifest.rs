//! Deterministic project-manifest dependency extraction.

use std::collections::BTreeSet;

use pep508_rs::Requirement;
use serde_json::Value as JsonValue;
use toml::Value;

use crate::{MindLeakError, Result};

/// One external package declared as a direct project dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub name: String,
}

/// Extract direct dependencies from a supported project manifest.
pub fn extract(path: &str, content: &str) -> Result<Option<Vec<Dependency>>> {
    let file_name = path.replace('\\', "/");
    let file_name = file_name.rsplit('/').next().unwrap_or(file_name.as_str());
    match file_name {
        "Cargo.toml" => cargo_dependencies(path, content).map(Some),
        "package.json" => package_json_dependencies(path, content).map(Some),
        "go.mod" => go_mod_dependencies(path, content).map(Some),
        name if name == "requirements.txt"
            || (name.starts_with("requirements-") && name.ends_with(".txt")) =>
        {
            requirements_txt_dependencies(path, content).map(Some)
        }
        _ => Ok(None),
    }
}

fn cargo_dependencies(path: &str, content: &str) -> Result<Vec<Dependency>> {
    let manifest: Value = toml::from_str(content)
        .map_err(|error| MindLeakError::Other(format!("invalid {path}: {error}")))?;
    let mut names = BTreeSet::new();
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        collect_cargo_table(manifest.get(section), &mut names);
    }
    if let Some(targets) = manifest.get("target").and_then(Value::as_table) {
        for target in targets.values() {
            for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
                collect_cargo_table(target.get(section), &mut names);
            }
        }
    }
    Ok(names.into_iter().map(|name| Dependency { name }).collect())
}

fn collect_cargo_table(value: Option<&Value>, names: &mut BTreeSet<String>) {
    let Some(dependencies) = value.and_then(Value::as_table) else {
        return;
    };
    for (alias, specification) in dependencies {
        let package = specification
            .as_table()
            .and_then(|table| table.get("package"))
            .and_then(Value::as_str)
            .unwrap_or(alias)
            .trim();
        if !package.is_empty() {
            names.insert(package.to_string());
        }
    }
}

fn package_json_dependencies(path: &str, content: &str) -> Result<Vec<Dependency>> {
    let manifest: JsonValue = serde_json::from_str(content)
        .map_err(|error| MindLeakError::Other(format!("invalid {path}: {error}")))?;
    if !manifest.is_object() {
        return Err(MindLeakError::Other(format!(
            "invalid {path}: root must be an object"
        )));
    }
    let mut names = BTreeSet::new();
    for section in [
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ] {
        if let Some(dependencies) = manifest.get(section).and_then(JsonValue::as_object) {
            names.extend(
                dependencies
                    .keys()
                    .filter(|name| !name.trim().is_empty())
                    .cloned(),
            );
        }
    }
    Ok(to_dependencies(names))
}

fn go_mod_dependencies(path: &str, content: &str) -> Result<Vec<Dependency>> {
    let mut names = BTreeSet::new();
    let mut in_require_block = false;
    for raw_line in content.lines() {
        let line = raw_line.split("//").next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if in_require_block {
            if line == ")" {
                in_require_block = false;
                continue;
            }
            let name = line.split_whitespace().next().unwrap_or("");
            if name.is_empty() || name == "(" {
                return Err(MindLeakError::Other(format!(
                    "invalid {path}: malformed require block"
                )));
            }
            names.insert(name.to_string());
            continue;
        }
        let Some(rest) = line.strip_prefix("require ") else {
            continue;
        };
        let rest = rest.trim();
        if rest == "(" {
            in_require_block = true;
            continue;
        }
        let name = rest.split_whitespace().next().unwrap_or("");
        if name.is_empty() {
            return Err(MindLeakError::Other(format!(
                "invalid {path}: malformed require directive"
            )));
        }
        names.insert(name.to_string());
    }
    if in_require_block {
        return Err(MindLeakError::Other(format!(
            "invalid {path}: unclosed require block"
        )));
    }
    Ok(to_dependencies(names))
}

fn requirements_txt_dependencies(path: &str, content: &str) -> Result<Vec<Dependency>> {
    let mut names = BTreeSet::new();
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty()
            || line.starts_with('#')
            || line.starts_with('-')
            || line.starts_with('.')
            || line.starts_with('/')
            || line.starts_with("git+")
            || line.starts_with("http://")
            || line.starts_with("https://")
        {
            continue;
        }
        let line = line.split(" #").next().unwrap_or(line).trim();
        let requirement = line.parse::<Requirement>().map_err(|error| {
            MindLeakError::Other(format!("invalid {path} requirement {line:?}: {error}"))
        })?;
        names.insert(requirement.name.to_string());
    }
    Ok(to_dependencies(names))
}

fn to_dependencies(names: BTreeSet<String>) -> Vec<Dependency> {
    names.into_iter().map(|name| Dependency { name }).collect()
}
