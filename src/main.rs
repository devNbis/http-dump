//! Provides a RESTful web server managing some Todos.
//!
//! Run with
//!
//! ```not_rust
//! cargo run -p http_dump
//! ```

use axum::{
    Router,
     body::{Body, Bytes}, 
    extract::{ //FromRef, FromRequest,
       Request, State,// Extension,
      ws::{ WebSocketUpgrade,WebSocket,Message//, Utf8Bytes
      }}, 
      http::{
      //header::CONTENT_TYPE, 
      //HeaderMap,
      //Method,
      StatusCode,
     // Version
    }, middleware::{self, Next},
     response::{IntoResponse, Response}, routing::{any,get}
    
};
//use tracing::info;
//use serde::{Deserialize, Serialize};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use tokio::signal;
use tokio::sync::broadcast;
use tokio::sync::broadcast::Sender;
use tower_http::services::ServeDir;
use std::{//any::Any, 
  path::PathBuf,
  sync::{Arc//, Mutex
  },
 // collections::HashSet,
  env};
use http_body_util::BodyExt;

// Our shared state
struct AppState {
    // We require unique usernames. This tracks which usernames have been taken.
    //user_set: Mutex<HashSet<String>>,
    // Channel used to send messages to all connected clients.
    tx: broadcast::Sender<String>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                format!("{}=debug,tower_http=debug", env!("CARGO_CRATE_NAME")).into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let bind_address = env::var("HTTP_DUMP_BIND").unwrap_or("0.0.0.0:3100".to_string());

    let assets_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");

   // let user_set = Mutex::new(HashSet::new());
    let (tx, _rx) = broadcast::channel(100);

    //let app_state = Arc::new(AppState {user_set, tx });
    let app_state = Arc::new(AppState { tx });

    let app = Router::new()
    .fallback_service(ServeDir::new(assets_dir).append_index_html_on_directories(true))
       
        .route("/ws", get(websocket_handler))
        .route("/", any(|| async move { /* ... */ }))

       .layer(middleware::from_fn_with_state(app_state.clone(),print_request_response))
       //.route_layer(middleware::from_fn_with_state(app_state.clone(),print_request_response))
       
       .with_state(app_state);


    let listener = tokio::net::TcpListener::bind(bind_address)
        .await
        .unwrap();
    tracing::debug!("listening on {}", listener.local_addr().unwrap());
    #[allow(unused)]
    axum::serve(listener, app)
    .with_graceful_shutdown(shutdown_signal())
    .await;
}


async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| websocket(socket, state))
}

// This function deals with a single websocket connection, i.e., a single
// connected client / user, for which we will spawn two independent tasks (for
// receiving / sending chat messages).
async fn websocket(mut stream: WebSocket, state: Arc<AppState>) {
    // By splitting, we can send and receive at the same time.
    //let (mut sender, mut receiver) = stream.split();
    let mut rx = state.tx.subscribe();
   let mut _send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            // In any websocket error, break loop.
            if stream.send(Message::text(msg)).await.is_err() {
                break;
            }
        }
    });

}





async fn print_request_response(
   State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
    
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let (parts, body) = req.into_parts();
    let bytes = buffer_and_print(format!("Request: {:?}",parts).as_str(), body, state.tx.clone()).await?;
    
    let req = Request::from_parts(parts, Body::from(bytes));

    let res = next.run(req).await;

    let (parts, body) = res.into_parts();
    let bytes = buffer_and_print(format!("Response: {:?}",parts).as_str(), body, state.tx.clone()).await?;
    let res = Response::from_parts(parts, Body::from(bytes));

    Ok(res)
}

async fn buffer_and_print<B>(direction: &str, body: B,  tx: Sender<String>) -> Result<Bytes, (StatusCode, String)>
where
    B: axum::body::HttpBody<Data = Bytes>,
    B::Error: std::fmt::Display,
{
    let bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(err) => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("failed to read {direction} body: {err}"),
            ));
        }
    };

    if let Ok(body) = std::str::from_utf8(&bytes) {
        
        let _ = tx.send(format!("{direction} body = {body:?}"));
        
        tracing::debug!("{direction} body = {body:?}");
    }

    Ok(bytes)
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}