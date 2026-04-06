//! Basic runtime metrics — §12.6, §13.5.

/// In-process metrics counters (no Prometheus in MVP; exporter can be wired later).
#[derive(Default, Debug, Clone)]
pub struct Metrics {
    /// overlay_messages_dropped_total.
    pub messages_dropped: u64,
    /// Total envelopes forwarded.
    pub messages_forwarded: u64,
    /// Number of PEX rounds applied.
    pub pex_rounds: u64,
    /// Number of repair ticks executed.
    pub repair_ticks: u64,
}
