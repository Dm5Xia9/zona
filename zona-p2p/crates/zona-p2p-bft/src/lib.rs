//! BFT consensus layer for zona-p2p — §11, §12.1, §12.6, §12.7.
//!
//! This crate implements the three BFT-related concerns described in the
//! specification:
//!
//! 1. **`validator`** — `ValidatorSet` with quorum verification (§11.2, §11.7).
//! 2. **`certificate`** — `BftCertificate` produced after a consensus round
//!    (§11.5); verifiable by any node that holds the current `ValidatorSet`.
//! 3. **`epoch`** — `EpochHeader` with commitments to the next validator set
//!    and `verify_epoch_chain` for light clients (§11.10, §12.1).
//! 4. **`state`** — `BftState` on a full node: accepts certificates, rotates
//!    epochs, triggers `FINALITY_PAUSED` on divergence (§12.6).
//! 5. **`light_client`** — `LightClient` that accepts headers only after
//!    ≥ 2 independent channels or 1 channel + external checkpoint (§11.10,
//!    §12.7).
//! 6. **`error`** — `BftError` enumeration.
//!
//! ## What this crate does NOT include
//!
//! The **protocol mechanics** of a BFT round (Propose → Prepare → Commit
//! phases, leader rotation, view-change, etc.) are intentionally out of scope
//! here.  Those are transport-level concerns belonging to a concrete BFT
//! implementation (Tendermint, HotStuff, …).  This crate provides the
//! *data structures* and *verification logic* that are shared regardless of
//! the concrete protocol family chosen (§11.4).

pub mod error;
pub mod validator;
pub mod certificate;
pub mod epoch;
pub mod state;
pub mod light_client;

pub use error::BftError;
pub use validator::{ValidatorEntry, ValidatorSet};
pub use certificate::{BftCertificate, CertPayload};
pub use epoch::{EpochHeader, verify_epoch_chain};
pub use state::{BftState, FinalityStatus};
pub use light_client::{Anchor, CertObservation, LightClient};
