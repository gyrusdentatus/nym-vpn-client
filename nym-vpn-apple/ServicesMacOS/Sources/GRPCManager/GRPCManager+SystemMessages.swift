import GRPC
import SwiftProtobuf
import SystemMessageModels

extension GRPCManager {
    public func fetchSystemMessages() async throws -> [NymNetworkMessage] {
        logger.log(level: .info, "Checking if stored account")

        return try await withCheckedThrowingContinuation { continuation in
            let call = client.getSystemMessages(Google_Protobuf_Empty())

            call.response.whenComplete { result in
                switch result {
                case let .success(response):
                    let messages = response.messages.map {
                        NymNetworkMessage(name: $0.name, message: $0.message, properties: $0.properties)
                    }
                    continuation.resume(returning: messages)
                case let .failure(error):
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    public func fetchCompatibleVersions() async throws -> (macOS: String?, core: String?) {
        try await withCheckedThrowingContinuation { continuation in
            let call = client.getNetworkCompatibility(Google_Protobuf_Empty())
            call.response.whenComplete { result in
                switch result {
                case let .success(success):
                    continuation.resume(returning: (macOS: success.messages.macos, core: success.messages.core))
                case let .failure(error):
                    continuation.resume(throwing: error)
                }
            }
        }
    }
}
