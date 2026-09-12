import Foundation
import Photos

protocol PhotoImporting: Sendable {
    func importGroup(_ files: [DownloadedAssetFile]) async -> PhotoImportResult
}

actor PhotoLibraryImporter: PhotoImporting {
    func importGroup(_ files: [DownloadedAssetFile]) async -> PhotoImportResult {
        let status = await PHPhotoLibrary.requestAuthorization(for: .addOnly)
        guard status == .authorized || status == .limited else {
            return .retainedForFiles(files.map(\.url), reason: .photosDenied)
        }

        let jpeg = files.first(where: { $0.asset.format == .jpeg })
        let nef = files.first(where: { $0.asset.format == .nef })

        if let jpeg, let nef {
            let resourceTypes = [
                NSNumber(value: PHAssetResourceType.photo.rawValue),
                NSNumber(value: PHAssetResourceType.alternatePhoto.rawValue),
            ]
            if PHAssetCreationRequest.supportsAssetResourceTypes(resourceTypes) {
                do {
                    try await createAsset(primary: jpeg, alternate: nef)
                    return .imported
                } catch {
                    do {
                        try await createAsset(primary: jpeg, alternate: nil)
                        return .jpegOnly(nefURL: nef.url)
                    } catch {
                        return .retainedForFiles(files.map(\.url), reason: .other(error.localizedDescription))
                    }
                }
            }

            do {
                try await createAsset(primary: jpeg, alternate: nil)
                return .jpegOnly(nefURL: nef.url)
            } catch {
                return .retainedForFiles(files.map(\.url), reason: .other(error.localizedDescription))
            }
        }

        guard let primary = jpeg ?? nef else { return .imported }
        do {
            try await createAsset(primary: primary, alternate: nil)
            return .imported
        } catch {
            return .retainedForFiles(files.map(\.url), reason: .other(error.localizedDescription))
        }
    }

    private func createAsset(
        primary: DownloadedAssetFile,
        alternate: DownloadedAssetFile?
    ) async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, any Error>) in
            PHPhotoLibrary.shared().performChanges {
                let request = PHAssetCreationRequest.forAsset()
                let primaryOptions = PHAssetResourceCreationOptions()
                primaryOptions.originalFilename = primary.asset.filename
                request.addResource(with: .photo, fileURL: primary.url, options: primaryOptions)

                if let alternate {
                    let alternateOptions = PHAssetResourceCreationOptions()
                    alternateOptions.originalFilename = alternate.asset.filename
                    request.addResource(
                        with: .alternatePhoto,
                        fileURL: alternate.url,
                        options: alternateOptions
                    )
                }
            } completionHandler: { success, error in
                if success {
                    continuation.resume()
                } else {
                    continuation.resume(
                        throwing: error ?? CameraBridgeError.other("Photos rejected the asset")
                    )
                }
            }
        }
    }
}
