use camera_core::{
    CameraAsset, CameraDriver, CameraError, CameraErrorCode, CameraSession, ConnectionConfig,
    DownloadRequest, MediaFormat, MediaPage, MediaQuery, TransferPhase,
};
use clap::{Arg, ArgAction, Args, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use nikon_driver::NikonZ6IiDriver;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::time::{sleep, timeout};
use tracing::{info, warn};
use uuid::Uuid;

const ROOT_HELP_TEMPLATE: &str =
    "{about-with-newline}\n用法：{usage}\n\n命令：\n{subcommands}\n选项：\n{options}";
const SUBCOMMAND_HELP_TEMPLATE: &str = "{about-with-newline}\n用法：{usage}\n\n选项：\n{options}";

#[derive(Debug, Parser)]
#[command(
    name = "camera-cli",
    about = "CameraBridge PTP/IP 探测与验收命令行工具"
)]
struct Application {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 检查一个明确指定的端点。本命令绝不会扫描子网。
    Network(ConnectionArguments),
    /// 连接相机、验证 Z6 II 能力配置，并加载第一页媒体。
    Probe(ConnectionArguments),
    /// 以脱敏 JSON 输出相机媒体。
    List {
        #[command(flatten)]
        connection: ConnectionArguments,
        #[arg(long, value_enum)]
        format: Vec<FormatArgument>,
        #[arg(long, default_value_t = 60)]
        page_size: u32,
        #[arg(long, default_value_t = false)]
        all: bool,
    },
    /// 将一个不透明资产 ID 对应的文件下载到应用自有目录。
    Download {
        #[command(flatten)]
        connection: ConnectionArguments,
        #[arg(long)]
        asset_id: String,
        #[arg(long)]
        destination: PathBuf,
        #[arg(long, default_value_t = false)]
        overwrite: bool,
    },
    /// 执行可重复的连接验收检查，以及可选的下载验收检查。
    Validate(ValidationArguments),
}

#[derive(Clone, Debug, Args)]
struct ValidationArguments {
    #[command(flatten)]
    connection: ConnectionArguments,
    #[arg(long, default_value_t = 20)]
    cycles: u32,
    #[arg(long, default_value_t = 500)]
    minimum_items: u64,
    #[arg(long, default_value_t = 15_000)]
    max_p95_ready_ms: u128,
    #[arg(long, default_value_t = 0)]
    download_count: usize,
    #[arg(long, default_value_t = 0)]
    required_disconnect_recoveries: u32,
    #[arg(long)]
    destination: Option<PathBuf>,
    #[arg(long)]
    report: Option<PathBuf>,
}

