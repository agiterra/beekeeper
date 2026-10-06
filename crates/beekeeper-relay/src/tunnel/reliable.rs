//! Reliable-stream tunnel routing for berd ↔ goose-server sessions.
//!
//! This module owns the relay-side session decision for `Profile::ReliableStream`:
//! first join acquires the Redis fenced lease and becomes the owner runtime;
//! later joins on the owner stay local, while later joins on other runtimes open
//! a fenced mesh bi-stream to the owner. The caller remains responsible for the
//! client-facing WebSocket/bridge bytes; this module supplies the routing and
//! mesh-frame discipline shared by that handler.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use beekeeper_core::CommunityId;
use beekeeper_relay_mesh::{
    BoxFuture, FencedHeader, GoodbyeReason, MeshError, MeshStream, MeshStreamFrame, Profile,
    RelayPeerTransport, RuntimeId, StreamHello, StreamRecvHalf, StreamRole, StreamSendHalf,
};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::directory::{
    AcquireResult, DirectoryError, ReleaseResult, RenewResult, SessionDirectory, SessionLease,
};

/// Maximum reliable-stream application chunk per mesh frame.
///
/// The wire cap is 16 MiB (`beekeeper_relay_mesh::wire::MAX_STREAM_FRAME`), but using
/// 1 MiB chunks keeps per-frame allocations bounded and comfortably supports
/// goose's 50 MiB HTTP bodies by streaming multiple ordered frames.
pub const MAX_RELIABLE_PAYLOAD_BYTES: usize = 1024 * 1024;

/// Renewal cadence for the default 30s session lease.
const DEFAULT_RENEW_INTERVAL: Duration = Duration::from_secs(10);

/// Relay-side router for reliable tunnel joins.
#[derive(Clone)]
pub struct ReliableStreamRouter<T: ?Sized> {
    directory: SessionDirectory,
    transport: Arc<T>,
    local_runtime_id: RuntimeId,
}

impl<T> ReliableStreamRouter<T>
where
    T: RelayPeerTransport + ?Sized,
{
    /// Create a reliable-stream router from the fenced directory, mesh transport,
    /// and this process's boot-unique runtime id.
    pub fn new(
        directory: SessionDirectory,
        transport: Arc<T>,
        local_runtime_id: RuntimeId,
    ) -> Self {
        Self {
            directory,
            transport,
            local_runtime_id,
        }
    }

    /// Return the fenced session directory used by this router.
    pub fn directory(&self) -> &SessionDirectory {
        &self.directory
    }

    /// Return this process's local mesh runtime id.
    pub fn local_runtime_id(&self) -> RuntimeId {
        self.local_runtime_id
    }

    /// Join a reliable-stream session from a client connected to this runtime.
    ///
    /// If no lease exists, this runtime becomes the owner. If the owner is this
    /// runtime, the caller should pair the client locally with the owner-side
    /// session hub. If another runtime owns the session, this opens a reliable
    /// mesh bi-stream to that owner and sends the required `Hello` first.
    pub async fn join(
        &self,
        community_id: CommunityId,
        session_id: Uuid,
    ) -> Result<ReliableJoin, ReliableStreamError> {
        let lease = match self
            .directory
            .acquire(
                community_id,
                session_id,
                self.local_runtime_id,
                Profile::ReliableStream,
            )
            .await?
        {
            AcquireResult::Acquired(lease) => return Ok(ReliableJoin::Owned { lease }),
            AcquireResult::Exists(lease) => lease,
        };

        if lease.profile != Profile::ReliableStream {
            return Err(ReliableStreamError::ProfileMismatch {
                session_id,
                expected: Profile::ReliableStream,
                actual: lease.profile,
            });
        }

        if lease.owner_runtime_id == self.local_runtime_id {
            return Ok(ReliableJoin::Owned { lease });
        }

        let fenced = lease.fenced_header();
        let hello = StreamHello {
            sender: self.local_runtime_id,
            role: StreamRole::Session {
                fenced,
                profile: Profile::ReliableStream,
            },
        };
        let stream = self
            .transport
            .open_session_stream(lease.owner_runtime_id, hello)
            .await?;

        Ok(ReliableJoin::Forwarded {
            lease,
            stream: ReliableMeshStream::new(fenced, stream),
        })
    }

    /// Accept an inbound mesh session stream opened by a non-owner runtime.
    ///
    /// The transport layer has already decoded the stream's first `Hello`; this
    /// validates that it is a reliable-stream session, that the claimed sender
    /// matches the authenticated peer, and that the fenced owner is local. Redis
    /// validation starts on the first stateful reliable frame, which carries the
    /// tenant community and is checked before payload delivery.
    pub async fn accept_inbound(
        &self,
        from: RuntimeId,
        hello: StreamHello,
        stream: MeshStream,
    ) -> Result<ReliableInbound, ReliableStreamError> {
        if hello.sender != from {
            return Err(ReliableStreamError::SenderMismatch {
                peer: from,
                hello_sender: hello.sender,
            });
        }

        let StreamRole::Session { fenced, profile } = hello.role else {
            return Err(ReliableStreamError::UnexpectedStreamRole);
        };
        if profile != Profile::ReliableStream {
            return Err(ReliableStreamError::ProfileMismatch {
                session_id: fenced.session_id,
                expected: Profile::ReliableStream,
                actual: profile,
            });
        }

        if fenced.owner_runtime_id != self.local_runtime_id {
            return Err(ReliableStreamError::OwnerIsNotLocal {
                session_id: fenced.session_id,
                owner_runtime_id: fenced.owner_runtime_id,
                local_runtime_id: self.local_runtime_id,
            });
        }

        Ok(ReliableInbound {
            fenced,
            from,
            stream: ReliableMeshStream::new_inbound(fenced, stream),
        })
    }

    /// Start background lease renewal for an owner-side session.
    ///
    /// Losing the fenced lease is fail-loud: the worker exits after logging; the
    /// caller should also validate/write through the fenced stream boundary and
    /// close clients when the session layer observes loss.
    pub fn spawn_renewer(&self, lease: SessionLease, cancel: CancellationToken) -> JoinHandle<()> {
        self.spawn_observable_renewer(lease, cancel).task
    }

    /// Start background lease renewal and return a loss signal consumers can
    /// observe.
    ///
    /// `lost` is cancelled when this runtime loses ownership or Redis renewal
    /// fails, so session consumers can tear down local state (`Room`, generation
    /// floors, client bridges). Caller-initiated `cancel` is treated as normal
    /// shutdown and does not trip the loss signal.
    pub fn spawn_observable_renewer(
        &self,
        lease: SessionLease,
        cancel: CancellationToken,
    ) -> ReliableLeaseRenewer {
        spawn_lease_renewer(self.directory.clone(), lease, cancel)
    }
}

