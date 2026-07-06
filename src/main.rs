// Entry point for aws-s3-explorer.
//
// IMPORTANT: Always run with `cargo run --release` for testing.
// Debug builds of egui are too slow to use — immediate-mode rendering
// with zero optimisations produces single-digit FPS.
//
// 1. Initialise tracing (log to stderr; respects `RUST_LOG` env var).
// 2. Load `AppConfig` from JSON via `config::AppConfig::load_or_create`.
// 3. Build a multi-thread tokio `Runtime` MANUALLY (not `#[tokio::main]`).
// 4. Clone the `Runtime` `Handle` for passing into `App`.
// 5. Call `eframe::run_native()` — this takes over the thread.
//
// Why manual runtime instead of `#[tokio::main]`?
//   `eframe::run_native()` never returns (it becomes the OS event loop).
//   `#[tokio::main]` wraps `main()` in `block_on()`, which conflicts.
//   By building the runtime ourselves we keep the `Handle` alive and can
//   spawn tasks from the synchronous eframe render loop.
//
// The `Runtime` variable must live until the end of `main()` to keep the
// background threads alive. When eframe exits (user closes window),
// `main()` returns, `Runtime` drops, and all background tasks are cancelled.

mod app;
mod config;
mod fs;
mod s3;
mod sync;
mod types;
mod ui;

fn main() -> anyhow::Result<()> {
    // Logging: RUST_LOG=aws_s3_explorer=debug,warn
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "aws_s3_explorer=info,warn".into()),
        )
        .init();

    let config = config::AppConfig::load_or_create()?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()?;
    let tokio_handle = runtime.handle().clone();

    // Resolve credentials once at startup before the GUI takes over.
    let s3_client = runtime.block_on(s3::client::build_client())?;

    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
        .expect("embedded assets/icon.png is a valid PNG (guarded by embedded_icon_decodes test)");

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("AWS S3 Explorer")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 500.0])
            .with_icon(icon),
        ..Default::default()
    };

    eframe::run_native(
        "aws-s3-explorer",
        native_options,
        Box::new(move |cc| {
            Ok(Box::new(app::S3ExplorerApp::new(
                cc,
                tokio_handle,
                s3_client,
                config,
            )))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {e}"))?;

    Ok(())
    // runtime drops here — all tokio tasks cancelled cleanly.
}

#[cfg(test)]
mod tests {
    #[test]
    fn embedded_icon_decodes() {
        let bytes = include_bytes!("../assets/icon.png");
        let result = eframe::icon_data::from_png_bytes(bytes);
        assert!(
            result.is_ok(),
            "embedded assets/icon.png failed to decode: {:?}",
            result.err()
        );
    }
}
