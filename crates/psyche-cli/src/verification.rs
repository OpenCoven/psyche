//! Operator interface to the experimental receipt verification workflow.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Subcommand;
use psyche_cli::{EXIT_CHECK_FAILED, EXIT_OK};
use psyche_config::Config;
use psyche_runtime::receipt_verification;

#[derive(Debug, Subcommand)]
pub(super) enum Command {
    /// Register the original operation and its independently trusted issuer policy.
    Submit {
        /// Original canonical Psyche execution request.
        #[arg(long)]
        request: PathBuf,
        /// Expected task, operation references and outcome from the native reader.
        #[arg(long)]
        context: PathBuf,
        /// Operator-controlled OpenSSH allowed-signers policy.
        #[arg(long)]
        allowed_signers: PathBuf,
    },
    /// Verify supplied evidence against the registered operation.
    Supply {
        /// Verification ID printed by submit.
        id: String,
        /// Extracted verify-build-binding executable for this host's pinned kit.
        #[arg(long)]
        verifier: PathBuf,
        /// Exact bytes of the native journal receipt.
        #[arg(long)]
        receipt: Option<PathBuf>,
        /// Detached canonical Psyche-to-Build binding.
        #[arg(long)]
        binding: Option<PathBuf>,
        /// Detached issuer signature over the binding.
        #[arg(long)]
        signature: Option<PathBuf>,
    },
    /// Reopen the durable request, latest decision and original receipt digest.
    Show {
        /// Verification ID printed by submit.
        id: String,
    },
}

pub(super) fn run(config: &Config, command: Command) -> ExitCode {
    let is_supply = matches!(command, Command::Supply { .. });
    let result = match command {
        Command::Submit {
            request,
            context,
            allowed_signers,
        } => receipt_verification::submit(config, &request, &context, &allowed_signers),
        Command::Supply {
            id,
            verifier,
            receipt,
            binding,
            signature,
        } => receipt_verification::supply(
            config,
            &id,
            &verifier,
            receipt.as_deref(),
            binding.as_deref(),
            signature.as_deref(),
        ),
        Command::Show { id } => receipt_verification::inspect(config, &id),
    };
    let report = match result {
        Ok(report) => report,
        Err(error) => {
            eprintln!("{error}");
            return EXIT_CHECK_FAILED.into();
        }
    };
    match serde_json::to_string(&report) {
        Ok(json) => println!("{json}"),
        Err(_) => {
            eprintln!("unable to render verification report");
            return EXIT_CHECK_FAILED.into();
        }
    }
    if is_supply && !report.is_verified() {
        EXIT_CHECK_FAILED.into()
    } else {
        EXIT_OK.into()
    }
}
