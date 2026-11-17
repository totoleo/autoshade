//! The develop preview scheduler: at most one render is in flight and the
//! latest recipe wins. Counters only — it owns no pixels.

/// The single-in-flight, latest-wins develop dispatcher's bookkeeping.
#[derive(Default)]
pub(crate) struct DevelopScheduler {
    /// One develop render is on the wire right now.
    pub(crate) inflight: bool,
    /// Accepted frames, a regression counter (latest-wins).
    pub(crate) count: u64,
}
