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
    debug_middleware,
    extract::{
        Path, Request, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{any, get},
};

use clap::Parser;
use http_body_util::BodyExt;
use rust_embed::Embed;
use std::{collections::HashMap, sync::Arc};
use tokio::signal;
use tokio::sync::{Mutex, broadcast, broadcast::Sender};
use tower::ServiceBuilder;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Programm for dump http(s) traffic into system log or live http view
#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Binding of the web server
    #[arg(
        short,
        long,
        value_name = "IP:Port",
        env = "HTTP_DUMP_BIND",
        default_value = "0.0.0.0:8089",
        help = "host address include port"
    )]
    bind: String,

    /// Error map definition
    #[arg(
        short,
        long,
        value_name = "MAP",
        env = "HTTP_DUMP_ERROR_MAP",
        default_value = "",
        help = "string as mapp with <count>:<error> delimiter ; \nsample: \"4:500,6:400\"\nresponed with error on every 4th call with 500 and every 6th call with 400"
    )]
    error_map: String,

    /// logging definition
    #[arg(
        short,
        long,
        value_name = "LOG",
        env = "HTTP_DUMP_TRACELOG",
        default_value = "info,tower=info",
        help = "logging definition"
    )]
    tracelog: String,
}

// Our shared state
#[derive(Debug)]
struct AppState {
    count: Mutex<i32>,
    error_map: HashMap<i32, StatusCode>,

    tx: broadcast::Sender<String>,
}

#[derive(Embed, Clone)]
#[folder = "assets/"]
struct Asset;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| cli.tracelog.into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let bind_address = cli.bind;

    let error_map = build_error_map(cli.error_map);

    let (tx, _rx) = broadcast::channel(100);

    let app_state = Arc::new(AppState {
        count: Mutex::new(0),
        error_map,
        tx,
    });
    tracing::debug!("AppState:  {:#?} ", app_state);
    let app = Router::new()
        .fallback_service(get(not_found))
        .route("/ws", get(websocket_handler))
        .route("/{*wildcard}", any(|| async move { /* ... */ }))
        .route("/", get(index_handler))
        .route("/index.html", get(index_handler))
        .route("/favicon.ico", get(favicon_handler))
        .route("/logo.svg", get(logo_handler))
        .layer(
            ServiceBuilder::new()
                .layer(middleware::from_fn_with_state(
                    app_state.clone(),
                    print_request_response,
                ))
                .layer(middleware::from_fn_with_state(
                    app_state.clone(),
                    response_on_error_counts,
                )),
        )
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind(bind_address).await.unwrap();
    tracing::info!("Listening on {}", listener.local_addr().unwrap());
    #[allow(unused)]
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await;
}

fn build_error_map(data_str: String) -> HashMap<i32, StatusCode> {
    let mut map = HashMap::new();
    if data_str.is_empty() {
        return map;
    }
    let token: Vec<&str> = if data_str.contains(";") {
        data_str.split(";").collect()
    } else {
        vec![data_str.as_str()]
    };
    for token_str in token {
        let val: Vec<&str> = token_str.split(":").collect();
        if val.len() == 2 {
            let key = val[0].parse::<i32>().unwrap();
            let code: StatusCode = StatusCode::from_bytes(val[1].as_bytes()).unwrap();
            map.entry(key).insert_entry(code);
        }
    }
    map
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| websocket(socket, state))
}

// This function will send the dumps to an websocket target
async fn websocket(mut stream: WebSocket, state: Arc<AppState>) {
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

// Finally, a fallback route for anything that didn't match.
async fn not_found() -> Html<&'static str> {
    Html("<h1>404</h1><p>Not Found</p>")
}

#[debug_middleware]
async fn response_on_error_counts(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if !req.uri().path().contains("logo.svg") && !req.uri().path().contains("favicon.ico") {
        let mut count = state.count.lock().await;
        *count += 1;
        tracing::debug!("request counter: {}", *count);

        for err_map in &state.clone().error_map {
            if is_factor(*err_map.0, *count) {
                return Ok(err_map.1.into_response());
            }
        }
    }
    let res = next.run(req).await;
    Ok(res)
}

#[debug_middleware]
async fn print_request_response(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let (parts, body) = req.into_parts();
    let bytes = buffer_and_print(
        format!("Request: {:?}", parts).as_str(),
        body,
        state.tx.clone(),
    )
    .await?;
    let req = Request::from_parts(parts, Body::from(bytes));

    let res = next.run(req).await;

    let (parts, body) = res.into_parts();
    let bytes = buffer_and_print(
        format!("Response: {:?}", parts).as_str(),
        body,
        state.tx.clone(),
    )
    .await?;
    let res = Response::from_parts(parts, Body::from(bytes));

    Ok(res)
}

async fn buffer_and_print<B>(
    direction: &str,
    body: B,
    tx: Sender<String>,
) -> Result<Bytes, (StatusCode, String)>
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
        tracing::info!("{direction} body = {body:?}");
    }

    Ok(bytes)
}

// test if number is of factor
// factor = 2 Number = 4 result true
// factor = 2 Number = 5 result false
// factor = 0 false
fn is_factor(factor: i32, number: i32) -> bool {
    if factor == 0 {
        return false;
    }
    number % factor == 0
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

// We use static route matchers ("/logo.svg") to serve our home
// page.
async fn logo_handler() -> impl IntoResponse {
    static_handler(Path("logo.svg".to_string())).await
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
