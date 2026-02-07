import SwiftUI
import AVFoundation
import UserNotifications

// MARK: - MAIN CONTENT VIEW (The Tab System)
struct ContentView: View {
    var body: some View {
        TabView {
            // Tab 1: The original Monitor
            MonitorView()
            .tabItem {
                Label("Monitor", systemImage: "antenna.radiowaves.left.and.right")
            }
            
            // Tab 2: The new Project Sync
            ProjectView()
            .tabItem {
                Label("Project Sync", systemImage: "arrow.triangle.2.circlepath")
            }
        }
    }
}

// MARK: - TAB 1: MONITOR VIEW 
struct MonitorView: View {
    // 🔴 YOUR SERVER URL
    let serverURL = "https://manjaro-work.taile483f.ts.net/status"

    @StateObject private var watcher = TailscaleWatcher()
    @State private var isShareSheetPresented = false
    @State private var downloadedFileURL: URL?

    var body: some View {
        VStack(spacing: 20) {

            // --- HEADER ---
            Image(systemName: watcher.isConnected ? "antenna.radiowaves.left.and.right" : "antenna.radiowaves.left.and.right.slash")
            .font(.system(size: 60))
            .foregroundColor(watcher.isConnected ? .green : .red)
            .symbolEffect(.bounce, value: watcher.lastFile)

            Text("Taildrop Monitor")
            .font(.largeTitle).bold()

            // --- FILE DISPLAY ---
            VStack(alignment: .leading, spacing: 10) {
                Text("LATEST FILE DETECTED")
                .font(.caption).fontWeight(.heavy).foregroundColor(.gray)

                Text(watcher.lastFile)
                .font(.system(size: 24, weight: .semibold, design: .monospaced))
                .minimumScaleFactor(0.5)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding()
                .background(Color(UIColor.secondarySystemBackground))
                .cornerRadius(12)
            }
            .padding(.horizontal)

            // --- ACTION BUTTON ---
            if watcher.lastFile != "Waiting..." {
                Button(action: { downloadFile() }) {
                    HStack {
                        Image(systemName: "arrow.down.circle.fill")
                        Text("Download & Save")
                    }
                    .font(.headline)
                    .foregroundColor(.white)
                    .padding()
                    .frame(maxWidth: .infinity)
                    .background(Color.blue)
                    .cornerRadius(12)
                }
                .padding(.horizontal)
            }

            Spacer()

            Text(watcher.statusMessage)
            .font(.footnote)
            .foregroundColor(.gray)
        }
        .padding()
        .onAppear {
            watcher.startService(url: serverURL)
        }
        .onChange(of: watcher.shouldAutoDownload) { shouldDownload in
            if shouldDownload {
                downloadFile()
                watcher.shouldAutoDownload = false
            }
        }
        .sheet(isPresented: $isShareSheetPresented) {
            if let url = downloadedFileURL {
                ShareSheet(activityItems: [url])
            }
        }
    }

    func downloadFile() {
        let downloadURLString = serverURL.replacingOccurrences(of: "/status", with: "/download")
        guard let url = URL(string: downloadURLString) else { return }

        watcher.statusMessage = "Downloading..."

        Task {
            do {
                let (localURL, _) = try await URLSession.shared.download(from: url)
                // Rename and Move
                let tempDir = FileManager.default.temporaryDirectory
                let dstURL = tempDir.appendingPathComponent(watcher.lastFile)

                if FileManager.default.fileExists(atPath: dstURL.path) {
                    try FileManager.default.removeItem(at: dstURL)
                }
                try FileManager.default.moveItem(at: localURL, to: dstURL)

                // Update UI on Main Thread
                await MainActor.run {
                    self.downloadedFileURL = dstURL
                    self.isShareSheetPresented = true
                    watcher.statusMessage = "Ready to Save!"
                }
            } catch {
                await MainActor.run {
                    watcher.statusMessage = "Error: \(error.localizedDescription)"
                }
            }
        }
    }
}

