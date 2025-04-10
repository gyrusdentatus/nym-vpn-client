import Combine
import Foundation
import NetworkExtension
import AppSettings
import CountriesManager
import ConnectionTypes
import CredentialsManager
import TunnelMixnet
import Tunnels
import TunnelStatus
#if os(macOS)
import GRPCManager
#endif

public final class ConnectionManager: ObservableObject {
    private let connectionStorage: ConnectionStorage
    private let countriesManager: CountriesManager

    private var timerCancellable: AnyCancellable?

    let appSettings: AppSettings
    let credentialsManager: CredentialsManager
    let tunnelsManager: TunnelsManager
#if os(macOS)
    let grpcManager: GRPCManager
#endif

    var cancellables = Set<AnyCancellable>()
    var tunnelStatusUpdateCancellable: AnyCancellable?

    // TODO: remove this once iOS tunnel supports tunnel reconnection
    public var isReconnecting = false
    public var isDisconnecting = false

    public static let shared = ConnectionManager()

    @Published public var connectedDate: Date?
    @Published public var connectedDateString: String?
    @Published public var lastError: Error?

    @Published public var connectionType: ConnectionType {
        didSet {
            appSettings.connectionType = connectionType.rawValue
            Task { @MainActor in
                await reconnectIfNeeded()
            }
        }
    }
    @Published public var isTunnelManagerLoaded: Result<Void, Error>?
#if os(iOS)
    @Published public var activeTunnel: Tunnel? {
        didSet {
            guard let activeTunnel else { return }
            configureTunnelStatusObserver(tunnel: activeTunnel)
        }
    }

    // TODO: remove this once iOS tunnel supports tunnel reconnection
    @Published public var currentTunnelStatus: TunnelStatus = .disconnected {
        didSet {
            updateTunnelStatusIfReconnecting()
//            updateTunnelStatusIfDisconnecting() 
        }
    }
#elseif os(macOS)
    @Published public var currentTunnelStatus: TunnelStatus = .disconnected
#endif
    @Published public var entryGateway: EntryGateway {
        didSet {
            Task { @MainActor in
                connectionStorage.entryGateway = entryGateway
                await reconnectIfNeeded()
            }
        }
    }
    @Published public var exitRouter: ExitRouter {
        didSet {
            Task { @MainActor in
                connectionStorage.exitRouter = exitRouter
                await reconnectIfNeeded()
            }
        }
    }

#if os(iOS)
    public init(
        appSettings: AppSettings = AppSettings.shared,
        connectionStorage: ConnectionStorage = ConnectionStorage.shared,
        countriesManager: CountriesManager = CountriesManager.shared,
        credentialsManager: CredentialsManager = CredentialsManager.shared,
        tunnelsManager: TunnelsManager = TunnelsManager.shared
    ) {
        self.appSettings = appSettings
        self.connectionStorage = connectionStorage
        self.countriesManager = countriesManager
        self.credentialsManager = credentialsManager
        self.tunnelsManager = tunnelsManager
        self.entryGateway = connectionStorage.entryGateway
        self.exitRouter = connectionStorage.exitRouter
        self.connectionType = connectionStorage.connectionType
        setup()
    }
#endif

#if os(macOS)
    public init(
        appSettings: AppSettings = AppSettings.shared,
        connectionStorage: ConnectionStorage = ConnectionStorage.shared,
        countriesManager: CountriesManager = CountriesManager.shared,
        credentialsManager: CredentialsManager = CredentialsManager.shared,
        tunnelsManager: TunnelsManager = TunnelsManager.shared,
        grpcManager: GRPCManager = GRPCManager.shared
    ) {
        self.appSettings = appSettings
        self.connectionStorage = connectionStorage
        self.countriesManager = countriesManager
        self.credentialsManager = credentialsManager
        self.tunnelsManager = tunnelsManager
        self.grpcManager = grpcManager
        self.entryGateway = connectionStorage.entryGateway
        self.exitRouter = connectionStorage.exitRouter
        self.connectionType = connectionStorage.connectionType
        setup()
    }
#endif