#[derive(Clone, Debug, Args)]
struct ConnectionArguments {
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    #[arg(long, default_value_t = ptpip::DEFAULT_PORT)]
    port: u16,
    #[arg(long, default_value_t = 5_000)]
    connect_timeout_ms: u64,
    #[arg(long, default_value_t = 15_000)]
    request_timeout_ms: u64,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum FormatArgument {
    Jpeg,
    Nef,
}

impl From<FormatArgument> for MediaFormat {
    fn from(value: FormatArgument) -> Self {
        match value {
            FormatArgument::Jpeg => Self::Jpeg,
            FormatArgument::Nef => Self::Nef,
        }
    }
}

#[derive(Debug, Serialize)]
struct ProbeSummary {
    camera_id: String,
    manufacturer: String,
    model: String,
    firmware: Option<String>,
    profile_id: String,
    first_page_items: usize,
    total_items: Option<u64>,
    elapsed_ms: u128,
}

#[derive(Debug, Serialize)]
struct AssetSummary {
    id: String,
    capture_group_id: String,
    format: MediaFormat,
    size_bytes: u64,
    captured_at: Option<String>,
    thumbnail_available: bool,
}

#[derive(Debug, Serialize)]
struct DownloadEvidence {
    asset_id: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct ValidationReport {
    app_version: &'static str,
    host_os: String,
    schema_version: u32,
    generated_at_utc: String,
    target_model: &'static str,
    target_firmware: &'static str,
    actual_model: Option<String>,
    actual_firmware: Option<String>,
    cycles: u32,
    first_attempt_successes: u32,
    minimum_first_attempt_successes: u32,
    recovered_with_one_retry: u32,
    failed_cycles: u32,
    p95_ready_ms: Option<u128>,
    max_p95_ready_ms: u128,
    minimum_items: u64,
    enumerated_items_min: u64,
    enumerated_items_max: u64,
    pagination_consistent: bool,
    connection_acceptance_passed: bool,
    requested_download_count: usize,
    completed_download_count: usize,
    required_disconnect_recoveries: u32,
    interrupted_downloads_recovered: u32,
    repeat_sha256_match: Option<bool>,
    requested_checks_passed: bool,
    downloads: Vec<DownloadEvidence>,
}

#[derive(Debug)]
struct ConnectionCycleEvidence {
    model: String,
    firmware: Option<String>,
    enumerated_items: u64,
    first_page_ready_ms: u128,
}

fn localized_command(command: clap::Command, is_root: bool) -> clap::Command {
    let template = if is_root {
        ROOT_HELP_TEMPLATE
    } else {
        SUBCOMMAND_HELP_TEMPLATE
    };
    command
        .disable_help_flag(true)
        .disable_help_subcommand(true)
        .help_template(template)
        .arg(
            Arg::new("help")
                .short('h')
                .long("help")
                .action(ArgAction::Help)
                .help("显示帮助"),
        )
        .mut_subcommands(|subcommand| localized_command(subcommand, false))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .init();
    let matches = localized_command(Application::command(), true).get_matches();
    match Application::from_arg_matches(&matches)?.command {
        Command::Network(arguments) => network(arguments).await?,
        Command::Probe(arguments) => probe(arguments).await?,
        Command::List {
            connection,
            format,
            page_size,
            all,
        } => list(connection, format, page_size, all).await?,
        Command::Download {
            connection,
            asset_id,
            destination,
            overwrite,
        } => download(connection, asset_id, destination, overwrite).await?,
        Command::Validate(arguments) => validate(arguments).await?,
    }
    Ok(())
}

async fn network(arguments: ConnectionArguments) -> Result<(), Box<dyn std::error::Error>> {
    let remote = format!("{}:{}", arguments.host, arguments.port);
    let stream = timeout(
        Duration::from_millis(arguments.connect_timeout_ms),
        TcpStream::connect(&remote),
    )
    .await??;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "reachable": true,
            "local_address": stream.local_addr()?.to_string(),
            "remote_address": stream.peer_addr()?.to_string(),
            "scan_performed": false
        }))?
    );
    Ok(())
}

async fn probe(arguments: ConnectionArguments) -> Result<(), Box<dyn std::error::Error>> {
    let started = Instant::now();
    let session = connect(&arguments).await?;
    let info = session.info().await?;
    let capabilities = session.capabilities().await?;
    let page = session.list_media(MediaQuery::default()).await?;
    let summary = ProbeSummary {
        camera_id: info.id.to_string(),
        manufacturer: info.manufacturer,
        model: info.model,
        firmware: info.firmware_version,
        profile_id: capabilities.profile_id,
        first_page_items: page.items.len(),
        total_items: page.total_items,
        elapsed_ms: started.elapsed().as_millis(),
    };
    println!("{}", serde_json::to_string_pretty(&summary)?);
    session.disconnect().await?;
    Ok(())
}

async fn list(
    arguments: ConnectionArguments,
    formats: Vec<FormatArgument>,
    page_size: u32,
    all: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let session = connect(&arguments).await?;
    let filter: BTreeSet<_> = formats.into_iter().map(Into::into).collect();
    let mut cursor = None;
    let mut summaries = Vec::new();
    loop {
        let page = session
            .list_media(MediaQuery {
                formats: filter.clone(),
                page_size,
                cursor,
            })
            .await?;
        summaries.extend(page.items.into_iter().map(|asset| {
            AssetSummary {
                id: asset.id.to_string(),
                capture_group_id: asset.capture_group_id.to_string(),
                format: asset.format,
                size_bytes: asset.size_bytes,
                captured_at: asset
                    .captured_at
                    .map(|date| date.format("%Y-%m-%dT%H:%M:%S").to_string()),
                thumbnail_available: asset.thumbnail_available,
            }
        }));
        cursor = page.next_cursor;
        if !all || cursor.is_none() {
            break;
        }
    }
    println!("{}", serde_json::to_string_pretty(&summaries)?);
    session.disconnect().await?;
    Ok(())
}