// MARK: - TAB 2: PROJECT VIEW (The Sync Engine)
struct ProjectView: View {
    @StateObject private var syncManager = SyncManager()
    @State private var isImporterPresented = false // <--- State for File Picker
    
    var body: some View {
        NavigationView {
            List {
                Section(header: Text("Actions")) {
                    Button(action: { isImporterPresented = true }) {
                        Label("Broadcast New Project", systemImage: "arrow.up.doc.fill")
                    }
                }
                
                Section(header: Text("Project Status")) {
                    if syncManager.outdatedFiles.isEmpty {
                        HStack {
                            Image(systemName: "checkmark.circle.fill").foregroundColor(.green)
                            Text("Synced with Linux")
                        }
                    } else {
                        ForEach(syncManager.outdatedFiles) { file in
                            HStack {
                                VStack(alignment: .leading) {
                                    Text(file.name).font(.headline)
                                    Text("Remote is newer").font(.caption).foregroundColor(.secondary)
                                }
                                Spacer()
                                Button("Pull") { syncManager.pullFile(file) }.buttonStyle(.bordered)
                            }
                        }
                    }
                }
            }
            .navigationTitle("Rust Project")
            .toolbar {
                ToolbarItem(placement: .navigationBarTrailing) {
                    Button(action: { syncManager.checkForUpdates() }) {
                        Image(systemName: "arrow.triangle.2.circlepath")
                    }
                }
            }
            // --- FILE IMPORTER ---
            .fileImporter(
                isPresented: $isImporterPresented,
                allowedContentTypes: [.folder], // Select Folders only
                allowsMultipleSelection: false
            ) { result in
                switch result {
                case .success(let urls):
                    if let url = urls.first {
                        // Start the Upload Process
                        syncManager.uploadProject(folderUrl: url)
                    }
                case .failure(let error):
                    print("Import failed: \(error.localizedDescription)")
                }
            }
        }
        .onAppear { syncManager.checkForUpdates() }
    }
}
// MARK: - WATCHER LOGIC (Concurrency Fixed)
@MainActor
class TailscaleWatcher: NSObject, ObservableObject, UNUserNotificationCenterDelegate {
    @Published var lastFile: String = "Waiting..."
    @Published var isConnected: Bool = false
    @Published var statusMessage: String = "Initializing..."
    @Published var shouldAutoDownload: Bool = false

    private var timer: Timer?
        private var audioPlayer: AVAudioPlayer?
            private var serverUrl: String = ""

                override init() {
                    super.init()
                    UNUserNotificationCenter.current().delegate = self
                }

                func startService(url: String) {
                    self.serverUrl = url
                    setupAudioSession()
                    playSilence()
                    startPolling()
                    requestPermissions()
                }

                // --- DELEGATE METHODS (Fixed for Swift 6) ---

                nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification, withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void) {
                    completionHandler([.banner, .sound, .badge])
                }

                nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse, withCompletionHandler completionHandler: @escaping () -> Void) {
                    // Jump back to MainActor to update UI state
                    Task { @MainActor in
                        self.shouldAutoDownload = true
                    }
                    completionHandler()
                }

                // --- POLLING ---

                private func startPolling() {
                    timer = Timer.scheduledTimer(withTimeInterval: 3.0, repeats: true) { [weak self] _ in
                        self?.checkServer()
                    }
                }

                private func checkServer() {
                    guard let url = URL(string: serverUrl) else { return }
                    let request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 5.0)

                    // Using Task ensures we stay in Swift concurrency world
                    Task {
                        do {
                            let (data, _) = try await URLSession.shared.data(for: request)

                            if let json = try JSONSerialization.jsonObject(with: data) as? [String: Any] {
                                var newFileName = "Waiting..."

                                // Logic to find the file name
                                if let fileObject = json["last_sent_file"] as? [String: Any],
                                    let name = fileObject["name"] as? String {
                                        newFileName = name
                                    } else if let name = json["last_received_file"] as? String {
                                        newFileName = name
                                    }

                                    self.isConnected = true

                                    if newFileName != self.lastFile && newFileName != "Waiting..." {
                                        if self.lastFile != "Waiting..." {
                                            self.sendNotification(fileName: newFileName)
                                        }
                                    }
                                    self.lastFile = newFileName
                                    if self.statusMessage != "Downloading..." { self.statusMessage = "Monitoring active" }
                            }
                        } catch {
                            self.isConnected = false
                            self.statusMessage = "Connection lost: \(error.localizedDescription)"
                        }
                    }
                }

                private func sendNotification(fileName: String) {
                    let content = UNMutableNotificationContent()
                    content.title = "File Ready"
                    content.body = "Tap to download: \(fileName)"
                    content.sound = .default
                    let request = UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil)
                    UNUserNotificationCenter.current().add(request)
                }

                // --- AUDIO HACK ---
                private func setupAudioSession() {
                    try? AVAudioSession.sharedInstance().setCategory(.playback, options: .mixWithOthers)
                    try? AVAudioSession.sharedInstance().setActive(true)
                }
                private func playSilence() {
                    let b64 = "UklGRigAAABXQVZFZm10IBIAAAABAAEARKwAAIhYAQACABAAAABkYXRhAgAAAAEA"
                    guard let data = Data(base64Encoded: b64) else { return }
                    do {
                        audioPlayer = try AVAudioPlayer(data: data)
                        audioPlayer?.numberOfLoops = -1; audioPlayer?.volume = 0.01; audioPlayer?.play()
                    } catch {}
                }
                private func requestPermissions() {
                    UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound, .badge]) { _, _ in }
                }
}

// MARK: - SYNC MANAGER (Logic for Tab 2)
struct RemoteFile: Codable, Identifiable {
    let id = UUID()
    let name: String
    let is_dir: Bool
    let size: Int64
    let modified: UInt64

    enum CodingKeys: String, CodingKey {
        case name, is_dir, size, modified
    }
}

@MainActor
class SyncManager: ObservableObject {
    @Published var outdatedFiles: [RemoteFile] = []
    @Published var isSyncing: Bool = false

    let serverBase = "https://manjaro-work.taile483f.ts.net"

    func checkForUpdates() {
        guard let url = URL(string: "\(serverBase)/browse") else { return }
        self.isSyncing = true

        Task {
            do {
                let (data, _) = try await URLSession.shared.data(from: url)
                let remoteFiles = try JSONDecoder().decode([RemoteFile].self, from: data)
                self.compareManifest(remoteFiles: remoteFiles)
                self.isSyncing = false
            } catch {
                print("Sync Error: \(error)")
                self.isSyncing = false
            }
        }
    }

    func uploadProject(folderUrl: URL) {
        // 1. Access Security Scoped Resource (Required for Folders)
        guard folderUrl.startAccessingSecurityScopedResource() else {
            print("❌ Permission denied to access folder")
            return
        }
        defer { folderUrl.stopAccessingSecurityScopedResource() }
        
        let fileManager = FileManager.default
        let folderName = folderUrl.lastPathComponent
        
        print("🚀 Broadcasting Project: \(folderName)")
        
        // 2. Enumerate Files recursively
        if let enumerator = fileManager.enumerator(at: folderUrl, includingPropertiesForKeys: [.isRegularFileKey], options: [.skipsHiddenFiles]) {
            
            for case let fileUrl as URL in enumerator {
                // Check if it's a file (not a dir)
                let resourceValues = try? fileUrl.resourceValues(forKeys: [.isRegularFileKey])
                if resourceValues?.isRegularFile == true {
                    
                    // 3. Calculate Relative Path (e.g., "MyGame/src/main.rs")
                    // We strip the base folder path to get the relative structure
                    let pathComponents = fileUrl.pathComponents
                    if let index = pathComponents.firstIndex(of: folderName) {
                        let relativePath = pathComponents[index...].joined(separator: "/")
                        
                        // 4. Upload File
                        uploadSingleFile(fileUrl: fileUrl, serverPath: relativePath)
                    }
                }
            }
        }
        
        // Refresh list after a short delay
        DispatchQueue.main.asyncAfter(deadline: .now() + 2) {
            self.checkForUpdates()
        }
    }
    
