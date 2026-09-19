import AppKit
import SwiftUI
import UniformTypeIdentifiers
import WebKit

@main
struct ResoptApp: App {
    @StateObject private var model = ReviewSession()

    var body: some Scene {
        Window("resopt", id: "main") {
            ContentView(model: model)
                .frame(minWidth: 900, minHeight: 640)
                .onDisappear { model.stop() }
        }
        .windowStyle(.titleBar)
    }
}

private struct ContentView: View {
    @ObservedObject var model: ReviewSession
    @State private var choosingProject = false
    @State private var choosingReport = false
    @State private var jobs = ProcessInfo.processInfo.activeProcessorCount >= 4 ? 4 : 2

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 12) {
                Image(systemName: "photo.stack")
                    .foregroundStyle(.tint)
                Text("resopt")
                    .font(.headline)
                Button(model.project == nil ? "选择项目目录" : "更换项目目录") {
                    choosingProject = true
                }
                .disabled(model.isStarting)
                Button("打开已有报告") { choosingReport = true }
                    .disabled(model.isStarting)
                Picker("并行任务", selection: $jobs) {
                    Text("2").tag(2)
                    Text("4").tag(4)
                    Text("8").tag(8)
                }
                .frame(width: 110)
                .help("同时分析的不同图片数量。任务数越多，占用的内存也越多；下次扫描时生效。")
                if let project = model.project {
                    Text(project.path)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .foregroundStyle(.secondary)
                        .help(project.path)
                }
                Spacer(minLength: 8)
                if let report = model.reportDirectory {
                    Button("显示报告目录") {
                        NSWorkspace.shared.activateFileViewerSelecting([report])
                    }
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 10)

            Divider()

            if let url = model.pageURL {
                ReviewWebView(url: url)
            } else {
                VStack(spacing: 14) {
                    Image(systemName: "folder.badge.magnifyingglass")
                        .font(.system(size: 46))
                        .foregroundStyle(.secondary)
                    Text("选择一个项目开始分析")
                        .font(.title2)
                    Text("扫描和图片处理都在这台 Mac 上完成。应用候选前会显示需要修改的文件。")
                        .foregroundStyle(.secondary)
                    Button("选择项目目录") { choosingProject = true }
                        .disabled(model.isStarting)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }

            Divider()
            HStack {
                if model.isStarting { ProgressView().controlSize(.small) }
                Text(model.status)
                    .lineLimit(2)
                    .textSelection(.enabled)
                Spacer()
            }
            .font(.caption)
            .foregroundStyle(model.hasError ? .red : .secondary)
            .padding(.horizontal, 16)
            .padding(.vertical, 7)
        }
        .fileImporter(
            isPresented: $choosingProject,
            allowedContentTypes: [.directory]
        ) { result in
            switch result {
            case .success(let url): model.start(project: url, jobs: jobs)
            case .failure(let error): model.fail("无法打开项目目录：\(error.localizedDescription)")
            }
        }
        .fileImporter(
            isPresented: $choosingReport,
            allowedContentTypes: [.directory]
        ) { result in
            switch result {
            case .success(let url): model.open(report: url)
            case .failure(let error): model.fail("无法打开报告目录：\(error.localizedDescription)")
            }
        }
    }
}

private struct ReviewWebView: NSViewRepresentable {
    let url: URL

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> WKWebView {
        let view = WKWebView()
        view.uiDelegate = context.coordinator
        view.load(URLRequest(url: url))
        return view
    }

    func updateNSView(_ view: WKWebView, context: Context) {
        if view.url != url {
            view.load(URLRequest(url: url))
        }
    }

    final class Coordinator: NSObject, WKUIDelegate {
        func webView(
            _ webView: WKWebView,
            createWebViewWith configuration: WKWebViewConfiguration,
            for navigationAction: WKNavigationAction,
            windowFeatures: WKWindowFeatures
        ) -> WKWebView? {
            if navigationAction.targetFrame == nil,
               let url = navigationAction.request.url {
                NSWorkspace.shared.open(url)
            }
            return nil
        }
    }
}

@MainActor
private final class ReviewSession: ObservableObject {
    @Published private(set) var project: URL?
    @Published private(set) var reportDirectory: URL?
    @Published private(set) var pageURL: URL?
    @Published private(set) var status = "就绪。请选择项目目录。"
    @Published private(set) var hasError = false
    @Published private(set) var isStarting = false

    private var process: Process?
    private var stdoutPipe: Pipe?
    private var stderrPipe: Pipe?
    private var stdoutBuffer = Data()
    private var stderrText = ""
    private var scopedProject: URL?
    private var scopedReport: URL?
    private var generation = 0

