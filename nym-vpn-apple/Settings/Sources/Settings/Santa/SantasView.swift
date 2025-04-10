import SwiftUI
import AppSettings
import Constants
import Theme
import UIComponents

public struct SantasView: View {
    @ObservedObject private var viewModel: SantasViewModel

    public init(viewModel: SantasViewModel) {
        self.viewModel = viewModel
    }

    public var body: some View {
        VStack(spacing: .zero) {
            navbar()
            ScrollView {
                santasSpacer()
                VStack {
                    enivironmentDetails()
                    santasSpacer()
                    environmentSection()
                    featureFlagsSection()
                }
                Spacer()
            }
        }
        .preferredColorScheme(AppSettings.shared.currentAppearance.colorScheme)
        .navigationBarBackButtonHidden(true)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background {
            NymColor.background
                .ignoresSafeArea()
        }
    }
}

private extension SantasView {
    func navbar() -> some View {
        CustomNavBar(
            title: viewModel.title,
            leftButton: CustomNavBarButton(type: .back, action: { viewModel.navigateBack() })
        )
        .padding(0)
    }

    func enivironmentDetails() -> some View {
        VStack {
            Text("Environment Details:")
                .foregroundStyle(NymColor.accent)
                .bold()
                .padding(4)
            Text("App environment: \(viewModel.currentAppEnv)")
                .padding(4)
            Text("Daemon/lib environment: \(viewModel.actualEnv)")
                .padding(4)
            Text("Daemon/lib version: \(viewModel.libVersion)")
                .padding(4)
#if os(macOS)
            Button("Refetch daemon info") {
                viewModel.updateDaemonInfo()
            }
#endif
        }
        .padding(16)
    }

    func environmentSection() -> some View {
        VStack {
            Text("Environment:")
                .foregroundStyle(NymColor.accent)
                .bold()
                .padding(4)
#if os(macOS)
            Text("⚠️ Please restart daemon after switching the env ⚠️")
                .padding(4)
#endif
            HStack {
                ForEach(viewModel.envs, id: \.self) { env in
                    Button(env.rawValue) {
                        viewModel.changeEnvironment(to: env)
                    }
                }
            }
        }
    }

    @ViewBuilder
    func featureFlagsSection() -> some View {
        VStack {
            Text("Feature flags:")
                .foregroundStyle(NymColor.accent)
                .bold()
                .padding(4)

            HStack {
                Toggle("zknym", isOn: $viewModel.isZknymEnabled)
            }
        }
    }
}

private extension SantasView {
    func santasSpacer() -> some View {
        Spacer()
            .frame(height: 16)
    }
}