async fn download(
    arguments: ConnectionArguments,
    asset_id: String,
    destination: PathBuf,
    overwrite: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let session = connect(&arguments).await?;
    let asset_id = camera_core::AssetId::parse(asset_id)?;
    let asset = find_asset(&session, &asset_id).await?;
    let path = download_path(&destination, &asset);
    let job = session
        .start_download(DownloadRequest {
            asset_id: asset.id.clone(),
            destination_path: path.clone(),
            expected_size: asset.size_bytes,
            resume_partial: false,
            overwrite,
        })
        .await?;
    wait_with_progress(&job).await?;
    println!(
        "{}",
        serde_json::to_string_pretty(&hash_evidence(&asset.id, &path).await?)?
    );
    session.disconnect().await?;
    Ok(())
}

async fn validate(options: ValidationArguments) -> Result<(), Box<dyn std::error::Error>> {
    let ValidationArguments {
        connection: arguments,
        cycles,
        minimum_items,
        max_p95_ready_ms,
        download_count,
        required_disconnect_recoveries,
        destination,
        report: report_path,
    } = options;
    if cycles == 0 {
        return Err("cycles must be greater than zero".into());
    }
    if download_count > 0 && destination.is_none() {
        return Err("--destination is required when --download-count is non-zero".into());
    }
    if minimum_items == 0 || max_p95_ready_ms == 0 {
        return Err("--minimum-items and --max-p95-ready-ms must be greater than zero".into());
    }
    if required_disconnect_recoveries > 0 && download_count == 0 {
        return Err("--required-disconnect-recoveries requires downloads".into());
    }
    let mut timings = Vec::new();
    let mut first_success = 0;
    let mut recovered = 0;
    let mut failures = 0;
    let mut counts = Vec::new();
    let mut actual_model = None;
    let mut actual_firmware = None;
    let mut camera_identity_consistent = true;
    for cycle in 0..cycles {
        let cycle_started = Instant::now();
        match connect_and_count(&arguments).await {
            Ok(evidence) => {
                first_success += 1;
                merge_cycle_evidence(
                    &evidence,
                    &mut actual_model,
                    &mut actual_firmware,
                    &mut camera_identity_consistent,
                );
                counts.push(evidence.enumerated_items);
                timings.push(evidence.first_page_ready_ms);
            }
            Err(first_error) => {
                warn!(cycle, error = %first_error, "first connection attempt failed; retrying once");
                sleep(Duration::from_millis(500)).await;
                let retry_offset_ms = cycle_started.elapsed().as_millis();
                match connect_and_count(&arguments).await {
                    Ok(evidence) => {
                        recovered += 1;
                        merge_cycle_evidence(
                            &evidence,
                            &mut actual_model,
                            &mut actual_firmware,
                            &mut camera_identity_consistent,
                        );
                        counts.push(evidence.enumerated_items);
                        timings.push(retry_offset_ms.saturating_add(evidence.first_page_ready_ms));
                    }
                    Err(second_error) => {
                        failures += 1;
                        warn!(cycle, error = %second_error, "validation cycle failed");
                    }
                }
            }
        }
    }
    let mut downloads = Vec::new();
    let mut repeat_sha256_match = None;
    let mut interrupted_downloads_recovered = 0_u32;
    if download_count > 0 {
        let directory = destination.expect("validated destination");
        fs::create_dir_all(&directory).await?;
        let mut session = connect(&arguments).await?;
        let assets: Vec<_> = enumerate_media(&session)
            .await?
            .into_iter()
            .take(download_count)
            .collect();
        if assets.len() != download_count {
            return Err(format!(
                "requested {download_count} downloads but found only {} assets",
                assets.len()
            )
            .into());
        }
        for asset in &assets {
            let path = download_path(&directory, asset);
            let (next_session, evidence, recovered) =
                download_with_one_reconnect(&arguments, session, asset, &path).await?;
            session = next_session;
            interrupted_downloads_recovered += u32::from(recovered);
            downloads.push(evidence);
        }
        if let Some(asset) = assets.first() {
            let path = download_path(&directory, asset);
            let expected_hash = downloads.first().map(|evidence| evidence.sha256.clone());
            let (next_session, repeated, recovered) =
                download_with_one_reconnect(&arguments, session, asset, &path).await?;
            session = next_session;
            interrupted_downloads_recovered += u32::from(recovered);
            repeat_sha256_match = expected_hash
                .as_deref()
                .map(|expected| expected == repeated.sha256);
        }
        session.disconnect().await?;
    }
    timings.sort_unstable();
    let p95_ready_ms = percentile_95(&timings);
    counts.sort_unstable();
    let enumerated_items_min = counts.first().copied().unwrap_or(0);
    let enumerated_items_max = counts.last().copied().unwrap_or(0);
    let pagination_consistent = camera_identity_consistent
        && !counts.is_empty()
        && enumerated_items_min == enumerated_items_max;
    let minimum_first_attempt_successes = minimum_first_attempt_successes(cycles);
    let connection_acceptance_passed = failures == 0
        && first_success >= minimum_first_attempt_successes
        && p95_ready_ms.is_some_and(|value| value <= max_p95_ready_ms)
        && enumerated_items_min >= minimum_items
        && pagination_consistent;
    let download_checks_passed = downloads.len() == download_count
        && (download_count == 0 || repeat_sha256_match == Some(true))
        && interrupted_downloads_recovered >= required_disconnect_recoveries;
    let requested_checks_passed = connection_acceptance_passed && download_checks_passed;
    let report = ValidationReport {
        app_version: env!("CARGO_PKG_VERSION"),
        host_os: host_os_description(),
        schema_version: 3,
        generated_at_utc: chrono::Utc::now().to_rfc3339(),
        target_model: "Nikon Z6 II",
        target_firmware: "1.70",
        actual_model,
        actual_firmware,
        cycles,
        first_attempt_successes: first_success,
        minimum_first_attempt_successes,
        recovered_with_one_retry: recovered,
        failed_cycles: failures,
        p95_ready_ms,
        max_p95_ready_ms,
        minimum_items,
        enumerated_items_min,
        enumerated_items_max,
        pagination_consistent,
        connection_acceptance_passed,
        requested_download_count: download_count,
        completed_download_count: downloads.len(),
        required_disconnect_recoveries,
        interrupted_downloads_recovered,
        repeat_sha256_match,
        requested_checks_passed,
        downloads,
    };
    let json = serde_json::to_string_pretty(&report)?;
    println!("{json}");
    if let Some(path) = report_path {
        fs::write(path, json).await?;
    }
    if !requested_checks_passed {
        return Err("validation thresholds were not met; inspect the redacted report".into());
    }
    Ok(())
}

