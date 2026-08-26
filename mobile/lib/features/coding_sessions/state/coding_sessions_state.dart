/// The coding-session observer's state layer: the Riverpod providers that read
/// a channel's signed facts off the relay and hand the domain layer's folds to
/// the UI.
///
/// Read-only by construction — nothing here publishes.
library;

export 'coding_session_event_store.dart';
export 'coding_session_observer_provider.dart';
export 'coding_session_observer_snapshot.dart';
