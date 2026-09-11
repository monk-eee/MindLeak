use std::ffi::OsString;

use super::{confirm, inspect, RecoveryError, RecoveryPreview};
use crate::config::SupervisorConfig;

pub const USAGE: &str = "usage: ackplane-supervisor [--workers <workers.json>]\n       ackplane-supervisor [--workers <workers.json>] recover inspect <slot>\n       ackplane-supervisor [--workers <workers.json>] recover confirm <slot> <run-id> <confirmation-digest> --reason <reason>";

#[derive(Debug, PartialEq, Eq)]
enum Command<'arguments> {
    Inspect(&'arguments str),
    Confirm {
        slot: &'arguments str,
        run_id: &'arguments str,
        digest: &'arguments str,
        reason: &'arguments str,
    },
}

impl<'arguments> Command<'arguments> {
    fn parse(arguments: &'arguments [OsString]) -> Result<Self, RecoveryError> {
        let arguments = arguments
            .iter()
            .map(|argument| {
                argument.to_str().ok_or_else(|| {
                    RecoveryError::Refused("recovery arguments must be UTF-8".into())
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        match arguments.as_slice() {
            ["recover", "inspect", slot] => Ok(Self::Inspect(slot)),
            ["recover", "confirm", slot, run_id, digest, "--reason", reason]
                if digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()) =>
            {
                Ok(Self::Confirm {
                    slot,
                    run_id,
                    digest,
                    reason,
                })
            }
            _ => Err(RecoveryError::Refused(USAGE.into())),
        }
    }
}

pub async fn execute(
    config: &SupervisorConfig,
    arguments: &[OsString],
) -> Result<RecoveryPreview, RecoveryError> {
    match Command::parse(arguments)? {
        Command::Inspect(slot) => inspect(config, slot),
        Command::Confirm {
            slot,
            run_id,
            digest,
            reason,
        } => confirm(config, slot, run_id, digest, reason).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_requires_an_explicit_run_digest_and_reason() {
        let digest = "a".repeat(64);
        let arguments: Vec<_> = [
            "recover",
            "confirm",
            "first",
            "supervisor-first-run",
            &digest,
            "--reason",
            "operator cleanup",
        ]
        .map(OsString::from)
        .into();
        assert!(matches!(
            Command::parse(&arguments).unwrap(),
            Command::Confirm { slot: "first", .. }
        ));
        for arguments in [
            vec!["recover", "confirm", "first"],
            vec![
                "recover",
                "confirm",
                "first",
                "run",
                "not-a-digest",
                "--reason",
                "reason",
            ],
            vec!["recover", "inspect", "first", "--force"],
            vec!["recover", "kill", "1234"],
        ] {
            assert!(Command::parse(
                &arguments
                    .into_iter()
                    .map(OsString::from)
                    .collect::<Vec<_>>()
            )
            .is_err());
        }
    }
}
