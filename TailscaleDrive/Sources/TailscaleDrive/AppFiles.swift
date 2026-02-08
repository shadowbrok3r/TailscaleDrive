import Foundation

enum AppFiles {
    static let downloadsFolderName = "Downloads"

    /// Documents/Downloads (visible in Files if UIFileSharingEnabled is true)
    static func downloadsDirectory() throws -> URL {
        let fm = FileManager.default
        let docs = try fm.url(
            for: .documentDirectory,
            in: .userDomainMask,
            appropriateFor: nil,
            create: false
        )
        let downloads = docs.appendingPathComponent(downloadsFolderName, isDirectory: true)
        try fm.createDirectory(at: downloads, withIntermediateDirectories: true)
        return downloads
    }

    @discardableResult
    static func writeDownload(_ data: Data, baseName: String, ext: String? = nil) throws -> URL {
        let dir = try downloadsDirectory()

        let safeBase = baseName.isEmpty ? "file" : baseName
        let fileName = ext.map { "\(safeBase).\($0)" } ?? safeBase

        var url = dir.appendingPathComponent(fileName, isDirectory: false)
        if !FileManager.default.fileExists(atPath: url.path) {
            try data.write(to: url, options: .atomic)
            return url
        }

        let stem = url.deletingPathExtension().lastPathComponent
        let fileExt = url.pathExtension

        for n in 2...10_000 {
            let alt = fileExt.isEmpty
                ? "\(stem) (\(n))"
                : "\(stem) (\(n)).\(fileExt)"
            url = dir.appendingPathComponent(alt)
            if !FileManager.default.fileExists(atPath: url.path) {
                try data.write(to: url, options: .atomic)
                return url
            }
        }

        throw NSError(domain: "AppFiles", code: 1, userInfo: [
            NSLocalizedDescriptionKey: "Could not allocate unique filename"
        ])
    }
}
