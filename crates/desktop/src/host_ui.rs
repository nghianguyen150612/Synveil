//! Blocking native work runs off the Qt thread; this model contains no secrets.
use synveil_server_bootstrap::ServerBootstrapCoordinator;
use synveil_server_host::{
    client::NativeHostOwners,
    protocol::{HostSelection, Request, Response, ReviewedContext},
};

pub struct HostSession {
    pub coordinator: ServerBootstrapCoordinator<NativeHostOwners>,
    review: Option<ReviewedContext>,
}
impl Default for HostSession {
    fn default() -> Self {
        Self {
            coordinator: ServerBootstrapCoordinator::new(NativeHostOwners::default()),
            review: None,
        }
    }
}
pub enum HostAction {
    Inspect,
    Preflight,
    Review(HostSelection, Option<zeroize::Zeroizing<String>>),
    ReconfigureNetwork(HostSelection),
    Confirm(Option<zeroize::Zeroizing<String>>),
    Account {
        login: String,
        password: zeroize::Zeroizing<String>,
        create: bool,
    },
    Verify,
    Cancel,
    Lifecycle(synveil_server_host::protocol::HostPurpose),
}
#[derive(Default)]
pub struct HostView {
    pub selection: String,
    pub step: String,
    pub feedback: String,
    pub review: String,
    pub candidates: String,
    pub server_id: String,
    pub origin: String,
}
impl HostSession {
    pub fn run_with_progress(
        &mut self,
        action: HostAction,
        progress: impl FnMut(
            synveil_server_bootstrap::PrivilegedEffect,
            &synveil_server_bootstrap::OwnerObservation,
        ),
    ) -> Result<HostView, String> {
        match action {
            HostAction::Lifecycle(purpose) => {
                self.coordinator.begin_host();
                self.coordinator
                    .owners_mut()
                    .authorize()
                    .map_err(str::to_owned)?;
                let context = match self
                    .coordinator
                    .owners_mut()
                    .request(Request::ReviewLifecycle { purpose })
                    .map_err(str::to_owned)?
                {
                    Response::Review { context } => context,
                    Response::Error { code } => return Err(code),
                    _ => return Err("Invalid lifecycle review".to_owned()),
                };
                self.coordinator
                    .review(&context.plan)
                    .map_err(|_| "Lifecycle plan changed; inspect and review again.".to_owned())?;
                let view = HostView {
                    step: "review".to_owned(),
                    review: context.summary.join("\n\n"),
                    server_id: context.plan.server_installation_id.clone(),
                    origin: context.selection.canonical_origin.clone(),
                    feedback: "Review this preserving lifecycle change before confirming."
                        .to_owned(),
                    ..HostView::default()
                };
                self.review = Some(context);
                Ok(view)
            }
            HostAction::Preflight => {
                self.coordinator.begin_host();
                self.coordinator
                    .owners_mut()
                    .authorize()
                    .map_err(str::to_owned)?;
                self.inspection()
            }
            HostAction::Inspect => self.inspection(),
            HostAction::Review(selection, external) => {
                self.coordinator.begin_host();
                self.coordinator
                    .owners_mut()
                    .authorize()
                    .map_err(str::to_owned)?;
                let context = self
                    .coordinator
                    .owners_mut()
                    .prepare_review(selection, external.map(|url| url.to_string()))?;
                self.coordinator
                    .review(&context.plan)
                    .map_err(|_| "The plan changed; inspect and review again.".to_owned())?;
                let view = HostView { step: "review".to_owned(), review: context.summary.join("\n\n"), server_id: context.plan.server_installation_id.clone(), origin: context.selection.canonical_origin.clone(), feedback: "Review these changes before confirming. Authorization alone does not prepare the server.".to_owned(), ..HostView::default() };
                self.review = Some(context);
                Ok(view)
            }
            HostAction::ReconfigureNetwork(selection) => {
                self.coordinator.begin_host();
                self.coordinator
                    .owners_mut()
                    .authorize()
                    .map_err(str::to_owned)?;
                let context = match self
                    .coordinator
                    .owners_mut()
                    .request(Request::ReviewNetwork { selection })
                    .map_err(str::to_owned)?
                {
                    Response::Review { context } => context,
                    Response::Error { code } => return Err(code),
                    _ => return Err("Invalid network review".to_owned()),
                };
                self.coordinator.review(&context.plan).map_err(|_| {
                    "The network plan changed; inspect and review again.".to_owned()
                })?;
                let view = HostView {
                    step: "review".to_owned(),
                    review: context.summary.join("\n\n"),
                    server_id: context.plan.server_installation_id.clone(),
                    origin: context.selection.canonical_origin.clone(),
                    feedback: "Review the network and access change before confirming.".to_owned(),
                    ..HostView::default()
                };
                self.review = Some(context);
                Ok(view)
            }
            HostAction::Confirm(external) => {
                let context = self
                    .review
                    .take()
                    .ok_or("Review a fresh plan before preparing this Host.")?;
                self.coordinator
                    .owners_mut()
                    .confirm_context(context.clone(), external.map(|value| value.to_string()))
                    .map_err(str::to_owned)?;
                self.coordinator.confirm_with_progress(&context.plan, &context.binding, progress).map_err(|_| "Preparation stopped. Inspect the preserved owners and review a fresh plan to resume.".to_owned())?;
                self.inspection()
            }
            HostAction::Account {
                login,
                password,
                create,
            } => {
                self.coordinator
                    .owners_mut()
                    .authorize()
                    .map_err(str::to_owned)?;
                match self
                    .coordinator
                    .owners_mut()
                    .request(Request::Account {
                        login,
                        password: password.to_string(),
                        create,
                    })
                    .map_err(str::to_owned)?
                {
                    Response::Complete => self.inspection(),
                    Response::Error { code } => Err(code),
                    _ => Err("Invalid account result.".to_owned()),
                }
            }
            HostAction::Verify => {
                self.coordinator
                    .owners_mut()
                    .authorize()
                    .map_err(str::to_owned)?;
                self.coordinator.verify().map_err(|_| {
                    "The Host is not Ready. Inspect server health and recovery.".to_owned()
                })?;
                self.inspection()
            }
            HostAction::Cancel => {
                self.review = None;
                self.coordinator.cancel();
                let _ = self.coordinator.owners_mut().request(Request::Cancel);
                self.coordinator = ServerBootstrapCoordinator::new(NativeHostOwners::default());
                Ok(HostView {
                    step: "selection".to_owned(),
                    feedback: "Setup cancelled. Any already prepared server data is preserved."
                        .to_owned(),
                    ..HostView::default()
                })
            }
        }
    }
    fn inspection(&mut self) -> Result<HostView, String> {
        let response = self
            .coordinator
            .owners_mut()
            .request(Request::Inspect)
            .or_else(|_| NativeHostOwners::inspect_without_elevation())
            .map_err(str::to_owned)?;
        match response {
            Response::Inspection {
                observation,
                detail,
                candidates,
                selection,
                ..
            } => {
                use synveil_server_bootstrap::HostSetupState as S;
                let step = match observation.derive_state() {
                    S::Ready => "ready",
                    S::AdminBootstrapRequired if observation.bootstrap_closed => "sign_in",
                    S::AdminBootstrapRequired => "account",
                    S::Blocked | S::NeedsRepair => "recovery",
                    _ => "selection",
                };
                Ok(HostView {
                    selection: serde_json::to_string(&selection).unwrap_or_default(),
                    step: step.to_owned(),
                    feedback: detail,
                    candidates: serde_json::to_string(&candidates)
                        .unwrap_or_else(|_| "[]".to_owned()),
                    server_id: observation.server_installation_id.unwrap_or_default(),
                    origin: observation.canonical_origin.unwrap_or_default(),
                    ..HostView::default()
                })
            }
            Response::Error { code } => Err(code),
            _ => Err("Invalid Host inspection result.".to_owned()),
        }
    }
}