/// Background lease renewer plus an observable ownership-loss signal.
pub struct ReliableLeaseRenewer {
    /// Worker task. Await during teardown if the caller needs release completion.
    pub task: JoinHandle<()>,
    /// Cancelled when renewal observes loss/NotOwner or a renewal error.
    pub lost: CancellationToken,
}

/// Result of a local client joining a reliable tunnel session.
pub enum ReliableJoin {
    /// This runtime owns the fenced session. Pair the client locally and renew
    /// `lease` for the life of the owner-side session.
    Owned {
        /// Fenced ownership lease acquired by this runtime.
        lease: SessionLease,
    },
    /// Another runtime owns the session. Pump client bytes through `stream`.
    Forwarded {
        /// Current owner lease read from Redis.
        lease: SessionLease,
        /// Reliable mesh stream opened to the owner runtime.
        stream: ReliableMeshStream,
    },
}

/// Inbound non-owner stream accepted by the owner runtime.
pub struct ReliableInbound {
    /// Fenced session tuple validated for this inbound stream.
    pub fenced: FencedHeader,
    /// Authenticated peer runtime that opened the stream.
    pub from: RuntimeId,
    /// Reliable stream wrapper pinned to `fenced`.
    pub stream: ReliableMeshStream,
}

/// Frames the reader task may hold ahead of the caller. Bounded so a slow
/// consumer parks the reader instead of letting the wire buffer without limit;
/// the task waits for room and never drops a frame to make it.
const READER_QUEUE_FRAMES: usize = 1;

/// A reliable mesh stream pinned to one fenced session.
///
/// The receive half is owned by a task, never by a caller's future, so no
/// consumer can cancel a wire read or a fence check. See
/// [`ReliableMeshStream::recv_validated`].
pub struct ReliableMeshStream {
    fenced: FencedHeader,
    send: Box<dyn StreamSendHalf>,
    /// Community latched by the first stateful frame in either direction.
    /// Shared with the reader task, which validates inbound frames against it.
    community_id: Arc<OnceLock<CommunityId>>,
    reader: Reader,
}

/// Receive side of a reliable stream.
enum Reader {
    /// No receive yet, so the recv half waits here. `None` only while the
    /// reader task is being spawned out of it.
    Idle(Option<Box<dyn StreamRecvHalf>>),
    /// The task owns the wire; validated frames — and the one terminal error —
    /// arrive in order on `rx`.
    Running {
        rx: mpsc::Receiver<Result<ReliableFrame, ReliableStreamError>>,
        task: JoinHandle<()>,
    },
}

/// Everything the reader task needs to validate one frame.
struct ReaderContext {
    fenced: FencedHeader,
    community_id: Arc<OnceLock<CommunityId>>,
    fence: Arc<dyn FrameFence>,
}

impl ReliableMeshStream {
    /// Wrap a raw mesh stream for one fenced reliable session.
    pub fn new(fenced: FencedHeader, stream: MeshStream) -> Self {
        let (send, recv) = stream.into_halves();
        Self {
            fenced,
            send,
            community_id: Arc::new(OnceLock::new()),
            reader: Reader::Idle(Some(recv)),
        }
    }

    /// Wrap a raw inbound mesh stream. The community is latched from the first
    /// stateful reliable frame before any payload is delivered.
    pub fn new_inbound(fenced: FencedHeader, stream: MeshStream) -> Self {
        Self::new(fenced, stream)
    }

    /// Return the community/tenant latched from the first stateful frame.
    pub fn community_id(&self) -> Option<CommunityId> {
        self.community_id.get().copied()
    }

    /// Pin this outbound stream to a community before sending payload frames.
    pub fn with_community(self, community_id: CommunityId) -> Self {
        let _ = self.community_id.set(community_id);
        self
    }

    /// Return the fenced tuple pinned to this stream.
    pub fn fenced(&self) -> FencedHeader {
        self.fenced
    }

    /// Send bytes as one or more ordered mesh `Data` frames.
    pub async fn send_bytes(
        &mut self,
        community_id: CommunityId,
        bytes: &[u8],
    ) -> Result<(), ReliableStreamError> {
        self.ensure_outbound_community(community_id)?;
        for chunk in bytes.chunks(MAX_RELIABLE_PAYLOAD_BYTES) {
            let payload = ReliableWireFrame::Data {
                community_id,
                payload: chunk.to_vec(),
            }
            .encode();
            self.send
                .send_frame(MeshStreamFrame::Data {
                    fenced: self.fenced,
                    payload,
                })
                .await?;
        }
        Ok(())
    }

    /// Finish the send half without fabricating a session frame. Used when an
    /// owner-side stream is draining before the first validated frame has
    /// latched its community id.
    pub fn finish(&mut self) -> Result<(), ReliableStreamError> {
        self.send.finish()?;
        Ok(())
    }

    /// Send a clean reliable-session close frame and finish the send half.
    pub async fn send_goodbye(
        &mut self,
        community_id: CommunityId,
        reason: GoodbyeReason,
    ) -> Result<(), ReliableStreamError> {
        self.ensure_outbound_community(community_id)?;
        let payload = ReliableWireFrame::Goodbye {
            community_id,
            reason,
        }
        .encode();
        self.send
            .send_frame(MeshStreamFrame::Data {
                fenced: self.fenced,
                payload,
            })
            .await?;
        self.send.finish()?;
        Ok(())
    }

