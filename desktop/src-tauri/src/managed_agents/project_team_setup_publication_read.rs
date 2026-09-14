//! Serialized read-side reconciliation for the publication journal.

use super::*;

/// Observe the live source and persist its reconciliation as one serialized
/// journal operation. The context check stays immediately before the write so
/// a community change cannot make a read-triggered reconciliation durable.
pub(super) async fn reconciled_publication_read<F, V>(
    draft: &ProjectTeamSetupDraft,
    source: F,
    verify_before_save: V,
) -> Result<(Option<CurrentSource>, Option<PublicationJournal>), SetupError>
where
    F: std::future::Future<Output = Result<Option<CurrentSource>, SetupError>>,
    V: FnOnce() -> Result<(), SetupError>,
{
    let _guard = PUBLICATION_LOCK.lock().await;
    let source = source.await?;
    let journal = match load_journal(draft)? {
        Some(mut journal) => {
            if journal.source_event.is_some() {
                reconcile_source_observation(&mut journal, source.clone())?;
                verify_before_save()?;
                save_journal(draft, &journal)?;
            }
            Some(journal)
        }
        None => None,
    };
    Ok((source, journal))
}