    func start(project url: URL, jobs: Int) {
        do {
            let reports = try FileManager.default.url(
                for: .applicationSupportDirectory,
                in: .userDomainMask,
                appropriateFor: nil,
                create: true
            ).appendingPathComponent("resopt/Reports", isDirectory: true)
            try FileManager.default.createDirectory(
                at: reports,
                withIntermediateDirectories: true
            )
            let output = reports.appendingPathComponent(UUID().uuidString, isDirectory: true)
            launch(
                arguments: ["web", url.path, "--out", output.path, "--no-open", "--jobs", String(jobs)],
                project: url,
                report: output,
                reportNeedsAccess: false
            )
        } catch {
            fail("启动失败：\(error.localizedDescription)")
        }
    }

    func open(report url: URL) {
        launch(
            arguments: ["serve", url.path],
            project: nil,
            report: url,
            reportNeedsAccess: true
        )
    }

    private func launch(
        arguments: [String],
        project: URL?,
        report: URL,
        reportNeedsAccess: Bool
    ) {
        stop()
        generation += 1
        let currentGeneration = generation
        self.project = project
        pageURL = nil
        reportDirectory = report
        hasError = false
        isStarting = true
        status = project == nil ? "正在打开已有报告…" : "正在启动本地扫描…"
        stdoutBuffer.removeAll()
        stderrText = ""

        if let project, project.startAccessingSecurityScopedResource() {
            scopedProject = project
        }
        if reportNeedsAccess, report.startAccessingSecurityScopedResource() {
            scopedReport = report
        }

        do {
            guard let binary = Bundle.main.url(forResource: "resopt", withExtension: nil),
                  FileManager.default.isExecutableFile(atPath: binary.path) else {
                throw SessionError("App 中缺少 resopt 命令行引擎")
            }
            let child = Process()
            child.executableURL = binary
            child.arguments = arguments
            let stdout = Pipe()
            let stderr = Pipe()
            child.standardOutput = stdout
            child.standardError = stderr
            stdout.fileHandleForReading.readabilityHandler = { [weak self] handle in
                let data = handle.availableData
                if data.isEmpty { handle.readabilityHandler = nil; return }
                Task { @MainActor [weak self] in
                    self?.receiveStdout(data, generation: currentGeneration)
                }
            }
            stderr.fileHandleForReading.readabilityHandler = { [weak self] handle in
                let data = handle.availableData
                if data.isEmpty { handle.readabilityHandler = nil; return }
                Task { @MainActor [weak self] in
                    self?.receiveStderr(data, generation: currentGeneration)
                }
            }
            child.terminationHandler = { [weak self] terminated in
                let code = terminated.terminationStatus
                Task { @MainActor [weak self] in
                    self?.finished(code: code, generation: currentGeneration)
                }
            }
            process = child
            stdoutPipe = stdout
            stderrPipe = stderr
            try child.run()
        } catch {
            stop()
            fail("启动失败：\(error.localizedDescription)")
        }
    }

    func stop() {
        generation += 1
        stdoutPipe?.fileHandleForReading.readabilityHandler = nil
        stderrPipe?.fileHandleForReading.readabilityHandler = nil
        if let process, process.isRunning { process.terminate() }
        process = nil
        stdoutPipe = nil
        stderrPipe = nil
        if let scopedProject {
            scopedProject.stopAccessingSecurityScopedResource()
            self.scopedProject = nil
        }
        if let scopedReport {
            scopedReport.stopAccessingSecurityScopedResource()
            self.scopedReport = nil
        }
        isStarting = false
    }

    func fail(_ message: String) {
        hasError = true
        isStarting = false
        status = message
    }

    private func receiveStdout(_ data: Data, generation: Int) {
        guard generation == self.generation, !data.isEmpty else { return }
        stdoutBuffer.append(data)
        while let newline = stdoutBuffer.firstIndex(of: 10) {
            let line = String(decoding: stdoutBuffer[..<newline], as: UTF8.self)
            stdoutBuffer.removeSubrange(...newline)
            let prefix = line.hasPrefix("Local web: ") ? "Local web: " : "Review server: "
            if line.hasPrefix(prefix),
               let url = URL(string: String(line.dropFirst(prefix.count))),
               url.scheme == "http", url.host == "127.0.0.1" {
                pageURL = url
                isStarting = false
                status = project == nil
                    ? "已打开报告，可以继续审核或恢复之前的操作。"
                    : "正在分析项目；结果会在页面中逐步显示。"
            }
        }
        if stdoutBuffer.count > 16_384 { stdoutBuffer.removeAll() }
    }

    private func receiveStderr(_ data: Data, generation: Int) {
        guard generation == self.generation, !data.isEmpty else { return }
        stderrText += String(decoding: data, as: UTF8.self)
        if stderrText.count > 4_096 { stderrText = String(stderrText.suffix(4_096)) }
    }

    private func finished(code: Int32, generation: Int) {
        guard generation == self.generation else { return }
        process = nil
        isStarting = false
        if code == 0 {
            status = "本地服务已停止。报告保留在 App 的报告目录中。"
        } else {
            fail("本地服务已退出（代码 \(code)）。\(stderrText)")
        }
    }
}

private struct SessionError: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}