    /// Receive and validate the next session frame.
    ///
    /// Every incoming `Data`/`Goodbye` frame is checked against both the
    /// stream's pinned fenced tuple and the Redis directory. This is the
    /// reliable-stream equivalent of Dawn's hot-path media floor, but
    /// authoritative: stale or mismatched frames fail the session rather than
    /// being dropped silently. Such a frame ends the stream — its error is
    /// delivered once, and later receives report end of stream.
    ///
    /// The first call spawns the reader task with a clone of `directory`, and
    /// that clone fences every frame for the life of the stream; later calls
    /// pass a directory handle that is only used if the task is not up yet.
    ///
    /// # Cancellation
    ///
    /// Cancel-safe at every await point. The wire read, the decode and the
    /// directory round-trip all run in a task that owns the receive half, and
    /// the only thing this future awaits is a bounded channel, so dropping it
    /// loses nothing and restarts nothing: the frame in flight is still read,
    /// still fenced exactly once, and delivered to the next caller in order.
    /// That is what lets consumers put this call in a `tokio::select!` (see
    /// `mesh_boot::run_demo_echo`, which races it against a drain tick).
    pub async fn recv_validated(
        &mut self,
        directory: &SessionDirectory,
    ) -> Result<Option<ReliableFrame>, ReliableStreamError> {
        self.recv_next(|| -> Arc<dyn FrameFence> { Arc::new(directory.clone()) })
            .await
    }

    /// `recv_validated` over any fence check. Tests pass a fence whose await
    /// point they control; production always goes through `recv_validated`.
    #[cfg(test)]
    async fn recv_validated_with(
        &mut self,
        fence: Arc<dyn FrameFence>,
    ) -> Result<Option<ReliableFrame>, ReliableStreamError> {
        self.recv_next(move || fence).await
    }

    async fn recv_next(
        &mut self,
        fence: impl FnOnce() -> Arc<dyn FrameFence>,
    ) -> Result<Option<ReliableFrame>, ReliableStreamError> {
        self.start_reader(fence);
        let Reader::Running { rx, .. } = &mut self.reader else {
            // Unreachable: `start_reader` always leaves the reader running.
            // Reported as end of stream rather than panicking on it.
            return Ok(None);
        };
        // The one await a caller can cancel, and `Receiver::recv` is
        // cancel-safe: a dropped future leaves the frame queued.
        match rx.recv().await {
            Some(result) => result.map(Some),
            None => Ok(None),
        }
    }

    /// Move the receive half into its own task on the first receive. Later
    /// calls are a no-op, so the wire has exactly one reader.
    fn start_reader(&mut self, fence: impl FnOnce() -> Arc<dyn FrameFence>) {
        let Reader::Idle(slot) = &mut self.reader else {
            return;
        };
        let Some(recv) = slot.take() else {
            return;
        };
        let (tx, rx) = mpsc::channel(READER_QUEUE_FRAMES);
        let context = ReaderContext {
            fenced: self.fenced,
            community_id: Arc::clone(&self.community_id),
            fence: fence(),
        };
        let task = tokio::spawn(run_reader(recv, context, tx));
        self.reader = Reader::Running { rx, task };
    }

    fn ensure_outbound_community(
        &mut self,
        community_id: CommunityId,
    ) -> Result<(), ReliableStreamError> {
        latch_community(&self.community_id, community_id)
    }
}

impl Drop for ReliableMeshStream {
    fn drop(&mut self) {
        // The reader owns the receive half; dropping the stream retires it.
        if let Reader::Running { task, .. } = &self.reader {
            task.abort();
        }
    }
}

/// Own the receive half for the life of the stream: read, decode, fence, and
/// hand each validated frame to the caller in order. Nothing in here is ever
/// cancelled by a consumer, which is what makes `recv_validated` cancel-safe.
async fn run_reader(
    mut recv: Box<dyn StreamRecvHalf>,
    context: ReaderContext,
    tx: mpsc::Sender<Result<ReliableFrame, ReliableStreamError>>,
) {
    loop {
        match read_validated(&mut recv, &context).await {
            // Clean end of stream: dropping `tx` reports it to the caller.
            Ok(None) => return,
            // Parks while the caller is behind; it never drops a frame.
            Ok(Some(frame)) => {
                if tx.send(Ok(frame)).await.is_err() {
                    return;
                }
            }
            // A frame the fence rejects fails the session, so stop reading.
            Err(error) => {
                let _ = tx.send(Err(error)).await;
                return;
            }
        }
    }
}

/// Read one frame off the wire and fence it. Exactly one fence check per
/// frame, because the frame's owner is the task, not a receive attempt.
async fn read_validated(
    recv: &mut Box<dyn StreamRecvHalf>,
    context: &ReaderContext,
) -> Result<Option<ReliableFrame>, ReliableStreamError> {
    let Some(frame) = recv.recv_frame().await? else {
        return Ok(None);
    };
    let (fenced, payload) = match frame {
        MeshStreamFrame::Data { fenced, payload } => (fenced, payload),
        MeshStreamFrame::Goodbye { .. } => {
            return Err(ReliableStreamError::UnexpectedFrame("goodbye"));
        }
        MeshStreamFrame::Hello(_) => {
            return Err(ReliableStreamError::UnexpectedFrame("hello"));
        }
        MeshStreamFrame::Gossip { .. } => {
            return Err(ReliableStreamError::UnexpectedFrame("gossip"));
        }
    };
    let wire = ReliableWireFrame::decode(&payload)?;
    let community_id = wire.community_id();
    validate_frame_fence(context, community_id, fenced).await?;
    Ok(Some(match wire {
        ReliableWireFrame::Data { payload, .. } => ReliableFrame::Data(payload),
        ReliableWireFrame::Goodbye { reason, .. } => ReliableFrame::Goodbye(reason),
    }))
}

async fn validate_frame_fence(
    context: &ReaderContext,
    community_id: CommunityId,
    fenced: FencedHeader,
) -> Result<(), ReliableStreamError> {
    if fenced != context.fenced {
        return Err(ReliableStreamError::FrameFenceMismatch {
            expected: context.fenced,
            actual: fenced,
        });
    }
    if let Some(expected) = context.community_id.get().copied() {
        if expected != community_id {
            return Err(ReliableStreamError::CommunityMismatch {
                expected,
                actual: community_id,
            });
        }
    }
    context.fence.validate_frame(community_id, fenced).await?;
    latch_community(&context.community_id, community_id)
}

