//! Community onboarding through the relay's existing invite contract.
use crate::{client::BeekeeperClient, error::CliError};
use clap::Subcommand;

#[derive(Subcommand)]
pub enum CommunityCmd {
    /// Mint an invitation under the relay's existing membership/limit policy.
    MintInvite {
        #[arg(long)]
        ttl_secs: Option<u64>,
        #[arg(long)]
        max_uses: Option<i32>,
    },
    /// Read the relay's current joining policy before accepting it.
    JoinPolicy,
    /// Accept the displayed policy revision and receive a claim-bound receipt.
    AcceptPolicy {
        #[arg(long)]
        code: String,
        #[arg(long)]
        policy_version: String,
        #[arg(long)]
        age_confirmed: bool,
    },
    /// Claim an invite as the CLI signing identity; invitation limits still apply.
    ClaimInvite {
        #[arg(long)]
        code: String,
        #[arg(long)]
        policy_receipt: Option<String>,
    },
}

pub async fn dispatch(cmd: CommunityCmd, client: &BeekeeperClient) -> Result<(), CliError> {
    let response = match cmd {
        CommunityCmd::MintInvite { ttl_secs, max_uses } => client.post_bootstrap("/api/invites", &serde_json::json!({"ttl_secs":ttl_secs,"max_uses":max_uses})).await?,
        CommunityCmd::JoinPolicy => client.get_public("/api/join-policy").await?,
        CommunityCmd::AcceptPolicy { code, policy_version, age_confirmed } => client.post_bootstrap("/api/invites/accept-policy", &serde_json::json!({"code":code,"policy_version":policy_version,"age_confirmed":age_confirmed})).await?,
        CommunityCmd::ClaimInvite { code, policy_receipt } => client.post_bootstrap("/api/invites/claim", &serde_json::json!({"code":code,"policy_receipt":policy_receipt})).await?,
    };
    println!("{response}");
    Ok(())
}