fn connection_config(arguments: &ConnectionArguments) -> ConnectionConfig {
    ConnectionConfig {
        host: arguments.host.clone(),
        port: arguments.port,
        client_name: "CameraBridge Windows Probe".to_owned(),
        client_id: Uuid::new_v4(),
        connect_timeout: Duration::from_millis(arguments.connect_timeout_ms),
        request_timeout: Duration::from_millis(arguments.request_timeout_ms),
    }
}

async fn connect(
    arguments: &ConnectionArguments,
) -> Result<Arc<dyn CameraSession>, camera_core::CameraError> {
    NikonZ6IiDriver.connect(connection_config(arguments)).await
}

async fn connect_and_count(
    arguments: &ConnectionArguments,
) -> Result<ConnectionCycleEvidence, CameraError> {
    let started = Instant::now();
    let session = connect(arguments).await?;
    let result: Result<ConnectionCycleEvidence, CameraError> = async {
        let info = session.info().await?;
        let first_page = session
            .list_media(MediaQuery {
                formats: BTreeSet::new(),
                page_size: 60,
                cursor: None,
            })
            .await?;
        let first_page_ready_ms = started.elapsed().as_millis();
        let assets = enumerate_media_from_first_page(&session, first_page).await?;
        Ok(ConnectionCycleEvidence {
            model: info.model,
            firmware: info.firmware_version,
            enumerated_items: assets.len() as u64,
            first_page_ready_ms,
        })
    }
    .await;
    let disconnect_result = session.disconnect().await;
    let evidence = result?;
    disconnect_result?;
    Ok(evidence)
}