/// Latch the stream's community on first use and reject any later mismatch.
/// One atomic decides for both directions, so a send and an inbound frame
/// cannot latch different communities.
fn latch_community(
    latched: &OnceLock<CommunityId>,
    community_id: CommunityId,
) -> Result<(), ReliableStreamError> {
    let expected = *latched.get_or_init(|| community_id);
    if expected != community_id {
        return Err(ReliableStreamError::CommunityMismatch {
            expected,
            actual: community_id,
        });
    }
    Ok(())
}

/// The per-frame fence check applied by the reader task.
///
/// Production always uses the Redis [`SessionDirectory`]. The trait exists so
/// tests can own the await point between the wire read and the directory
/// round-trip — the window a cancelled receive must survive.
trait FrameFence: Send + Sync + 'static {
    /// Validate one frame's fenced tuple for `community_id`.
    fn validate_frame(
        &self,
        community_id: CommunityId,
        fenced: FencedHeader,
    ) -> BoxFuture<'_, Result<(), MeshError>>;
}

impl FrameFence for SessionDirectory {
    fn validate_frame(
        &self,
        community_id: CommunityId,
        fenced: FencedHeader,
    ) -> BoxFuture<'_, Result<(), MeshError>> {
        Box::pin(async move { self.validate_fenced_header(community_id, &fenced).await })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ReliableWireFrame {
    Data {
        community_id: CommunityId,
        payload: Vec<u8>,
    },
    Goodbye {
        community_id: CommunityId,
        reason: GoodbyeReason,
    },
}

impl ReliableWireFrame {
    const VERSION: u8 = 1;
    const DATA: u8 = 1;
    const GOODBYE: u8 = 2;

    fn community_id(&self) -> CommunityId {
        match self {
            Self::Data { community_id, .. } | Self::Goodbye { community_id, .. } => *community_id,
        }
    }

    fn encode(&self) -> Vec<u8> {
        match self {
            Self::Data {
                community_id,
                payload,
            } => {
                let mut encoded = Vec::with_capacity(18 + payload.len());
                encoded.push(Self::VERSION);
                encoded.push(Self::DATA);
                encoded.extend_from_slice(community_id.as_uuid().as_bytes());
                encoded.extend_from_slice(payload);
                encoded
            }
            Self::Goodbye {
                community_id,
                reason,
            } => {
                let mut encoded = Vec::with_capacity(19);
                encoded.push(Self::VERSION);
                encoded.push(Self::GOODBYE);
                encoded.extend_from_slice(community_id.as_uuid().as_bytes());
                encoded.push(reason.to_wire_byte());
                encoded
            }
        }
    }

    fn decode(bytes: &[u8]) -> Result<Self, ReliableStreamError> {
        if bytes.len() < 18 {
            return Err(ReliableStreamError::MalformedReliableFrame("too short"));
        }
        if bytes[0] != Self::VERSION {
            return Err(ReliableStreamError::MalformedReliableFrame(
                "unknown version",
            ));
        }
        let community_id = CommunityId::from_uuid(Uuid::from_bytes(
            bytes[2..18].try_into().expect("16 byte community id slice"),
        ));
        match bytes[1] {
            Self::DATA => Ok(Self::Data {
                community_id,
                payload: bytes[18..].to_vec(),
            }),
            Self::GOODBYE => {
                if bytes.len() != 19 {
                    return Err(ReliableStreamError::MalformedReliableFrame(
                        "bad goodbye length",
                    ));
                }
                let reason = <GoodbyeReason as GoodbyeReasonWireExt>::from_wire_byte(bytes[18])?;
                Ok(Self::Goodbye {
                    community_id,
                    reason,
                })
            }
            _ => Err(ReliableStreamError::MalformedReliableFrame("unknown kind")),
        }
    }
}

trait GoodbyeReasonWireExt {
    fn to_wire_byte(self) -> u8;
    fn from_wire_byte(byte: u8) -> Result<GoodbyeReason, ReliableStreamError>;
}

impl GoodbyeReasonWireExt for GoodbyeReason {
    fn to_wire_byte(self) -> u8 {
        match self {
            GoodbyeReason::SessionEnded => 1,
            GoodbyeReason::Draining => 2,
            GoodbyeReason::StaleGeneration => 3,
        }
    }

