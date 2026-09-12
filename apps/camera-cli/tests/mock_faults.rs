use camera_core::{
    CameraDriver, CameraErrorCode, ConnectionConfig, DownloadRequest, MediaQuery, TransferPhase,
};
use mock_camera::{MockCameraConfig, MockCameraServer};
use nikon_driver::NikonZ6IiDriver;
use std::net::SocketAddr;
use std::time::Duration;
use uuid::Uuid;

fn connection(address: SocketAddr, request_timeout: Duration) -> ConnectionConfig {
    ConnectionConfig {
        host: address.ip().to_string(),
        port: address.port(),
        client_name: "fault integration test".into(),
        client_id: Uuid::new_v4(),
        connect_timeout: Duration::from_secs(2),
        request_timeout,
    }
}

#[tokio::test]
async fn busy_response_maps_to_actionable_category() {
    let server = MockCameraServer::spawn(MockCameraConfig {
        busy_first_operation: true,
        ..MockCameraConfig::default()
    })
    .await
    .unwrap();
    let error = NikonZ6IiDriver
        .connect(connection(server.address(), Duration::from_secs(2)))
        .await
        .err()
        .expect("busy response should reject initial device-info request");
    assert_eq!(error.code(), CameraErrorCode::CameraBusy);
    server.stop().await.unwrap();
}

#[tokio::test]
async fn slow_response_obeys_request_timeout() {
    let server = MockCameraServer::spawn(MockCameraConfig {
        response_delay: Duration::from_millis(250),
        ..MockCameraConfig::default()
    })
    .await
    .unwrap();
    let error = NikonZ6IiDriver
        .connect(connection(server.address(), Duration::from_millis(40)))
        .await
        .err()
        .expect("slow device-info response should time out");
    assert_eq!(error.code(), CameraErrorCode::Timeout);
    server.stop().await.unwrap();
}

#[tokio::test]
async fn malformed_frame_is_rejected_as_protocol_error() {
    let server = MockCameraServer::spawn(MockCameraConfig {
        malformed_first_response: true,
        ..MockCameraConfig::default()
    })
    .await
    .unwrap();
    let error = NikonZ6IiDriver
        .connect(connection(server.address(), Duration::from_secs(2)))
        .await
        .err()
        .expect("malformed response must not be accepted");
    assert_eq!(error.code(), CameraErrorCode::Protocol);
    server.stop().await.unwrap();
}

#[tokio::test]
async fn interrupted_download_preserves_partial_and_reconnect_recovers() {
    let server = MockCameraServer::spawn(MockCameraConfig {
        asset_count: 4,
        chunk_size: 128,
        disconnect_first_download_after: Some(512),
        ..MockCameraConfig::default()
    })
    .await
    .unwrap();
    let directory = tempfile::tempdir().unwrap();

    let first_session = NikonZ6IiDriver
        .connect(connection(server.address(), Duration::from_secs(2)))
        .await
        .unwrap();
    let asset = first_session
        .list_media(MediaQuery::default())
        .await
        .unwrap()
        .items
        .into_iter()
        .next()
        .unwrap();
    let destination = directory.path().join(&asset.filename);
    let first_job = first_session
        .start_download(DownloadRequest {
            asset_id: asset.id.clone(),
            destination_path: destination.clone(),
            expected_size: asset.size_bytes,
            resume_partial: false,
            overwrite: false,
        })
        .await
        .unwrap();
    let first_result = first_job.wait().await;
    assert_eq!(first_result.phase, TransferPhase::Failed);
    assert!(!destination.exists());
    let partial = destination.with_file_name(format!("{}.partial", asset.filename));
    let partial_length = tokio::fs::metadata(&partial).await.unwrap().len();
    assert!(partial_length > 0 && partial_length < asset.size_bytes);

    let second_session = NikonZ6IiDriver
        .connect(connection(server.address(), Duration::from_secs(2)))
        .await
        .unwrap();
    let second_job = second_session
        .start_download(DownloadRequest {
            asset_id: asset.id,
            destination_path: destination.clone(),
            expected_size: asset.size_bytes,
            resume_partial: false,
            overwrite: false,
        })
        .await
        .unwrap();
    let second_result = second_job.wait().await;
    assert_eq!(second_result.phase, TransferPhase::Completed);
    assert_eq!(
        tokio::fs::metadata(destination).await.unwrap().len(),
        asset.size_bytes
    );
    assert!(!partial.exists());
    second_session.disconnect().await.unwrap();
    server.stop().await.unwrap();
}

#[tokio::test]
async fn cancellation_before_streaming_never_completes_destination() {
    let server = MockCameraServer::spawn(MockCameraConfig {
        asset_count: 2,
        response_delay: Duration::from_millis(50),
        ..MockCameraConfig::default()
    })
    .await
    .unwrap();
    let session = NikonZ6IiDriver
        .connect(connection(server.address(), Duration::from_secs(2)))
        .await
        .unwrap();
    let asset = session
        .list_media(MediaQuery::default())
        .await
        .unwrap()
        .items
        .into_iter()
        .next()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join(&asset.filename);
    let job = session
        .start_download(DownloadRequest {
            asset_id: asset.id,
            destination_path: destination.clone(),
            expected_size: asset.size_bytes,
            resume_partial: false,
            overwrite: false,
        })
        .await
        .unwrap();
    job.cancel();
    job.cancel();
    assert_eq!(job.wait().await.phase, TransferPhase::Cancelled);
    assert!(!destination.exists());
    session.disconnect().await.unwrap();
    server.stop().await.unwrap();
}
