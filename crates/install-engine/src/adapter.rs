use crate::{
    ApplyOutcome, InstallationEffect, InstallationPlan, PreflightFinding, PrivilegeRequirement,
    ReconciliationOutcome,
};

/// Mechanics-only boundary. Implementations must not add policy or generic shell execution.
pub trait InstallationAdapter {
    fn preflight(&mut self) -> Vec<PreflightFinding>;
    fn privilege_available(&mut self, requirement: PrivilegeRequirement) -> bool;
    fn inspect_preconditions(&mut self, effect: &InstallationEffect) -> bool;
    fn apply_effect(&mut self, effect: &InstallationEffect) -> ApplyOutcome;
    fn verify_effect(&mut self, effect: &InstallationEffect) -> bool;
    fn reconcile_unknown(&mut self, effect: &InstallationEffect) -> ReconciliationOutcome;
    fn compensation_supported(&mut self, effect: &InstallationEffect) -> bool;
    fn compensate_effect(&mut self, effect: &InstallationEffect) -> bool;
    fn verify_compensation(&mut self, effect: &InstallationEffect) -> bool;
    fn verify_installation(&mut self, plan: &InstallationPlan) -> bool;
}