    fn from_wire_byte(byte: u8) -> Result<GoodbyeReason, ReliableStreamError> {
        match byte {
            1 => Ok(GoodbyeReason::SessionEnded),
            2 => Ok(GoodbyeReason::Draining),
            3 => Ok(GoodbyeReason::StaleGeneration),
            _ => Err(ReliableStreamError::MalformedReliableFrame(
                "unknown goodbye reason",
            )),
        }
    }
}

/// Validated frame from a reliable mesh stream.
#[derive(Debug, PartialEq, Eq)]
pub enum ReliableFrame {
    /// Ordered opaque bytes from the remote tunnel endpoint.
    Data(Vec<u8>),
    /// Clean session close with a typed reason.
    Goodbye(GoodbyeReason),
}

#[derive(Debug, thiserror::Error)]
#[allow(missing_docs)]
pub enum ReliableStreamError {
    #[error("session directory: {0}")]
    Directory(#[from] DirectoryError),
    #[error(transparent)]
    Mesh(#[from] MeshError),
    #[error("profile mismatch for session {session_id}: expected {expected:?}, got {actual:?}")]
    ProfileMismatch {
        session_id: Uuid,
        expected: Profile,
        actual: Profile,
    },
    #[error("stream hello sender {hello_sender} does not match authenticated peer {peer}")]
    SenderMismatch {
        peer: RuntimeId,
        hello_sender: RuntimeId,
    },
    #[error("unexpected non-session stream role")]
    UnexpectedStreamRole,
    #[error("reliable stream owner for session {session_id} is {owner_runtime_id}, not local runtime {local_runtime_id}")]
    OwnerIsNotLocal {
        session_id: Uuid,
        owner_runtime_id: RuntimeId,
        local_runtime_id: RuntimeId,
    },
    #[error("unexpected {0} frame on reliable session stream")]
    UnexpectedFrame(&'static str),
    #[error("frame fence mismatch: expected {expected:?}, got {actual:?}")]
    FrameFenceMismatch {
        expected: FencedHeader,
        actual: FencedHeader,
    },
    #[error("community mismatch on reliable stream: expected {expected}, got {actual}")]
    CommunityMismatch {
        expected: CommunityId,
        actual: CommunityId,
    },
    #[error("malformed reliable frame: {0}")]
    MalformedReliableFrame(&'static str),
}

fn spawn_lease_renewer(
    directory: SessionDirectory,
    lease: SessionLease,
    cancel: CancellationToken,
) -> ReliableLeaseRenewer {
    spawn_lease_renewer_with_interval(directory, lease, cancel, DEFAULT_RENEW_INTERVAL)
}

fn spawn_lease_renewer_with_interval(
    directory: SessionDirectory,
    lease: SessionLease,
    cancel: CancellationToken,
    renew_interval: Duration,
) -> ReliableLeaseRenewer {
    let lost = CancellationToken::new();
    let lost_for_task = lost.clone();
    let task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(renew_interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let caller_cancelled = loop {
            tokio::select! {
                _ = cancel.cancelled() => break true,
                _ = interval.tick() => {
                    match directory.renew(&lease).await {
                        Ok(RenewResult::Renewed(_)) => {}
                        Ok(RenewResult::Lost { current, known_generation }) => {
                            tracing::warn!(
                                session_id = %lease.session_id,
                                owner_runtime_id = %lease.owner_runtime_id,
                                generation = lease.generation,
                                ?current,
                                ?known_generation,
                                "reliable tunnel lease renewal lost"
                            );
                            lost_for_task.cancel();
                            break false;
                        }
                        Err(err) => {
                            tracing::warn!(
                                session_id = %lease.session_id,
                                owner_runtime_id = %lease.owner_runtime_id,
                                generation = lease.generation,
                                error = %err,
                                "reliable tunnel lease renewal failed"
                            );
                            lost_for_task.cancel();
                            break false;
                        }
                    }
                }
            }
        };

        match directory.release(&lease).await {
            Ok(ReleaseResult::Released(_)) => {}
            Ok(ReleaseResult::NotOwner {
                current,
                known_generation,
            }) => {
                tracing::warn!(
                    session_id = %lease.session_id,
                    owner_runtime_id = %lease.owner_runtime_id,
                    generation = lease.generation,
                    ?current,
                    ?known_generation,
                    "reliable tunnel lease release found non-owner"
                );
                lost_for_task.cancel();
            }
            Err(err) => {
                tracing::warn!(
                    session_id = %lease.session_id,
                    owner_runtime_id = %lease.owner_runtime_id,
                    generation = lease.generation,
                    error = %err,
                    "reliable tunnel lease release failed"
                );
                if !caller_cancelled {
                    lost_for_task.cancel();
                }
            }
        }
    });

    ReliableLeaseRenewer { task, lost }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;

    use beekeeper_relay_mesh::endpoint::MeshEndpoint;
    use beekeeper_relay_mesh::{InboundHandler, MeshDatagram, PeerInfo};

    use super::*;

    fn community() -> CommunityId {
        CommunityId::from_uuid(Uuid::from_u128(0xCAFE))
    }

    fn runtime(byte: u8) -> RuntimeId {
        RuntimeId([byte; 32])
    }

    fn pool() -> deadpool_redis::Pool {
        let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
        deadpool_redis::Config::from_url(url)
            .create_pool(Some(deadpool_redis::Runtime::Tokio1))
            .expect("create redis pool")
    }

    /// The Redis-backed directory, or `None` when Redis is unreachable.
    ///
    /// Callers return early on `None`, which means the test reports **passed**
    /// without asserting anything. That is tolerable on a dev machine with no
    /// Docker, but in a run summary it is indistinguishable from real coverage,
    /// so the skip is announced on stderr and — where Redis is supposed to be
    /// present — turned into a failure.
    ///
    /// Set `BUZZ_TEST_REQUIRE_REDIS=1` anywhere Redis is a declared service
    /// (`.woodpecker/gate.yml` does) so a missing or broken service can never
    /// again be read as a green run.
    async fn redis_directory_if_available() -> Option<SessionDirectory> {
        fn unavailable(why: &str) -> Option<SessionDirectory> {
            assert!(
                !std::env::var("BUZZ_TEST_REQUIRE_REDIS").is_ok_and(|v| v != "0"),
                "BUZZ_TEST_REQUIRE_REDIS is set but Redis is unusable ({why}); \
                 refusing to skip and report a pass"
            );
            eprintln!("SKIP: {} needs Redis ({why})", module_path!());
            None
        }
        let pool = pool();
        let mut conn = match pool.get().await {
            Ok(conn) => conn,
            Err(e) => return unavailable(&format!("pool: {e}")),
        };
        if let Err(e) = redis::cmd("PING").query_async::<String>(&mut *conn).await {
            return unavailable(&format!("ping: {e}"));
        }
        Some(SessionDirectory::with_lease_ttl(
            pool,
            Duration::from_millis(500),
        ))
    }

    async fn clear_keys(directory: &SessionDirectory, community_id: CommunityId, session_id: Uuid) {
        let base = format!("buzz:{}:tunnel:{}", community_id, session_id);
        let _ = directory
            .release(&SessionLease {
                community_id,
                session_id,
                owner_runtime_id: runtime(1),
                generation: 1,
                profile: Profile::ReliableStream,
            })
            .await;
        let mut conn = pool().get().await.expect("redis conn");
        let _: () = redis::cmd("DEL")
            .arg(format!("{base}:lease"))
            .arg(format!("{base}:generation"))
            .query_async(&mut *conn)
            .await
            .expect("clear keys");
    }

    struct NoopTransport;

    impl RelayPeerTransport for NoopTransport {
        fn send_datagram(&self, _to: RuntimeId, _dgram: MeshDatagram) -> Result<(), MeshError> {
            unreachable!("reliable tests do not send datagrams")
        }

        fn open_session_stream(
            &self,
            _to: RuntimeId,
            _hello: StreamHello,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<MeshStream, MeshError>> + Send + '_>,
        > {
            Box::pin(async { Err(MeshError::Transport("unexpected open".into())) })
        }

        fn set_inbound(&self, _handler: Box<dyn InboundHandler>) {}
    }

    #[tokio::test]
    async fn first_join_acquires_local_ownership() {
        let Some(directory) = redis_directory_if_available().await else {
            return;
        };
        let community_id = community();
        let session_id = Uuid::new_v4();
        clear_keys(&directory, community_id, session_id).await;

        let router = ReliableStreamRouter::new(directory, Arc::new(NoopTransport), runtime(1));
        let join = router.join(community_id, session_id).await.unwrap();
        let ReliableJoin::Owned { lease } = join else {
            panic!("first join owns locally")
        };
        assert_eq!(lease.owner_runtime_id, runtime(1));
        assert_eq!(lease.profile, Profile::ReliableStream);
    }

    struct DirectTransport {
        peer: beekeeper_relay_mesh::peer::MeshPeer,
        opened: Mutex<Vec<(RuntimeId, StreamHello)>>,
    }

    impl RelayPeerTransport for DirectTransport {
        fn send_datagram(&self, _to: RuntimeId, _dgram: MeshDatagram) -> Result<(), MeshError> {
            unreachable!("reliable tests do not send datagrams")
        }

        fn open_session_stream(
            &self,
            to: RuntimeId,
            hello: StreamHello,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<MeshStream, MeshError>> + Send + '_>,
        > {
            Box::pin(async move {
                self.opened.lock().unwrap().push((to, hello.clone()));
                let mut stream = self.peer.open_bi().await?;
                stream.send_frame(MeshStreamFrame::Hello(hello)).await?;
                Ok(stream)
            })
        }

        fn set_inbound(&self, _handler: Box<dyn InboundHandler>) {}
    }

    async fn endpoint_pair() -> (MeshEndpoint, MeshEndpoint) {
        let bind = || "127.0.0.1:0".parse().unwrap();
        let a = MeshEndpoint::bind(bind()).await.unwrap();
        let b = MeshEndpoint::bind(bind()).await.unwrap();
        (a, b)
    }

    #[tokio::test]
    async fn later_join_routes_to_remote_owner_with_reliable_hello() {
        let Some(directory) = redis_directory_if_available().await else {
            return;
        };
        let community_id = community();
        let session_id = Uuid::new_v4();
        clear_keys(&directory, community_id, session_id).await;

        let (local_endpoint, owner_endpoint) = endpoint_pair().await;
        let local_runtime = local_endpoint.runtime_id();
        let owner_runtime = owner_endpoint.runtime_id();
        let owner_addr = owner_endpoint.addr();
        let accept_endpoint = owner_endpoint.clone();
        let accept = tokio::spawn(async move { accept_endpoint.accept().await.unwrap().unwrap() });
        let local_peer = local_endpoint.connect(owner_addr).await.unwrap();
        let owner_peer = accept.await.unwrap();

        let owner_lease = match directory
            .acquire(
                community_id,
                session_id,
                owner_runtime,
                Profile::ReliableStream,
            )
            .await
            .unwrap()
        {
            AcquireResult::Acquired(lease) => lease,
            AcquireResult::Exists(_) => panic!("fresh session should acquire"),
        };

        let transport = Arc::new(DirectTransport {
            peer: local_peer,
            opened: Mutex::new(Vec::new()),
        });
        let router = ReliableStreamRouter::new(directory.clone(), transport, local_runtime);

        let recv_hello = tokio::spawn(async move {
            let mut stream = owner_peer.accept_bi().await.unwrap();
            stream.recv_frame().await.unwrap().unwrap()
        });

        let join = router.join(community_id, session_id).await.unwrap();
        let ReliableJoin::Forwarded { lease, .. } = join else {
            panic!("second runtime should forward")
        };
        assert_eq!(lease, owner_lease);

        let got = tokio::time::timeout(Duration::from_secs(5), recv_hello)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            got,
            MeshStreamFrame::Hello(StreamHello {
                sender: local_runtime,
                role: StreamRole::Session {
                    fenced: owner_lease.fenced_header(),
                    profile: Profile::ReliableStream,
                },
            })
        );
    }

    #[tokio::test]
    async fn observable_renewer_signals_loss_when_lease_disappears() {
        let Some(directory) = redis_directory_if_available().await else {
            return;
        };
        let community_id = community();
        let session_id = Uuid::new_v4();
        clear_keys(&directory, community_id, session_id).await;
        let lease = match directory
            .acquire(
                community_id,
                session_id,
                runtime(1),
                Profile::ReliableStream,
            )
            .await
            .unwrap()
        {
            AcquireResult::Acquired(lease) => lease,
            AcquireResult::Exists(_) => panic!("fresh session should acquire"),
        };

        let cancel = CancellationToken::new();
        let renewer = spawn_lease_renewer_with_interval(
            directory.clone(),
            lease.clone(),
            cancel,
            Duration::from_millis(10),
        );
        directory.release(&lease).await.unwrap();

        tokio::time::timeout(Duration::from_secs(2), renewer.lost.cancelled())
            .await
            .expect("lost token is cancelled after ownership loss");
        renewer.task.await.unwrap();
    }

    #[tokio::test]
    async fn observable_renewer_normal_cancel_does_not_signal_loss() {
        let Some(directory) = redis_directory_if_available().await else {
            return;
        };
        let community_id = community();
        let session_id = Uuid::new_v4();
        clear_keys(&directory, community_id, session_id).await;
        let lease = match directory
            .acquire(
                community_id,
                session_id,
                runtime(1),
                Profile::ReliableStream,
            )
            .await
            .unwrap()
        {
            AcquireResult::Acquired(lease) => lease,
            AcquireResult::Exists(_) => panic!("fresh session should acquire"),
        };

        let cancel = CancellationToken::new();
        let renewer = spawn_lease_renewer_with_interval(
            directory,
            lease,
            cancel.clone(),
            Duration::from_millis(10),
        );
        cancel.cancel();
        renewer.task.await.unwrap();
        assert!(
            !renewer.lost.is_cancelled(),
            "caller-initiated shutdown is not ownership loss"
        );
    }

    /// Recv half that yields one prepared frame and then end-of-stream, and
    /// counts how many times the wire was read.
    struct OneFrameRecv {
        frame: Option<MeshStreamFrame>,
        reads: Arc<AtomicUsize>,
    }

    impl beekeeper_relay_mesh::StreamRecvHalf for OneFrameRecv {
        fn recv_frame(&mut self) -> BoxFuture<'_, Result<Option<MeshStreamFrame>, MeshError>> {
            self.reads.fetch_add(1, Ordering::Relaxed);
            let frame = self.frame.take();
            Box::pin(async move { Ok(frame) })
        }
    }

    struct DiscardSend;

    impl beekeeper_relay_mesh::StreamSendHalf for DiscardSend {
        fn send_frame(&mut self, _frame: MeshStreamFrame) -> BoxFuture<'_, Result<(), MeshError>> {
            Box::pin(async { Ok(()) })
        }

        fn finish(&mut self) -> Result<(), MeshError> {
            Ok(())
        }
    }

    /// Fence whose first check parks on a gate the test owns, so the caller's
    /// receive can be dropped exactly in the read-then-validate window. Later
    /// checks pass immediately.
    struct GatedFence {
        gate: Arc<tokio::sync::Notify>,
        gated: AtomicBool,
        calls: Arc<AtomicUsize>,
    }

    impl FrameFence for GatedFence {
        fn validate_frame(
            &self,
            _community_id: CommunityId,
            _fenced: FencedHeader,
        ) -> BoxFuture<'_, Result<(), MeshError>> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let gate = Arc::clone(&self.gate);
            let gated = self.gated.swap(false, Ordering::Relaxed);
            Box::pin(async move {
                if gated {
                    gate.notified().await;
                }
                Ok(())
            })
        }
    }

    /// A receive cancelled between the wire read and the fence check must not
    /// destroy the frame, and must not restart the check: the next call has to
    /// deliver it without touching the wire or Redis again. This is the hazard
    /// `mesh_boot::run_demo_echo` hits when its 100 ms drain tick wins the
    /// `select!`.
    #[tokio::test(start_paused = true)]
    async fn recv_validated_keeps_the_frame_when_the_fence_check_is_cancelled() {
        let fenced = demo_fenced();
        let reads = Arc::new(AtomicUsize::new(0));
        let stream = MeshStream::new(
            Box::new(DiscardSend),
            Box::new(OneFrameRecv {
                frame: Some(echo_frame(fenced)),
                reads: Arc::clone(&reads),
            }),
        );
        let mut reliable = ReliableMeshStream::new(fenced, stream);

        let gate = Arc::new(tokio::sync::Notify::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let fence: Arc<dyn FrameFence> = Arc::new(GatedFence {
            gate: Arc::clone(&gate),
            gated: AtomicBool::new(true),
            calls: Arc::clone(&calls),
        });

        // Drop the receive future while the fence check is still pending —
        // exactly what `select!` does when the drain tick wins the race.
        let cancelled = tokio::time::timeout(
            Duration::from_millis(100),
            reliable.recv_validated_with(Arc::clone(&fence)),
        )
        .await;
        assert!(cancelled.is_err(), "fence check should still be pending");
        assert_eq!(reads.load(Ordering::Relaxed), 1, "one wire read so far");
        assert_eq!(calls.load(Ordering::Relaxed), 1, "one fence check so far");

        // The fence answers after the caller gave up on that receive.
        gate.notify_one();

        let frame = tokio::time::timeout(
            Duration::from_secs(5),
            reliable.recv_validated_with(Arc::clone(&fence)),
        )
        .await
        .expect("second receive completes")
        .expect("second receive succeeds");
        assert!(
            matches!(frame, Some(ReliableFrame::Data(ref bytes)) if bytes == b"echo me"),
            "cancelled receive lost the frame: {frame:?}"
        );
        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "the fence must run once per frame, not once per receive attempt"
        );
        assert_eq!(
            reliable
                .recv_validated_with(Arc::clone(&fence))
                .await
                .expect("end of stream"),
            None
        );
        assert_eq!(
            reads.load(Ordering::Relaxed),
            2,
            "the frame's read plus the end-of-stream read — never a re-read"
        );
    }

    /// Frame the way the QUIC recv half does: u32-LE length, then the
    /// postcard body.
    fn framed(frame: &MeshStreamFrame) -> Vec<u8> {
        let bytes = beekeeper_relay_mesh::wire::encode(frame).expect("encode mesh stream frame");
        let mut out = (bytes.len() as u32).to_le_bytes().to_vec();
        out.extend_from_slice(&bytes);
        out
    }

    /// Recv half that models quinn's `read_exact` framing over a byte queue it
    /// owns: pop the 4-byte length prefix, park once *inside* the frame (where
    /// a real `read_exact` waits for the rest of the body), then pop the body.
    /// Bytes already popped are gone if the future is dropped — the data loss
    /// `RecvStream::read_exact` documents for cancellation.
    struct MidBodyParkRecv {
        bytes: std::collections::VecDeque<u8>,
        gate: Option<Arc<tokio::sync::Notify>>,
        reads: Arc<AtomicUsize>,
    }

    impl beekeeper_relay_mesh::StreamRecvHalf for MidBodyParkRecv {
        fn recv_frame(&mut self) -> BoxFuture<'_, Result<Option<MeshStreamFrame>, MeshError>> {
            self.reads.fetch_add(1, Ordering::Relaxed);
            Box::pin(async move {
                if self.bytes.is_empty() {
                    return Ok(None);
                }
                let mut len = [0u8; 4];
                for slot in len.iter_mut() {
                    *slot = self
                        .bytes
                        .pop_front()
                        .ok_or_else(|| MeshError::Transport("short length prefix".into()))?;
                }
                if let Some(gate) = self.gate.take() {
                    gate.notified().await;
                }
                let len = u32::from_le_bytes(len) as usize;
                let max = beekeeper_relay_mesh::wire::MAX_STREAM_FRAME as usize;
                if len > max {
                    return Err(MeshError::FrameTooLarge { size: len, max });
                }
                let mut body = vec![0u8; len];
                for slot in body.iter_mut() {
                    *slot = self
                        .bytes
                        .pop_front()
                        .ok_or_else(|| MeshError::Transport("short frame body".into()))?;
                }
                beekeeper_relay_mesh::wire::decode::<MeshStreamFrame>(&body).map(Some)
            })
        }
    }