    private func uploadSingleFile(fileUrl: URL, serverPath: String) {
        guard let url = URL(string: "\(serverBase)/upload") else { return }
        
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        
        let boundary = UUID().uuidString
        request.setValue("multipart/form-data; boundary=\(boundary)", forHTTPHeaderField: "Content-Type")
        
        guard let fileData = try? Data(contentsOf: fileUrl) else { return }
        
        // Build Multipart Body
        var body = Data()
        body.append("--\(boundary)\r\n".data(using: .utf8)!)
        // Key: The filename Rust will use to save
        body.append("Content-Disposition: form-data; name=\"\(serverPath)\"; filename=\"\(serverPath)\"\r\n".data(using: .utf8)!)
        body.append("Content-Type: application/octet-stream\r\n\r\n".data(using: .utf8)!)
        body.append(fileData)
        body.append("\r\n".data(using: .utf8)!)
        body.append("--\(boundary)--\r\n".data(using: .utf8)!)
        
        request.httpBody = body
        
        URLSession.shared.dataTask(with: request) { _, response, error in
            if let error = error {
                print("❌ Upload failed for \(serverPath): \(error)")
                return
            }
            if let httpResponse = response as? HTTPURLResponse, httpResponse.statusCode == 200 {
                print("✅ Uploaded: \(serverPath)")
            }
        }.resume()
    }

    private func compareManifest(remoteFiles: [RemoteFile]) {
        var newOutdated: [RemoteFile] = []
        let fileManager = FileManager.default
        let docsUrl = fileManager.urls(for: .documentDirectory, in: .userDomainMask)[0]

        for remote in remoteFiles {
            if remote.is_dir { continue }

            let localUrl = docsUrl.appendingPathComponent(remote.name)

            if fileManager.fileExists(atPath: localUrl.path) {
                if let attrs = try? fileManager.attributesOfItem(atPath: localUrl.path),
                    let localDate = attrs[.modificationDate] as? Date {
                        let localTimestamp = UInt64(localDate.timeIntervalSince1970)
                        // If Remote is newer by > 2 seconds
                        if remote.modified > (localTimestamp + 2) {
                            newOutdated.append(remote)
                        }
                    }
            } else {
                newOutdated.append(remote)
            }
        }
        self.outdatedFiles = newOutdated
    }

    func pullFile(_ file: RemoteFile) {
        guard let url = URL(string: "\(serverBase)/download/\(file.name)") else { return }

        Task {
            do {
                let (localUrl, _) = try await URLSession.shared.download(from: url)
                let docsUrl = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
                let destUrl = docsUrl.appendingPathComponent(file.name)

                if FileManager.default.fileExists(atPath: destUrl.path) {
                    try FileManager.default.removeItem(at: destUrl)
                }
                try FileManager.default.moveItem(at: localUrl, to: destUrl)

                // Remove from list
                self.outdatedFiles.removeAll { $0.name == file.name }
            } catch {
                print("Download error: \(error)")
            }
        }
    }

    func pullAll() {
        for file in outdatedFiles {
            pullFile(file)
        }
    }
}

// MARK: - HELPERS
struct ShareSheet: UIViewControllerRepresentable {
    var activityItems: [Any]
    func makeUIViewController(context: Context) -> UIActivityViewController {
        UIActivityViewController(activityItems: activityItems, applicationActivities: nil)
    }
    func updateUIViewController(_ uiViewController: UIActivityViewController, context: Context) {}
}
