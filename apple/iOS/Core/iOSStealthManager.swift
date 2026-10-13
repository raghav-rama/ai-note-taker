import Foundation
import UIKit
import SwiftUI
import Combine

/// Manages screen capture prevention on iOS.
///
/// Two layers of protection:
/// 1. **Content hiding via secure text field trick**: the app's content is
///    rendered inside a canvas view that iOS excludes from screen capture.
/// 2. **Screen recording detection**: observes `UIScreen.capturedDidChangeNotification`
///    and `UIApplication.userDidTakeScreenshotNotification` to show a warning banner.
@MainActor
final class iOSStealthManager: ObservableObject {

    @Published private(set) var isScreenBeingCaptured = false
    @Published private(set) var screenshotTaken = false

    private var cancellables = Set<AnyCancellable>()

    init() {
        checkCaptureState()
        observeCapture()
        observeScreenshots()
    }

    // MARK: - Private

    private func checkCaptureState() {
        if let screen = currentScreen() {
            isScreenBeingCaptured = screen.isCaptured
        }
    }

    private func currentScreen() -> UIScreen? {
        UIApplication.shared.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .first?
            .screen
    }

    private func observeCapture() {
        NotificationCenter.default
            .publisher(for: UIScreen.capturedDidChangeNotification)
            .receive(on: RunLoop.main)
            .sink { [weak self] _ in
                guard let self else { return }
                if let screen = self.currentScreen() {
                    self.isScreenBeingCaptured = screen.isCaptured
                }
            }
            .store(in: &cancellables)
    }

    private func observeScreenshots() {
        NotificationCenter.default
            .publisher(for: UIApplication.userDidTakeScreenshotNotification)
            .receive(on: RunLoop.main)
            .sink { [weak self] _ in
                self?.screenshotTaken = true
                DispatchQueue.main.asyncAfter(deadline: .now() + 3) {
                    self?.screenshotTaken = false
                }
            }
            .store(in: &cancellables)
    }
}

// MARK: - Secure Content Container (UITextField trick)

/// Uses the `UITextField` `isSecureTextEntry` trick to get iOS's capture-proof
/// canvas layer. The secure text field creates a special `_UITextFieldCanvasView`
/// (obtained via `layer.sublayers?.first?.delegate`) that iOS excludes from
/// screen capture. We host all content inside that canvas view.
///
/// This relies on an undocumented internal layer structure. The guard clauses
/// ensure graceful degradation: if Apple changes the implementation the content
/// renders normally without capture protection.
final class SecureContainerView: UIView {

    private let secureField = UITextField()
    private var canvas: UIView?

    override init(frame: CGRect) {
        super.init(frame: frame)
        setupSecureCanvas()
    }

    required init?(coder: NSCoder) {
        super.init(coder: coder)
        setupSecureCanvas()
    }

    /// Install a child view into the secure canvas (or self as fallback).
    func install(_ contentView: UIView) {
        let host = canvas ?? self
        host.addSubview(contentView)
        contentView.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            contentView.topAnchor.constraint(equalTo: host.topAnchor),
            contentView.bottomAnchor.constraint(equalTo: host.bottomAnchor),
            contentView.leadingAnchor.constraint(equalTo: host.leadingAnchor),
            contentView.trailingAnchor.constraint(equalTo: host.trailingAnchor),
        ])
    }

    private func setupSecureCanvas() {
        secureField.isSecureTextEntry = true
        secureField.isUserInteractionEnabled = false

        // Try to extract the capture-proof canvas view from the text field's layer.
        // The delegate of the first sublayer is the internal canvas UIView.
        if let canvasView = secureField.layer.sublayers?.first?.delegate as? UIView {
            canvasView.subviews.forEach { $0.removeFromSuperview() }
            canvasView.isUserInteractionEnabled = true
            addSubview(canvasView)
            canvasView.translatesAutoresizingMaskIntoConstraints = false
            NSLayoutConstraint.activate([
                canvasView.topAnchor.constraint(equalTo: topAnchor),
                canvasView.bottomAnchor.constraint(equalTo: bottomAnchor),
                canvasView.leadingAnchor.constraint(equalTo: leadingAnchor),
                canvasView.trailingAnchor.constraint(equalTo: trailingAnchor),
            ])
            canvas = canvasView
        }
        // If canvas is nil, install() falls back to adding directly to self.
    }
}

// MARK: - SwiftUI Integration via UIViewControllerRepresentable

/// A view controller that hosts SwiftUI content inside the capture-proof
/// SecureContainerView. Used as the root of the window to protect the
/// entire app's content from screen recording.
final class SecureHostingController<Content: View>: UIViewController {

    let hostingController: UIHostingController<Content>

    init(rootView: Content) {
        self.hostingController = UIHostingController(rootView: rootView)
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("Not implemented")
    }

    override func loadView() {
        view = SecureContainerView()
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        hostingController.view.backgroundColor = .clear
        addChild(hostingController)
        (view as! SecureContainerView).install(hostingController.view)
        hostingController.didMove(toParent: self)
    }
}

/// SwiftUI bridge that wraps content in the capture-proof secure container.
/// Environment objects must be applied to the content BEFORE passing it here,
/// because the content will be hosted inside a separate UIHostingController.
struct SecureRootView<Content: View>: UIViewControllerRepresentable {
    let content: Content

    init(@ViewBuilder content: () -> Content) {
        self.content = content()
    }

    func makeUIViewController(context: Context) -> SecureHostingController<Content> {
        SecureHostingController(rootView: content)
    }

    func updateUIViewController(_ uiViewController: SecureHostingController<Content>, context: Context) {
        uiViewController.hostingController.rootView = content
    }
}
