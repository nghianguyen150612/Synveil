//! Explicit unsupported-platform adapter for the desktop's typed Host surface.
//! It never launches a process or emulates the Linux privileged helper.
use crate::protocol::{HostSelection, Request, Response, ReviewedContext};
use synveil_server_bootstrap::{
    CanonicalOwners, CoordinatorError, OwnerObservation, PrivilegedEffect,
};

#[derive(Default)]
pub struct NativeHostOwners;

impl NativeHostOwners {
    pub fn authorize(&mut self) -> Result<(), &'static str> {
        Err("Managed Host unsupported on this platform")
    }

    pub fn request(&mut self, mut request: Request) -> Result<Response, &'static str> {
        use zeroize::Zeroize;
        match &mut request {
            Request::Account { password, .. } | Request::EnrollDesktop { password, .. } => {
                password.zeroize();
            }
            Request::Review {
                external_database_url: Some(url),
                ..
            }
            | Request::Confirm {
                external_database_url: Some(url),
                ..
            } => url.zeroize(),
            _ => {}
        }
        Err("Managed Host unsupported on this platform")
    }

    pub fn prepare_review(
        &mut self,
        _selection: HostSelection,
        external_database_url: Option<String>,
    ) -> Result<ReviewedContext, String> {
        match self.request(Request::Review {
            selection: _selection,
            external_database_url,
        }) {
            Err(error) => Err(error.to_owned()),
            Ok(_) => Err("Managed Host unsupported on this platform".to_owned()),
        }
    }

    pub fn confirm_context(
        &mut self,
        context: ReviewedContext,
        external_database_url: Option<String>,
    ) -> Result<(), &'static str> {
        self.request(Request::Confirm {
            context,
            external_database_url,
        })
        .map(|_| ())
    }

    pub fn inspect_without_elevation() -> Result<Response, &'static str> {
        Err("Managed Host unsupported on this platform")
    }
}

impl CanonicalOwners for NativeHostOwners {
    fn inspect(&mut self) -> Result<OwnerObservation, CoordinatorError> {
        Err(CoordinatorError::OwnerRejected)
    }

    fn current_generation(&mut self) -> Result<u64, CoordinatorError> {
        Err(CoordinatorError::OwnerRejected)
    }

    fn apply(&mut self, _effect: PrivilegedEffect) -> Result<(), CoordinatorError> {
        Err(CoordinatorError::OwnerRejected)
    }

    fn trust_descriptor(&mut self) -> Result<String, CoordinatorError> {
        Err(CoordinatorError::NotReady)
    }
}