async fn enumerate_media(
    session: &Arc<dyn CameraSession>,
) -> Result<Vec<CameraAsset>, CameraError> {
    let first_page = session
        .list_media(MediaQuery {
            formats: BTreeSet::new(),
            page_size: 60,
            cursor: None,
        })
        .await?;
    enumerate_media_from_first_page(session, first_page).await
}

async fn enumerate_media_from_first_page(
    session: &Arc<dyn CameraSession>,
    mut page: MediaPage,
) -> Result<Vec<CameraAsset>, CameraError> {
    const MAX_PAGES: usize = 10_000;

    let mut seen_cursors = BTreeSet::new();
    let mut seen_assets = BTreeSet::new();
    let mut assets = Vec::new();
    let mut total_hint = None;
    for _ in 0..MAX_PAGES {
        if let Some(total) = page.total_items {
            if total_hint.is_some_and(|previous| previous != total) {
                return Err(CameraError::protocol(
                    "camera changed total_items during pagination",
                ));
            }
            total_hint = Some(total);
        }
        let page_was_empty = page.items.is_empty();
        for asset in page.items {
            if !seen_assets.insert(asset.id.to_string()) {
                return Err(CameraError::protocol(
                    "camera returned a duplicate media identity",
                ));
            }
            assets.push(asset);
        }
        let Some(next) = page.next_cursor else {
            if total_hint.is_some_and(|total| total != assets.len() as u64) {
                return Err(CameraError::protocol(
                    "enumerated item count does not match total_items",
                ));
            }
            return Ok(assets);
        };
        if page_was_empty {
            return Err(CameraError::protocol(
                "camera returned an empty page with a next cursor",
            ));
        }
        if !seen_cursors.insert(next.as_str().to_owned()) {
            return Err(CameraError::protocol("camera repeated a pagination cursor"));
        }
        page = session
            .list_media(MediaQuery {
                formats: BTreeSet::new(),
                page_size: 60,
                cursor: Some(next),
            })
            .await?;
    }
    Err(CameraError::protocol(
        "camera exceeded the validation page limit",
    ))
}

fn merge_cycle_evidence(
    evidence: &ConnectionCycleEvidence,
    actual_model: &mut Option<String>,
    actual_firmware: &mut Option<String>,
    consistent: &mut bool,
) {
    if actual_model
        .as_ref()
        .is_some_and(|value| value != &evidence.model)
    {
        *consistent = false;
    }
    if actual_firmware
        .as_ref()
        .is_some_and(|value| Some(value) != evidence.firmware.as_ref())
    {
        *consistent = false;
    }
    if actual_model.is_none() {
        *actual_model = Some(evidence.model.clone());
    }
    if actual_firmware.is_none() {
        actual_firmware.clone_from(&evidence.firmware);
    }
}

fn minimum_first_attempt_successes(cycles: u32) -> u32 {
    cycles.saturating_mul(95).div_ceil(100)
}

fn download_path(directory: &Path, asset: &CameraAsset) -> PathBuf {
    let filename = asset.filename.replace(['/', '\\'], "_");
    directory.join(format!("{}_{}", asset.id, filename))
}

