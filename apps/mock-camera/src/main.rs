use clap::Parser;
use mock_camera::{MockCameraConfig, MockCameraServer};
use std::time::Duration;
use tracing::info;

#[derive(Debug, Parser)]
#[command(name = "mock-camera", about = "确定性的类 Nikon PTP/IP 测试对端")]
struct Arguments {
    #[arg(long, default_value = "127.0.0.1:15740")]
    listen: String,
    #[arg(long, default_value_t = 500)]
    assets: usize,
    #[arg(long, default_value_t = 16 * 1024)]
    chunk_size: usize,
    #[arg(long, default_value_t = 0)]
    delay_ms: u64,
    #[arg(long)]
    disconnect_first_download_after: Option<u64>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let arguments = Arguments::parse();
    let server = MockCameraServer::bind(
        &arguments.listen,
        MockCameraConfig {
            asset_count: arguments.assets,
            chunk_size: arguments.chunk_size,
            response_delay: Duration::from_millis(arguments.delay_ms),
            disconnect_first_download_after: arguments.disconnect_first_download_after,
            ..MockCameraConfig::default()
        },
    )
    .await?;
    info!(address = %server.address(), "mock camera ready; terminate the process to stop");
    std::future::pending::<()>().await;
    #[allow(unreachable_code)]
    {
        server.stop().await?;
        Ok(())
    }
}
