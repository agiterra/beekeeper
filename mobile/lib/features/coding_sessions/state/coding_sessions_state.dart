/// The coding-session observer's state layer: the Riverpod providers that read
/// a channel's signed facts off the relay and hand the domain layer's folds to
/// the UI.
///
/// The observer providers never publish. Publishing is the job of the command
/// provider that lands beside them (`coding_session_command_provider.dart`,
/// slice 2 of the 2026-09-07 mobile-interact plan), so a reader can still
/// tell which provider signed what.
library;

export 'coding_session_command_provider.dart';
export 'coding_session_event_store.dart';
export 'coding_session_observer_provider.dart';
export 'coding_session_observer_snapshot.dart';
export 'pending_creates_provider.dart';
export 'pending_turns_provider.dart';
export 'provider_catalogs_provider.dart';
