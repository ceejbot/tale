//! I know this is usually what you do for libraries not applications,
//! but I find myself frustrated by flinging around text-only errors
//! as a way to smooth out types.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum TaleError {
    /// I wish to register a complaint.
    #[error("I wish to register a complaint.")]
    Complaint,
}