    /// Disconnects tunnel if connected.
    /// iOS removes tunnel profile.
    public func disconnectBeforeLogout() async {
        guard currentTunnelStatus != .disconnected else { return }
#if os(iOS)
        disconnectActiveTunnel()
        await waitForTunnelStatus(with: .disconnected)
        resetVpnProfile()
#elseif os(macOS)
        grpcManager.disconnect()
        await waitForTunnelStatus(with: .disconnected)
#endif
    }
}

// MARK: - Setup -
private extension ConnectionManager {
    func setup() {
#if os(iOS)
        setupTunnelManagerObservers()
#elseif os(macOS)
        setupGRPCManagerObservers()
#endif
        setupCountriesManagerObserver()
        setupConnectionChangeObserver()
        setupConnectionErrorObserver()

        configureConnectedTimeTimer()
    }
}

// MARK: - Reset VPN profile -
public extension ConnectionManager {
    func resetVpnProfile() {
        tunnelsManager.resetVpnProfile()
    }
}

// MARK: - Connection -

private extension ConnectionManager {
    func waitForTunnelStatus(with targetStatus: TunnelStatus) async {
        await withCheckedContinuation { continuation in
            var cancellable: AnyCancellable?

            cancellable = $currentTunnelStatus
                .sink { status in
                    guard cancellable != nil,
                          status == targetStatus
                    else {
                        return
                    }
                    continuation.resume()
                    cancellable?.cancel()
                    cancellable = nil
                }
        }
    }
}
// MARK: - Countries -

private extension ConnectionManager {
    func setupCountriesManagerObserver() {
        countriesManager.$entryCountries.sink { [weak self] _ in
            self?.updateCountries()
        }
        .store(in: &cancellables)

        countriesManager.$exitCountries.sink { [weak self] _ in
            self?.updateCountries()
        }
        .store(in: &cancellables)

        countriesManager.$vpnCountries.sink { [weak self] _ in
            self?.updateCountries()
        }
        .store(in: &cancellables)
    }

    func setupConnectionChangeObserver() {
        $connectionType.sink { [weak self] _ in
            self?.updateCountries()
        }
        .store(in: &cancellables)
    }

    func setupConnectionErrorObserver() {
#if os(iOS)
        tunnelsManager.$lastError
            .receive(on: DispatchQueue.main)
            .sink { [weak self] newError in
                self?.lastError = newError
            }
            .store(in: &cancellables)
#elseif os(macOS)
        grpcManager.$errorReason
            .receive(on: DispatchQueue.main)
            .sink { [weak self] newError in
                self?.lastError = newError
            }
            .store(in: &cancellables)
#endif
    }

    func updateCountries() {
        Task { @MainActor in
            updateConnectionHops()
        }
    }

    func updateConnectionHops() {
        entryGateway = connectionStorage.entryGateway
        exitRouter = connectionStorage.exitRouter
    }
}

// MARK: - Connection time -
private extension ConnectionManager {
    func configureConnectedTimeTimer() {
        timerCancellable = Timer.publish(every: 1.0, on: .main, in: .common)
            .autoconnect()
            .receive(on: DispatchQueue.main)
            .sink { [weak self] _ in
                guard let self = self else { return }
                updateConnectedDateString()
            }
    }

    func updateConnectedDateString() {
        guard let connectedDate
        else {
            guard connectedDateString != nil else { return }
            connectedDateString = nil
            return
        }
        let timeElapsed = Date().timeIntervalSince(connectedDate)
        let hours = Int(timeElapsed) / 3600
        let minutes = (Int(timeElapsed) % 3600) / 60
        let seconds = Int(timeElapsed) % 60
        let newConnectedDateString = "\(String(format: "%02d:%02d:%02d", hours, minutes, seconds))"
        guard connectedDateString != newConnectedDateString else { return }
        connectedDateString = newConnectedDateString
    }
}
