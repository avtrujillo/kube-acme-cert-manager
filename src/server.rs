use std::{sync::Arc, net::SocketAddr};

use axum::{
    debug_handler, extract::{
        Path, State
    }, response::IntoResponse, Router
};
use http::{StatusCode, Uri, Request};
// use hyper_util::rt::{TokioExecutor, TokioIo};
use k8s_openapi::{
    api::core::v1::Service as KubeService, //ListOptional, ListResponse,
};
use kube::{api::ListParams, Api};
use tokio::net::TcpListener;
use http_body_util::BodyExt;
// use tokio_stream::wrappers::TcpListenerStream;

use super::{LastSeen, ChallengeServerWithControllerError};

use crate::crds::challenge::Challenge;

static SERVICE_LABEL_SELECTOR: &str = "acme.cert-manager.io/http01-solver=true";

// responds to healthchecks
#[axum::debug_handler]
pub async fn healthz() -> (StatusCode, ()) {
    (StatusCode::OK, ())
}

// pub async fn challenge_server_retry_err_stream(state: ChallengeServerState, socket_addr: SocketAddr) -> Result<(), ChallengeServerWithControllerError> {
//     let router = Router::new()
//     .route("/.well-known/acme-challenge/:token", axum::routing::get(challenge_handler))
//     .with_state(state.clone())
//     .route("/healthz", axum::routing::get(healthz))
//     .route("/healthz/", axum::routing::get(healthz));
    
//     run_challenge_server(router, socket_addr).await
// }

pub async fn run_challenge_server(
    state: ChallengeServerState, socket_addr: SocketAddr
) -> Result<(), ChallengeServerWithControllerError> {
    let domain_str = std::env::var("DOMAIN").map_err(|e| {
        ChallengeServerWithControllerError::DomainEnvVar(e)
    })?;
    let domain = http::uri::Authority::try_from(
        domain_str
    ).map_err(|e| {
        ChallengeServerWithControllerError::InvalidDomain(e)
    })?;
    let listener = TcpListener::bind(socket_addr).await.map_err(|e| {
        ChallengeServerWithControllerError::Server(
            crate::AcmeServerError::Bind(e)
        )
    })?;

    let router = Router::new()
    .route("/.well-known/acme-challenge/{token}", axum::routing::get(challenge_handler))
    .with_state(state.clone())
    .route("/healthz", axum::routing::get(healthz))
    .route("/healthz/", axum::routing::get(healthz))
    .fallback(async |http_uri: Uri| {
        match Uri::builder()
        .authority((http_uri.authority()).cloned().unwrap_or(
            domain
        ))
        .path_and_query(
            http_uri.path_and_query().cloned().unwrap_or(
                http::uri::PathAndQuery::from_static("/")
            )
        )
        .scheme(http::uri::Scheme::HTTPS)
        .build() {
            Err(e) => {
                eprintln!("Error building uri in https redirect: {e}");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            },
            Ok(https_uri) => {
                axum::response::Redirect::to(
                    https_uri.to_string().as_str()
                ).into_response()
            }
        }
    });

    axum::serve(listener, router).await.map_err(|e| {
        ChallengeServerWithControllerError::Server(
            crate::AcmeServerError::Axum(e)
        )
    })
}

#[debug_handler]
async fn challenge_handler(
    State(state): State<ChallengeServerState>,
    Path(token): Path<String>,
    request: Request<hyper::body::Bytes>
) -> Result<hyper::body::Bytes, StatusCode> {

    let challenge_path = format!("/.well-known/acme-challenge/{}", token);
    
    let services = get_service_list(state.service_api).await?;

    let service = services.into_iter().next().ok_or(StatusCode::NOT_FOUND)?;
    let service_spec = service.spec.ok_or(StatusCode::NOT_FOUND)?;
    let service_ip = service_spec.cluster_ip.ok_or(StatusCode::NOT_FOUND)?;

    let challenge_req_uri = Uri::builder()
    .authority(
        format!("{}:{}", service_ip, 8089)
    )
    .scheme("http")
    .path_and_query(
        challenge_path
    ).build().map_err(|e| {
        eprintln!("{:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let challenge_req = state.reqwest_client.request(
        http::Method::GET,
        challenge_req_uri.to_string()
    ).headers(request.headers().clone())
    .body(request.into_body())
    .build().map_err(|e| {
        eprintln!("Error in request builder: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let resp = state.reqwest_client.execute(challenge_req).await.map_err(|e| {
        eprintln!("{:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    resp.bytes().await.map_err(|e| {
        eprintln!("{:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })

}

// get all kubernetes services labeled with acme.cert-manager.io/http01-solver=true
async fn get_service_list(service_api: &kube::Api<KubeService>) -> Result<Vec<KubeService>, StatusCode> {
    
    let service_list = service_api.list(&ListParams {
        label_selector: Some(SERVICE_LABEL_SELECTOR.to_string()), ..Default::default()
    }).await.map_err(|e| {
        // TODO: tracing
        eprintln!("{:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(service_list.into_iter().collect())
}

#[derive(Clone)]
pub struct ChallengeServerState {
    // pub kube_client: kube::Client,
    pub reqwest_client: reqwest::Client,
    pub service_api: &'static Api<KubeService>,
    pub last_seen_challenge: &'static LastSeen<Option<Arc<Challenge>>>
}

#[derive(thiserror::Error, Debug)]
pub enum ChallengeServerRejection {
    #[error(transparent)]
    Collect(#[from] axum::Error)
}

impl IntoResponse for ChallengeServerRejection {
    fn into_response(self) -> axum::response::Response {
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    }
}

impl axum::extract::FromRequest<ChallengeServerState> for http::Request<hyper::body::Bytes> {
    type Rejection = ChallengeServerRejection;

    async fn from_request(req: Request<axum::body::Body>, _state: &ChallengeServerState) -> Result<Self, Self::Rejection> {
        let (req_parts, req_body) = req.into_parts();
        // TODO: try to get rid of the copy here
        let collected = req_body.collect().await?;
        Ok(http::Request::from_parts(
            req_parts,
            collected.to_bytes()
        ))
    }
}