    /// Fence that always passes, counting the checks it starts.
    #[derive(Default)]
    struct PassFence {
        calls: AtomicUsize,
    }

    impl FrameFence for PassFence {
        fn validate_frame(
            &self,
            _community_id: CommunityId,
            _fenced: FencedHeader,
        ) -> BoxFuture<'_, Result<(), MeshError>> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Box::pin(async { Ok(()) })
        }
    }

    /// Fence slower than the caller's cancel period, counting the checks it
    /// starts so a per-attempt restart is visible.
    struct SlowFence {
        delay: Duration,
        calls: Arc<AtomicUsize>,
    }

    impl FrameFence for SlowFence {
        fn validate_frame(
            &self,
            _community_id: CommunityId,
            _fenced: FencedHeader,
        ) -> BoxFuture<'_, Result<(), MeshError>> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let delay = self.delay;
            Box::pin(async move {
                tokio::time::sleep(delay).await;
                Ok(())
            })
        }
    }

    fn demo_fenced() -> FencedHeader {
        FencedHeader {
            session_id: Uuid::from_u128(0x5E5_5104),
            generation: 7,
            owner_runtime_id: runtime(3),
        }
    }

    fn echo_frame(fenced: FencedHeader) -> MeshStreamFrame {
        MeshStreamFrame::Data {
            fenced,
            payload: ReliableWireFrame::Data {
                community_id: community(),
                payload: b"echo me".to_vec(),
            }
            .encode(),
        }
    }

    /// A receive cancelled while the *wire read* is parked mid-frame must not
    /// corrupt the stream: the bytes already off the wire belong to the
    /// stream, not to the caller's future, so the next receive still yields
    /// the whole frame. `mesh_boot::run_demo_echo` cancels exactly here every
    /// time its 100 ms drain tick wins the `select!`.
    #[tokio::test(start_paused = true)]
    async fn recv_validated_survives_cancellation_while_the_wire_read_is_parked() {
        let fenced = demo_fenced();
        let gate = Arc::new(tokio::sync::Notify::new());
        let reads = Arc::new(AtomicUsize::new(0));
        let stream = MeshStream::new(
            Box::new(DiscardSend),
            Box::new(MidBodyParkRecv {
                bytes: framed(&echo_frame(fenced)).into_iter().collect(),
                gate: Some(Arc::clone(&gate)),
                reads: Arc::clone(&reads),
            }),
        );
        let mut reliable = ReliableMeshStream::new(fenced, stream);
        let fence: Arc<dyn FrameFence> = Arc::new(PassFence::default());

        let cancelled = tokio::time::timeout(
            Duration::from_millis(100),
            reliable.recv_validated_with(Arc::clone(&fence)),
        )
        .await;
        assert!(cancelled.is_err(), "the wire read should still be parked");
        assert_eq!(reads.load(Ordering::Relaxed), 1, "one wire read so far");

        // The body arrives after the caller gave up on that receive.
        gate.notify_one();

        let frame = tokio::time::timeout(
            Duration::from_secs(5),
            reliable.recv_validated_with(Arc::clone(&fence)),
        )
        .await
        .expect("second receive completes")
        .expect("second receive succeeds");
        assert!(
            matches!(frame, Some(ReliableFrame::Data(ref bytes)) if bytes == b"echo me"),
            "cancelled wire read corrupted the stream: {frame:?}"
        );
        assert_eq!(
            reliable
                .recv_validated_with(Arc::clone(&fence))
                .await
                .expect("end of stream"),
            None
        );
        assert_eq!(
            reads.load(Ordering::Relaxed),
            2,
            "the frame's read plus the end-of-stream read — never a re-read"
        );
    }

    /// A fence check slower than the caller's cancel period must still finish:
    /// the check belongs to the frame, not to the receive attempt. Otherwise a
    /// degraded Redis (150 ms) against the 100 ms drain tick starves the
    /// session forever while every attempt starts a fresh round trip.
    #[tokio::test(start_paused = true)]
    async fn a_fence_slower_than_the_cancel_period_still_delivers_the_frame() {
        let fenced = demo_fenced();
        let reads = Arc::new(AtomicUsize::new(0));
        let stream = MeshStream::new(
            Box::new(DiscardSend),
            Box::new(OneFrameRecv {
                frame: Some(echo_frame(fenced)),
                reads: Arc::clone(&reads),
            }),
        );
        let mut reliable = ReliableMeshStream::new(fenced, stream);
        let calls = Arc::new(AtomicUsize::new(0));
        let fence: Arc<dyn FrameFence> = Arc::new(SlowFence {
            delay: Duration::from_millis(150),
            calls: Arc::clone(&calls),
        });

        let mut delivered = None;
        for _ in 0..10 {
            if let Ok(result) = tokio::time::timeout(
                Duration::from_millis(100),
                reliable.recv_validated_with(Arc::clone(&fence)),
            )
            .await
            {
                delivered = Some(result.expect("receive succeeds"));
                break;
            }
        }

        assert!(
            matches!(delivered, Some(Some(ReliableFrame::Data(ref bytes))) if bytes == b"echo me"),
            "a fence slower than the cancel period starved the caller: {delivered:?}"
        );
        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "the fence must run once per frame, not once per receive attempt"
        );
        assert_eq!(
            reliable
                .recv_validated_with(Arc::clone(&fence))
                .await
                .expect("end of stream"),
            None
        );
        assert_eq!(
            reads.load(Ordering::Relaxed),
            2,
            "the frame's read plus the end-of-stream read — never a re-read"
        );
    }

    #[test]
    fn reliable_wire_frame_carries_community_without_plain_payload_changes() {
        let frame = ReliableWireFrame::Data {
            community_id: community(),
            payload: b"goose bytes".to_vec(),
        };
        let encoded = frame.encode();
        let decoded = ReliableWireFrame::decode(&encoded).unwrap();
        assert_eq!(decoded, frame);
        assert_eq!(decoded.community_id(), community());
    }

    #[test]
    fn payload_chunking_covers_goose_sized_bodies() {
        let fifty_mib: usize = 50 * 1024 * 1024;
        let chunks = fifty_mib.div_ceil(MAX_RELIABLE_PAYLOAD_BYTES);
        assert_eq!(chunks, 50);
        assert!(MAX_RELIABLE_PAYLOAD_BYTES < beekeeper_relay_mesh::wire::MAX_STREAM_FRAME as usize);
    }

    #[allow(dead_code)]
    fn _peer_info_is_not_an_owner_signal(_peer: PeerInfo) {}
}
