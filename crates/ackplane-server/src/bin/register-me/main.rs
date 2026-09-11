//! `register-me`: drive the real ADR-0085 enrollment ceremony from the
//! command line, as three explicit steps that mirror the real actors:
//!
//!   register-me request  --repo R --node N (--tenant-name T --salt-path PATH | --tenant-id ID)
//!   register-me approve  --request-id ID (--tenant-name T --salt-path PATH | --tenant-id ID)
//!                        --repo R --fingerprint FP --admin-database-url URL
//!   register-me activate --request-id ID
//!
//! `request` is the only step a node ever runs unattended. `approve` is a
//! separate administrator action — ADR-0085 gives a node no way to approve
//! itself, and no gRPC endpoint or UI for it exists yet, so this step calls
//! the enrollment store directly. That is a documented, single-operator
//! developer shortcut standing in for the not-yet-built approval surface,
//! never a claim that this is how a real deployment's administrator works.
//! `activate` proves possession of the approved key, then opens one real
//! `NodeSync` stream and sends one exactly replayable signed enrollment event so the node
//! is visibly live (e.g. in the Bridge Fleet view). `EnrollmentActivationResult`
//! returns the assigned `signing_key_id` directly, so `activate` needs no
//! database access at all.
//!
//! `request` explicitly selects `credential-facility-software` and an absolute
//! user-local `--state-dir`. The node provider owns the key, challenge and
//! activation receipt; the CLI persists only its immutable public request.
//! Raw key paths are refused rather than imported or used as a fallback.
//!
//! `--tenant-name` + `--salt-path` derive the same tenant id the Bridge
//! queries for (ADR-0098 decision 3) -- use it, or an enrolled repository
//! will never appear there. `--tenant-id` is a raw override for a deployment
//! that assigns tenant ids some other way (e.g. real OIDC).

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::ExitCode,
};

use sha2::{Digest, Sha256};
use tonic::Request;

use ackplane_node::{
    CredentialCandidate, CredentialProvider, CredentialProviderError, NodeSigner, SigningBinding,
};
use ackplane_protocol::v1::{self, node_enrollment_service_client::NodeEnrollmentServiceClient};
use ackplane_server::enrollment_store::{EnrollmentApproval, EnrollmentStore};
use ackplane_server::envelope_signature::envelope_signing_bytes;

const DEFAULT_GRPC_ENDPOINT: &str = "http://127.0.0.1:8443";

/// The same `hex(SHA-256(salt || tenant_name))` the Bridge derives (ADR-0098
/// decision 3). A node must enroll under this exact value, not the bare
/// tenant name, or the Bridge's Fleet query -- scoped to its own derived
/// token -- will never find it.
fn dev_tenant_token(salt: &[u8], tenant_name: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(salt);
    hasher.update(tenant_name.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Simple `--flag value` parser: no CLI-argument-parsing dependency exists
/// anywhere in this workspace yet, and this surface is small enough not to
/// be the reason to add one.
fn parse_flags(args: &[String]) -> Result<HashMap<String, String>, String> {
    let mut flags = HashMap::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let name = arg.strip_prefix("--").ok_or("expected a named --flag")?;
        if !matches!(
            name,
            "repo"
                | "node"
                | "tenant-id"
                | "tenant-name"
                | "salt-path"
                | "grpc-endpoint"
                | "display-name"
                | "capability"
                | "request-id"
                | "fingerprint"
                | "admin-database-url"
                | "approved-by"
                | "skip-sync"
                | "provider"
                | "state-dir"
        ) {
            return Err(format!(
                "unsupported option --{name}; raw key options are not supported"
            ));
        }
        let value = if name == "skip-sync" {
            String::new()
        } else {
            iter.next()
                .filter(|value| !value.starts_with("--"))
                .ok_or_else(|| format!("--{name} requires a value"))?
                .clone()
        };
        if flags.insert(name.to_string(), value).is_some() {
            return Err(format!("--{name} may be specified only once"));
        }
    }
    Ok(flags)
}

fn require<'a>(flags: &'a HashMap<String, String>, name: &str) -> Result<&'a str, String> {
    flags
        .get(name)
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("--{name} is required"))
}

