use std::{cell::Cell, sync::Arc, net::{SocketAddrV4, Ipv4Addr}};

use challenge_controller::challenge_controller_start;
use crds::challenge::Challenge;
use futures::TryFutureExt;
use k8s_openapi::api::core::v1::Service;
use kube::Api;
use server::ChallengeServerState;
use tokio::{sync::Mutex, task::JoinError};
use std::net::SocketAddr;

mod crds;
mod server;
mod challenge_controller;

#[tokio::main]
pub async fn main() {
    // https://docs.rs/rustls/latest/rustls/crypto/struct.CryptoProvider.html#method.install_default
    // rustls::crypto::ring::default_provider().install_default().unwrap();
    rustls::crypto::aws_lc_rs::default_provider().install_default().unwrap();
    let namespace = std::env::var("NAMESPACE").unwrap_or("lec".to_string());
    let port_var = std::env::var("PORT").unwrap_or("80".to_string());
    let challenge_socket = SocketAddr::V4(
        SocketAddrV4::new(
            Ipv4Addr::new(0, 0, 0, 0),
            port_var.parse().expect("Failed to parse PORT env var as u16")
        )
    ); // TODO: IPv6 support

    // TODO: set up some means of getitng config info
    let watcher_config = kube::runtime::watcher::Config::default();
    let last_seen_challenge: &'static LastSeen<Option<Arc<Challenge>>> = Box::leak(
        Box::new(LastSeen::new(None))
    );
    let kube_client = kube::Client::try_default().await.expect("Failed to start kube client");
    match run_challenge_controller_and_server(
        challenge_socket, namespace.clone(), kube_client.clone(), watcher_config, last_seen_challenge
    ).await {
        Ok(()) => (),
        Err(e) => {
            eprintln!("Restarting challenge controller + server loop due to an error: {:?}", e);
        }
    }
}

pub async fn run_challenge_controller_and_server(
    socket_addr: SocketAddr,
    namespace: String,
    kube_client: kube::Client,
    watcher_config: kube::runtime::watcher::Config,
    last_seen_challenge: &'static LastSeen<Option<Arc<Challenge>>>
) -> Result<(), ChallengeServerWithControllerError> {
    let service_api: &'static Api<Service> = Box::leak(
        Box::new(Api::namespaced(kube_client.clone(), &namespace))
    );
    let challenge_server_state = ChallengeServerState {
        service_api,
        last_seen_challenge: last_seen_challenge,
        reqwest_client: reqwest::Client::new()
    };

    let server_fut = server::run_challenge_server(
        challenge_server_state, socket_addr
    );

    let controller_fut = challenge_controller_start(
        namespace.clone(),
        kube_client.clone(),
        watcher_config.clone(),
        last_seen_challenge
    ).map_err(|e| {
        ChallengeServerWithControllerError::Controller(e)
    });

    let server_handle = tokio::spawn(
        server_fut
    );

    let controller_handle = tokio::spawn(
        controller_fut
    );

    let join_result = futures::future::try_join(
        server_handle, controller_handle
    ).await?;
    match join_result {
        (Ok(()), Ok(())) => Ok(()),
        (Err(e), Ok(())) => Err(e),
        (Ok(()), Err(e)) => Err(e),
        (Err(e), Err(_)) => Err(e)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ChallengeServerWithControllerError {
    #[error("Error in acme challenge controller: {0}")]
    Controller(#[from] challenge_controller::ChallengeControllerError),
    #[error("Error in acme challenge server: {0}")]
    Server(#[from] AcmeServerError),
    #[error("Error joining on challenge server and controller: {0}")]
    Join(#[from] JoinError),
    #[error("Error getting the DOMAIN env var: {0}")]
    DomainEnvVar(std::env::VarError),
    #[error("DOMAIN env var is not a valid domain: {0}")]
    InvalidDomain(http::uri::InvalidUri)
}

#[derive(Debug, thiserror::Error)]
pub enum AcmeServerError {
    #[error("Server error: {0}")]
    Axum(std::io::Error),
    #[error("Error binding to socket: {0}")]
    Bind(std::io::Error)
}

#[derive(Debug, thiserror::Error)]
pub enum ChallengeStartupError {
    #[error("Error starting challenge controller and/or server: {0}")]
    Kube(#[from] kube::Error)
}

pub struct Observed<T: Clone + Sync> {
    inner: T,
    timestamp: tokio::time::Instant
}

pub struct LastSeen<T: Clone + Sync>(Mutex<Cell<Observed<T>>>);

// TODO: optimize
impl<T: Clone + Sync> LastSeen<T> {
    pub fn new(inner: T) -> Self {
        Self(Mutex::new(Cell::new(Observed { inner: inner, timestamp: tokio::time::Instant::now() })))
    }

    // TODO: proper caching
    pub async fn get(&self) -> T {
        self.0.lock().await.get_mut().inner.clone()
        // self.0.get_mut().get_mut().inner.clone()
    }

    pub async fn insert(&self, observed: Observed<T>) {
        let mut observed_mutex = self.0.lock().await;
        if observed_mutex.get_mut().timestamp.duration_since(observed.timestamp).is_zero() {
            *observed_mutex.get_mut() = observed
        }
    }
}