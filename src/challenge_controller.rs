use kube::runtime::{Controller, finalizer::Event, controller::Action};
use std::sync::Arc;
use futures::future::Either;
use futures::StreamExt;
use crate::crds::challenge::Challenge;

use super::{LastSeen, Observed};

#[derive(Debug, thiserror::Error)]
pub enum ChallengeControllerError {
    #[error("kube error in challenge controller: {0}")]
    Kube(#[from] kube::Error),
    #[error("challenge controller terminated with Ok(()), which should never happen")]
    Ok
}


pub async fn challenge_controller_start(
    namespace: String,
    kube_client: kube::Client,
    watcher_config: kube::runtime::watcher::Config,
    last_seen_challenge: &'static LastSeen<Option<Arc<Challenge>>>
) -> Result<(), ChallengeControllerError> {
    let challenge_api = kube::Api::<Challenge>::namespaced(kube_client, namespace.as_str());
    let context = Arc::new(ChallengerControllerContext{
        challenge_api: challenge_api.clone(),
        last_seen_challenge
    });
    let controller = Controller::<Challenge>::new(
        challenge_api,
        // kube::api::ListParams::default()
        watcher_config
    );
    #[allow(clippy::unit_arg)]
    Ok(
        controller.run(reconcile, error_policy, context)
            .for_each(|res| async move {
                match res {
                    Err(e) => {
                        eprintln!("Challenge controller reconcile error: {:?}", e)
                    },
                    Ok(o) => println!(
                        "Challenge reconciled: {:?}", o
                    )
                }
            })
        .await
    )
}

async fn reconcile(challenge: Arc<Challenge>, context: Arc<ChallengerControllerContext>) -> Result<Action, kube::runtime::finalizer::Error<kube::Error>> {
    kube::runtime::finalizer(
        &context.as_ref().challenge_api,
        "challenge_server",
        challenge,
        |event: Event<Challenge>| {
            match event {
                Event::Apply(c) => {
                    Either::Left(apply_challenge(c, context.as_ref()))
                }, Event::Cleanup(c) => {
                    Either::Right(cleanup_challenge(c, context.as_ref()))
                }
            }
        }
    ).await
}

async fn apply_challenge(challenge: Arc<Challenge>, ctx: &ChallengerControllerContext) -> Result<Action, kube::Error> {
    ctx.last_seen_challenge.insert(
        Observed {
            inner: Some(challenge),
            timestamp: tokio::time::Instant::now()
        }
    ).await;
    Ok(Action::await_change())
}

async fn cleanup_challenge(_challenge: Arc<Challenge>, _ctx: &ChallengerControllerContext) -> Result<Action, kube::Error> {
    Ok(Action::await_change())
}

fn error_policy(_challenge: Arc<Challenge>, e: &kube::runtime::finalizer::Error<kube::Error>, _context: Arc<ChallengerControllerContext>) -> Action {
    eprintln!("{:?}", e);
    Action::requeue(tokio::time::Duration::from_secs(30))
}

struct ChallengerControllerContext {
    challenge_api: kube::Api<Challenge>,
    last_seen_challenge: &'static LastSeenChallenge,
}

type LastSeenChallenge = LastSeen<Option<Arc<Challenge>>>;