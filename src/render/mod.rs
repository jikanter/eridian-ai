mod markdown;
mod stream;

pub use self::markdown::{MarkdownRender, RenderOptions};
use self::stream::{markdown_stream, raw_stream};

use crate::utils::{error_text, pretty_error, AbortSignal, IS_STDOUT_TERMINAL};
use crate::{client::SseEvent, config::GlobalConfig};

use anyhow::Result;
use tokio::sync::mpsc::UnboundedReceiver;

pub async fn render_stream(
    rx: UnboundedReceiver<SseEvent>,
    config: &GlobalConfig,
    abort_signal: AbortSignal,
) -> Result<()> {
    let ret = if *IS_STDOUT_TERMINAL && config.read().highlight {
        let render_options = config.read().render_options()?;
        let mut render = MarkdownRender::init(render_options)?;
        markdown_stream(rx, &mut render, &abort_signal).await
    } else {
        raw_stream(rx, &abort_signal).await
    };
    ret.map_err(|err| err.context("Failed to reader stream"))
}

pub fn render_error(err: anyhow::Error, output_format: Option<crate::cli::OutputFormat>, code: crate::utils::ExitCode) {
    if matches!(output_format, Some(crate::cli::OutputFormat::Json)) {
        let mut error_obj = serde_json::json!({
            "code": code.as_i32(),
            "category": code.category_name(),
            "message": err.to_string(),
        });
        if let Some(typed) = err.downcast_ref::<crate::utils::AichatError>() {
            error_obj["context"] = typed.to_json_context();
        }
        let payload = serde_json::json!({ "error": error_obj });
        eprintln!("{}", serde_json::to_string(&payload).unwrap_or_default());
    } else {
        eprintln!("{}", error_text(&pretty_error(&err)));
    }
}
