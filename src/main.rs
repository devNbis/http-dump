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

    extract::{ 
       Request, State,Path,
      ws::{ WebSocketUpgrade,WebSocket,Message
      }}, 
      http::{header,StatusCode },
      debug_middleware,
       middleware::{self, Next},
     response::{IntoResponse, Response, Html}, routing::{any,get}
    
};

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use tokio::signal;
use tokio::sync::broadcast;
use tokio::sync::Mutex;
use tokio::sync::broadcast::Sender;
use tower::ServiceBuilder;
use std::{
   sync::{Arc },
  env};
use http_body_util::BodyExt;
use rust_embed::Embed;


// Our shared state
struct AppState {
  count:Mutex<i32>,
   err400_count:i32,
   err500_count:i32,
  
    tx: broadcast::Sender<String>,
}



#[derive(Embed, Clone)]
#[folder = "assets/"]
struct Asset;

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

    let bind_address = env::var("HTTP_DUMP_BIND").unwrap_or("0.0.0.0:8089".to_string());
    let err400_count: i32= env::var("HTTP_DUMP_ERR400_COUNT").unwrap_or("0".to_string()).parse().expect("Failed env var.");
    let err500_count: i32= env::var("HTTP_DUMP_ERR500_COUNT").unwrap_or("0".to_string()).parse().expect("Failed env var.");
   
    let (tx, _rx) = broadcast::channel(100);

    let app_state = Arc::new(AppState {count:Mutex::new(0), err400_count ,err500_count,tx });

    let app = Router::new()
       .fallback_service(get(not_found))
        .route("/ws", get(websocket_handler))
        .route("/{*wildcard}", any(|| async move { /* ... */ }))
      .route("/", get(index_handler))
    .route("/index.html", get(index_handler))
    .route("/favicon.ico", get(favicon_handler))
    .layer(
        ServiceBuilder::new()
       .layer(middleware::from_fn_with_state(app_state.clone(),print_request_response))
       .layer(middleware::from_fn_with_state(app_state.clone(),response_on_error_counts))
    )
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



// Finally, we use a fallback route for anything that didn't match.
async fn not_found() -> Html<&'static str> {
  Html("<h1>404</h1><p>Not Found</p>")
}

#[debug_middleware]
async fn response_on_error_counts(
 State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
    
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let mut count = state.count.lock().await;
    *count += 1;
     tracing::debug!("request counter: {}, 400er: {}, 500er: {}",*count, state.err400_count, state.err500_count);
    if is_factor( state.err400_count ,*count) {
      

            return Err((
                StatusCode::BAD_REQUEST,
                "".to_string()),
            )
       ;
    }
    if is_factor( state.err500_count ,*count) {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "".to_string()),
            )
          ;
    }
    //let (parts, body) = req.into_parts();
    let res = next.run(req).await;
    //let res = Response::from_parts(parts,body);
    Ok(res)
  
}


#[debug_middleware]
async fn print_request_response(
   State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
    
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let (parts, body) = req.into_parts();

    let bytes = buffer_and_print(format!("Request: {:?}",parts).as_str(), body, state.tx.clone()).await?;
    /* let mut count = state.count.lock().await;
     tracing::debug!("request counter: {}, 400er: {}, 500er: {}",*count, state.err400_count, state.err500_count);
    *count += 1;
    
    if is_factor( state.err400_count ,*count) {
            return Err((
                StatusCode::BAD_REQUEST,
                "".to_string()),
            )
          ;
    }
    if is_factor( state.err500_count ,*count) {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "".to_string()),
            )
          ;
    }*/

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

fn is_factor( factor:i32, number:i32) -> bool {
  if factor == 0  {
    return false;
  }
  number % factor ==0
}
// We use static route matchers ("/" and "/index.html") to serve our home
// page.
async fn index_handler() -> impl IntoResponse {
  static_handler(Path("index.html".to_string())).await
}

// We use static route matchers ("/favicon.ico") to serve our home
// page.
async fn favicon_handler() -> impl IntoResponse {
  static_handler(Path("favicon.ico".to_string())).await
}

// We use a wildcard matcher ("/dist/*file") to match against everything
// within our defined assets directory. This is the directory on our Asset
// struct below, where folder = "examples/public/".
async fn static_handler(Path(path): Path<String>) -> impl IntoResponse {
  StaticFile(path)
}

pub struct StaticFile<T>(pub T);

impl<T> IntoResponse for StaticFile<T>
where
  T: Into<String>,
{
  fn into_response(self) -> Response {
    let path = self.0.into();

    match Asset::get(path.as_str()) {
      Some(content) => {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        ([(header::CONTENT_TYPE, mime.as_ref())], content.data).into_response()
      }
      None => (StatusCode::NOT_FOUND, "404 Not Found").into_response(),
    }
  }
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