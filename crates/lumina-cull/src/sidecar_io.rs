//! Sidecar binding for a Stage-1 proposal (decisions §5).
//!
//! All reads/writes go through the existing `lumina-sidecar` API
//! ([`lumina_sidecar::SidecarDocument::culling`], `validate_culling_section`,
//! `load_sidecar`, `save_sidecar`). This module **never** writes `rating`,
//! `flag`, `color_label` or any recipe field, and it never touches virtual
//! copies: a proposal is source-level analysis only.
//!
//! Missing/stale states are explicit ([`CullingReadState`]) — the caller shows
//! "no proposal / outdated" instead of a guessed recommendation, and there is
//! no automatic re-computation.

use std::path::Path;

use log::info;
use lumina_sidecar::{
    load_sidecar, save_sidecar, validate_culling_section, CullingIdentity, CullingSection,
    CullingStatus, SidecarDocument,
};

use crate::identity::{identity_mismatches, IdentityMismatch};
use crate::CullError;

/// Explicit read state of a source-level culling proposal.
#[derive(Debug, Clone, PartialEq)]
pub enum CullingReadState {
    /// No `culling` section: the valid "no proposal" state (not an error).
    NoProposal,
    /// A usable proposal whose identity matches the current context.
    Valid(CullingSection),
    /// A persisted proposal whose identity no longer matches (visible `stale`).
    Stale {
        /// The persisted (but outdated) section.
        section: CullingSection,
        /// The documented identity components that changed.
        mismatches: Vec<IdentityMismatch>,
    },
    /// A persisted section explicitly marked `stale`/`missing`/`corrupt`.
    Unusable {
        /// The persisted but unusable section.
        section: CullingSection,
    },
}

/// Evaluates a schema-valid section against the current identity.
///
/// - persisted status other than `valid` → [`CullingReadState::Unusable`];
/// - identity mismatch → [`CullingReadState::Stale`] (never re-run here);
/// - otherwise → [`CullingReadState::Valid`].
#[must_use]
pub fn evaluate_section(
    section: Option<&CullingSection>,
    current: &CullingIdentity,
) -> CullingReadState {
    let Some(section) = section else {
        return CullingReadState::NoProposal;
    };
    if section.status != CullingStatus::Valid {
        return CullingReadState::Unusable {
            section: section.clone(),
        };
    }
    let mismatches = identity_mismatches(&section.identity, current);
    if mismatches.is_empty() {
        CullingReadState::Valid(section.clone())
    } else {
        CullingReadState::Stale {
            section: section.clone(),
            mismatches,
        }
    }
}

/// Convenience wrapper over [`evaluate_section`] for a validated document.
#[must_use]
pub fn evaluate_culling(document: &SidecarDocument, current: &CullingIdentity) -> CullingReadState {
    evaluate_section(document.culling.as_ref(), current)
}

/// Loads a sidecar from `path` and evaluates its culling proposal. A missing
/// sidecar file is a loud [`CullError::Sidecar`] (never "no proposal"); an
/// absent section inside an existing document is [`CullingReadState::NoProposal`].
pub fn load_culling(path: &Path, current: &CullingIdentity) -> Result<CullingReadState, CullError> {
    let document = load_sidecar(path)?;
    Ok(evaluate_culling(&document, current))
}

/// Records a **valid** proposal on the document (source level only).
///
/// Rejects anything but a `valid` status (stale/missing/corrupt sections are
/// read-only states, not something to write) and re-validates the section
/// before mutating. Virtual copies, rating, flag and labels are not touched.
pub fn record_culling(
    document: &mut SidecarDocument,
    section: CullingSection,
) -> Result<(), CullError> {
    validate_culling_section(&section).map_err(|e| CullError::InvalidProposal(e.to_string()))?;
    if section.status != CullingStatus::Valid {
        return Err(CullError::InvalidProposal(format!(
            "only a valid proposal may be recorded, got status {:?}",
            section.status
        )));
    }
    let proposal = section.proposal;
    let score = section.score;
    let reason_count = section.reasons.len();
    document.culling = Some(section);
    info!(
        "culling proposal recorded (source level): proposal={proposal:?} score={score:.3} \
         reasons={reason_count}"
    );
    Ok(())
}

/// Explicitly removes the proposal from the document ("no proposal"), without
/// inventing a replacement.
pub fn clear_culling(document: &mut SidecarDocument) {
    if document.culling.take().is_some() {
        info!("culling proposal cleared (source level): no proposal");
    }
}

/// Writes a document containing a proposal through the existing atomic sidecar
/// writer. Validation happens inside `save_sidecar`; failures are loud.
pub fn save_culling(path: &Path, document: &SidecarDocument) -> Result<(), CullError> {
    save_sidecar(path, document)?;
    info!(
        "culling sidecar saved: path={} proposal_present={}",
        path.display(),
        document.culling.is_some()
    );
    Ok(())
}
