use beekeeper_core::role_vocabulary::RoleVocabulary;
// CLI adapters for the shared receipt-backed authority projection.
use crate::{client::BeekeeperClient, error::CliError};
#[cfg(test)]
pub(super) use beekeeper_agent_management::authority::{
    decode_authority_acceptance_receipt, project_receipt_backed_authority_chain,
};
pub(super) use beekeeper_agent_management::authority::{
    ProjectedAuthority, AUTHORITY_ACCEPTANCE_RECEIPT_TYPE,
};
pub(super) async fn fetch_trusted_relay_self(client: &BeekeeperClient) -> Result<String, CliError> {
    beekeeper_agent_management::authority::fetch_trusted_relay_self(client)
        .await
        .map_err(Into::into)
}
pub(super) async fn fetch_projected_authority(
    client: &BeekeeperClient,
    channel: &str,
    genesis: &str,
    founder: &str,
) -> Result<ProjectedAuthority, CliError> {
    beekeeper_agent_management::authority::fetch_projected_authority(
        client, channel, genesis, founder,
    )
    .await
    .map_err(Into::into)
}
pub(super) async fn fetch_projected_authority_with(
    client: &BeekeeperClient,
    channel: &str,
    genesis: &str,
    founder: &str,
    vocabulary: &RoleVocabulary,
) -> Result<ProjectedAuthority, CliError> {
    beekeeper_agent_management::authority::fetch_projected_authority_with(
        client, channel, genesis, founder, vocabulary,
    )
    .await
    .map_err(Into::into)
}
