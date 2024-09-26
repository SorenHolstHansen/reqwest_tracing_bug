use axum::{extract::State, http::Request, response::Response, routing::get, Router};
use opentelemetry::{global, trace::TracerProvider};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    resource::{EnvResourceDetector, SdkProvidedResourceDetector, TelemetryResourceDetector},
    runtime,
    trace::{BatchConfig, Config},
    Resource,
};
use reqwest::StatusCode;
use std::time::Duration;
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;
use tracing::Level;
use tracing::{info_span, Span};
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Clone)]
struct AppState {
    reqwest_client: reqwest::Client,
}

#[tokio::main]
async fn main() {
    let provider = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_trace_config(Config::default().with_resource(Resource::from_detectors(
            Duration::from_secs(5),
            vec![
                Box::new(EnvResourceDetector::new()),
                Box::new(SdkProvidedResourceDetector {}),
                Box::new(TelemetryResourceDetector {}),
            ],
        )))
        .with_batch_config(BatchConfig::default())
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_endpoint("http://localhost:4317"),
        )
        .install_batch(runtime::Tokio)
        .unwrap();

    global::set_tracer_provider(provider.clone());
    let otel_tracer = provider.tracer("tracing-otel-subscriber");

    tracing_subscriber::registry()
        .with(tracing_subscriber::filter::LevelFilter::from_level(
            Level::TRACE,
        ))
        .with(tracing_subscriber::fmt::layer())
        .with(OpenTelemetryLayer::new(otel_tracer))
        .init();

    let reqwest_client = reqwest::Client::new();

    let state = AppState { reqwest_client };

    let app = Router::new()
        .route("/", get(handler))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|_request: &Request<_>| {
                    info_span!("http_request", took = tracing::field::Empty,)
                })
                .on_response(|_response: &Response, latency: Duration, span: &Span| {
                    span.record("took", latency.as_secs());
                }),
        )
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:8008").await.unwrap();
    tracing::debug!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

#[tracing::instrument(skip(state))]
async fn handler(State(state): State<AppState>) -> StatusCode {
    let _a = state
        .reqwest_client
        .get("https://facebook.com")
        .send()
        .await;
    StatusCode::OK
}
