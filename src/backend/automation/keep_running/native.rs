use std::path::Path;

use crate::control::keep_running as control;

use super::*;

fn failure(error: control::KeepRunningError) -> Failure {
    Failure {
        kind: if error.kind() == control::ErrorKind::Unsupported {
            FailureKind::UnsupportedContext
        } else {
            FailureKind::Retryable
        },
        message: error.to_string(),
    }
}

fn spec(rule: &KeepRunningRule) -> Result<control::LaunchSpec, Failure> {
    rule.validate().map_err(|message| Failure {
        message,
        kind: FailureKind::UnsupportedContext,
    })?;
    control::LaunchSpec::new(
        Path::new(&rule.executable_path),
        &rule.arguments,
        Path::new(&rule.working_directory),
    )
    .map_err(failure)
}

impl Platform for Windows {
    type Catalog = control::ProcessCatalog;
    type Context = control::DesktopContext;
    type Identity = control::ExecutableIdentity;
    type Member = control::Member;
    type Launch = control::PreparedLaunch;

    fn context() -> Result<Self::Context, Failure> {
        control::DesktopContext::capture().map_err(failure)
    }
    fn binding(context: &Self::Context) -> String {
        context.binding_key()
    }
    fn identity(rule: &KeepRunningRule) -> Result<Self::Identity, Failure> {
        spec(rule)?.identity().map_err(failure)
    }
    fn action_identity(
        target: &ProcessActionTarget,
        context: &Self::Context,
    ) -> Result<Self::Identity, Failure> {
        let target = crate::control::process::ProcessControlTarget::from_action_target(target);
        control::retain_member(&target, context)
            .map(|member| member.executable_identity().clone())
            .map_err(failure)
    }
    fn scan(
        rule: &KeepRunningRule,
        context: &Self::Context,
        exclude: Option<&ProcessTargetKey>,
        catalog: Option<&Self::Catalog>,
    ) -> Result<Vec<Self::Member>, Failure> {
        control::scan_members(&spec(rule)?, context, exclude, catalog).map_err(failure)
    }
    fn key(member: &Self::Member) -> ProcessTargetKey {
        member.key()
    }
    fn exited(member: &Self::Member) -> Result<bool, Failure> {
        member.exit_confirmed().map_err(failure)
    }
    fn prepare(
        rule: &KeepRunningRule,
        binding: &str,
        identity: &Self::Identity,
    ) -> Result<Self::Launch, PrepareFailure<Self::Launch>> {
        let spec = spec(rule).map_err(|error| PrepareFailure {
            error,
            cleanup: None,
        })?;
        if spec.identity().map_err(|error| PrepareFailure {
            error: failure(error),
            cleanup: None,
        })? != *identity
        {
            return Err(PrepareFailure {
                error: Failure {
                    kind: FailureKind::UnsupportedContext,
                    message: "Keep Running executable changed before preparation.".into(),
                },
                cleanup: None,
            });
        }
        control::prepare(&spec, binding).map_err(|error| PrepareFailure {
            error: failure(error.error),
            cleanup: error.cleanup,
        })
    }
    fn launch_key(launch: &Self::Launch) -> Option<ProcessTargetKey> {
        launch.key()
    }
    fn commit(launch: &mut Self::Launch) -> Result<(), Failure> {
        launch.resume_accept().map_err(failure)
    }
    fn accepted(launch: &Self::Launch) -> bool {
        launch.is_accepted()
    }
    fn take_member(launch: &mut Self::Launch) -> Result<Option<Self::Member>, Failure> {
        launch.take_member().map_err(failure)
    }
    fn dispose(launch: &mut Self::Launch) -> Result<(), Failure> {
        match launch.dispose().map_err(failure)? {
            control::DisposeOutcome::Disposed => Ok(()),
            control::DisposeOutcome::AlreadyAccepted => Err(Failure {
                kind: FailureKind::UnsupportedContext,
                message: "Keep Running will not dispose an accepted app.".into(),
            }),
        }
    }
}
