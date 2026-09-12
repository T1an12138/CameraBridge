import Foundation
import SwiftData

@MainActor
final class StagingRetentionService {
    /// 仅删除已成功导入、明确标记为已导出，且 24 小时保留期已结束的文件。
    /// 失败或尚未导出的原始文件绝不会自动删除。
    func removeEligibleFiles(records: [TransferRecord], modelContext: ModelContext) {
        let now = Date.now
        for record in records where record.photosImported && record.exportedToFiles {
            guard let deadline = record.preserveUntil,
                  deadline <= now,
                  let localPath = record.localPath else { continue }
            do {
                try FileManager.default.removeItem(at: URL(filePath: localPath))
                record.localPath = nil
            } catch CocoaError.fileNoSuchFile {
                record.localPath = nil
            } catch {
                continue
            }
        }
        try? modelContext.save()
    }
}