async fn download_with_one_reconnect(
    arguments: &ConnectionArguments,
    session: Arc<dyn CameraSession>,
    asset: &CameraAsset,
    path: &Path,
) -> Result<(Arc<dyn CameraSession>, DownloadEvidence, bool), Box<dyn std::error::Error>> {
    match execute_download(&session, asset, path).await {
        Ok(()) => {
            let evidence = hash_evidence(&asset.id, path).await?;
            Ok((session, evidence, false))
        }
        Err(error) if is_recoverable_download_error(&error) => {
            warn!(
                asset_id = %asset.id,
                error = %error,
                "download interrupted; reconnecting once and restarting the current file"
            );
            let _ = session.disconnect().await;
            sleep(Duration::from_millis(500)).await;
            let replacement = connect(arguments).await?;
            let rebound = find_asset(&replacement, &asset.id).await?;
            execute_download(&replacement, &rebound, path).await?;
            let evidence = hash_evidence(&rebound.id, path).await?;
            Ok((replacement, evidence, true))
        }
        Err(error) => Err(error.into()),
    }
}

async fn execute_download(
    session: &Arc<dyn CameraSession>,
    asset: &CameraAsset,
    path: &Path,
) -> Result<(), CameraError> {
    let job = session
        .start_download(DownloadRequest {
            asset_id: asset.id.clone(),
            destination_path: path.to_owned(),
            expected_size: asset.size_bytes,
            resume_partial: false,
            overwrite: true,
        })
        .await?;
    wait_with_progress(&job).await
}

fn is_recoverable_download_error(error: &CameraError) -> bool {
    matches!(
        error.code(),
        CameraErrorCode::ConnectionFailed
            | CameraErrorCode::Timeout
            | CameraErrorCode::CameraAsleep
            | CameraErrorCode::SessionInterrupted
    )
}

async fn find_asset(
    session: &Arc<dyn CameraSession>,
    id: &camera_core::AssetId,
) -> Result<camera_core::CameraAsset, camera_core::CameraError> {
    let mut cursor = None;
    loop {
        let page = session
            .list_media(MediaQuery {
                formats: BTreeSet::new(),
                page_size: 60,
                cursor,
            })
            .await?;
        if let Some(asset) = page.items.into_iter().find(|asset| &asset.id == id) {
            return Ok(asset);
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            return Err(camera_core::CameraError::ObjectNotFound);
        }
    }
}

async fn wait_with_progress(
    job: &Arc<dyn camera_core::TransferJob>,
) -> Result<(), camera_core::CameraError> {
    loop {
        let snapshot = job.snapshot();
        info!(phase = ?snapshot.phase, transferred = snapshot.bytes_transferred, total = snapshot.total_bytes, "transfer progress");
        if snapshot.phase.is_terminal() {
            return match snapshot.phase {
                TransferPhase::Completed => Ok(()),
                _ => Err(snapshot
                    .error
                    .unwrap_or_else(|| camera_core::CameraError::Internal {
                        message: "transfer ended without an error".into(),
                    })),
            };
        }
        sleep(Duration::from_millis(250)).await;
    }
}

async fn hash_evidence(
    id: &camera_core::AssetId,
    path: &Path,
) -> Result<DownloadEvidence, Box<dyn std::error::Error>> {
    let mut file = fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut bytes = 0_u64;
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        bytes = bytes
            .checked_add(read as u64)
            .ok_or("download size overflow while hashing")?;
    }
    Ok(DownloadEvidence {
        asset_id: id.to_string(),
        bytes,
        sha256: format!("{:x}", hasher.finalize()),
    })
}

fn host_os_description() -> String {
    #[cfg(windows)]
    if let Ok(output) = std::process::Command::new("cmd")
        .args(["/C", "ver"])
        .output()
    {
        let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !version.is_empty() {
            return version;
        }
    }
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

fn percentile_95(sorted: &[u128]) -> Option<u128> {
    if sorted.is_empty() {
        return None;
    }
    let rank = sorted.len().saturating_mul(95).div_ceil(100);
    sorted.get(rank.saturating_sub(1)).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acceptance_threshold_matches_nineteen_of_twenty() {
        assert_eq!(minimum_first_attempt_successes(20), 19);
        assert_eq!(minimum_first_attempt_successes(1), 1);
        assert_eq!(minimum_first_attempt_successes(0), 0);
    }

    #[test]
    fn percentile_uses_nearest_rank() {
        let values: Vec<u128> = (1..=20).collect();
        assert_eq!(percentile_95(&values), Some(19));
        assert_eq!(percentile_95(&[]), None);
    }
}
