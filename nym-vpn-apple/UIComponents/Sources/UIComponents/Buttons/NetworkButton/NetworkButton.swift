import SwiftUI
import Theme

public struct NetworkButton: View {
    @StateObject private var viewModel: NetworkButtonViewModel
    @State private var isHovered = false

    public init(viewModel: NetworkButtonViewModel) {
        self._viewModel = StateObject(wrappedValue: viewModel)
    }

    public var body: some View {
        VStack {
            HStack {
                Image(viewModel.imageName, bundle: .module)
                    .foregroundStyle(viewModel.selectionImageColor)
                    .padding(.leading, 16)

                VStack(alignment: .leading) {
                    Text(viewModel.title)
                        .foregroundStyle(NymColor.sysOnSurface)
                        .textStyle(.BodyLegacy.Large.semibold)
                    Text(viewModel.subtitle)
                        .foregroundStyle(NymColor.sysOutline)
                        .textStyle(viewModel.isSmallScreen ? .BodyLegacy.Small.primary : .BodyLegacy.Medium.regular)
                }
                .padding(.leading, 8)
                Spacer()
            }
        }
        .frame(height: viewModel.isSmallScreen ? 56 : 64)
        .background(NymColor.navigationBarBackground.opacity(isHovered ? 0.7 : 1))
        .cornerRadius(8)
        .overlay(
            RoundedRectangle(cornerRadius: 8)
                .inset(by: 0.5)
                .stroke(viewModel.selectionStrokeColor)
        )
        .onHover { newValue in
            isHovered = newValue
        }
        .animation(.default, value: viewModel.selectionStrokeColor)
    }
}
