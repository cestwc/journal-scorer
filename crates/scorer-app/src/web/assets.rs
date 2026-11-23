//! The browser UI: static files compiled into the binary.

use axum::http::header;
use axum::response::{Html, IntoResponse};

const INDEX_HTML: &str = include_str!("../../static/index.html");
const APP_JS: &str = include_str!("../../static/app.js");
const APP_CSS: &str = include_str!("../../static/app.css");

pub async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

pub async fn app_js() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8")], APP_JS)
}

pub async fn app_css() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], APP_CSS)
}