fn state_directory(flags: &HashMap<String, String>) -> Result<PathBuf, String> {
    if flags.contains_key("key-path") {
        return Err(
            "--key-path is not supported; restore a provider identity instead of importing a seed"
                .to_string(),
        );
    }
    let path = PathBuf::from(require(flags, "state-dir")?);
    if !path.is_absolute() {
        return Err("--state-dir must be an absolute user-local directory".to_string());
    }
    Ok(path)
}

fn state_path(directory: &Path) -> PathBuf {
    directory.join("enrollment-request.json")
}

/// Resolve the wire `tenant_id`: derive it from `--tenant-name` + `--salt-path`
/// (matches what the Bridge will query for), or take `--tenant-id` directly
/// for a deployment that assigns tenant ids some other way (e.g. real OIDC).
fn resolve_tenant_id(flags: &HashMap<String, String>) -> Result<String, String> {
    if flags.contains_key("tenant-id") {
        return Ok(require(flags, "tenant-id")?.to_string());
    }
    let tenant_name = require(flags, "tenant-name")
        .map_err(|_| "either --tenant-id or --tenant-name + --salt-path is required".to_string())?;
    let salt_path = require(flags, "salt-path")?;
    let salt = std::fs::read(salt_path)
        .map_err(|error| format!("could not read salt {salt_path}: {error}"))?;
    Ok(dev_tenant_token(&salt, tenant_name))
}

mod enrollment;
mod provider;
mod request;

use request::SavedRequest;

fn print_usage() {
    eprintln!(
        "register-me: enroll a repository node with Ackplane\n\n\
         USAGE:\n\
         \x20 register-me request  --repo R --node N (--tenant-name T --salt-path PATH | --tenant-id ID)\n\
         \x20                      --provider credential-facility-software --state-dir ABSOLUTE_PATH\n\
         \x20                      [--grpc-endpoint URL] [--display-name NAME]\n\
         \x20                      [--capability C,C]\n\
         \x20 register-me approve  --request-id ID (--tenant-name T --salt-path PATH | --tenant-id ID)\n\
         \x20                      --repo R --fingerprint FP --admin-database-url URL\n\
         \x20                      [--approved-by NAME]\n\
         \x20 register-me activate --request-id ID --state-dir ABSOLUTE_PATH [--grpc-endpoint URL] [--skip-sync]\n\n\
         Use one user-local state directory per repository. The explicitly selected software\n\
         provider stores its key in the OS credential facility, never a seed file or dotenv.\n\
         Raw key options and implicit replacement are refused. Repeating an identical pending\n\
         request resumes it; changed enrollment parameters are refused.\n\n\
         The provider saves the assigned key ID and receipt before attempting NodeSync.\n\
         Repeating `activate` reuses that record; `--skip-sync` reports recorded activation\n\
         only and does not verify that the node is currently live or authorized.\n\n\
         `--tenant-name` + `--salt-path` derive the same tenant id the Bridge queries for --\n\
         use it, or the enrolled repository will never appear there.\n\
         `--tenant-id` is a raw override for a deployment that assigns tenant ids some other way.\n\n\
         `request` is the only step a real node runs unattended; `approve` is a separate\n\
         administrator action (a local-dev database shortcut standing in for the approval\n\
         RPC/UI that does not exist yet); `activate` proves possession and opens one real\n\
         NodeSync stream with an exactly replayable signed enrollment event, using the signing_key_id\n\
         EnrollmentActivationResult returns directly -- no database access needed."
    );
}

#[tokio::main]
async fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        print_usage();
        return ExitCode::FAILURE;
    }
    let command = args.remove(0);
    let flags = match parse_flags(&args) {
        Ok(flags) => flags,
        Err(error) => {
            eprintln!("register-me: {error}");
            return ExitCode::FAILURE;
        }
    };

    let result = match command.as_str() {
        "request" => enrollment::run_request(flags).await,
        "approve" => enrollment::run_approve(flags).await,
        "activate" => enrollment::run_activate(flags).await,
        _ => {
            print_usage();
            return ExitCode::FAILURE;
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("register-me: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests;
