import Foundation

/// The Rust core (SSH, SFTP, sync): JSON commands in, JSON results and events out.
enum Core {
    struct Failure: LocalizedError {
        let message: String
        var errorDescription: String? { message }
    }

    /// (kind, id, payload) on the main thread. "data" carries raw terminal bytes, the rest JSON.
    nonisolated(unsafe) static var onEvent: (String, String, Data) -> Void = { _, _, _ in }

    static func start(device: String) {
        termix_init(device) { kind, id, data, len in
            let k = String(cString: kind!), i = String(cString: id!)
            let d = len > 0 ? Data(bytes: data!, count: len) : Data()
            DispatchQueue.main.async { Core.onEvent(k, i, d) }
        }
    }

    private final class Pending {
        let cont: CheckedContinuation<Data, Error>
        init(_ c: CheckedContinuation<Data, Error>) { cont = c }
    }

    @discardableResult
    static func call(_ cmd: String, _ args: [String: Any] = [:]) async throws -> Data {
        let json = String(decoding: try JSONSerialization.data(withJSONObject: args), as: UTF8.self)
        return try await withCheckedThrowingContinuation { cont in
            let ctx = Unmanaged.passRetained(Pending(cont)).toOpaque()
            termix_call(cmd, json, ctx) { ctx, ok, out in
                let p = Unmanaged<Pending>.fromOpaque(ctx!).takeRetainedValue()
                let s = String(cString: out!)
                if ok { p.cont.resume(returning: Data(s.utf8)) } else { p.cont.resume(throwing: Failure(message: s)) }
            }
        }
    }

    static func call<T: Decodable>(_ cmd: String, _ args: [String: Any] = [:], as: T.Type) async throws -> T {
        try decoder.decode(T.self, from: try await call(cmd, args))
    }

    static func write(_ session: String, _ bytes: ArraySlice<UInt8>) {
        bytes.withUnsafeBufferPointer { termix_write(session, $0.baseAddress, $0.count) }
    }

    static let decoder: JSONDecoder = {
        let d = JSONDecoder()
        d.keyDecodingStrategy = .convertFromSnakeCase
        return d
    }()
    static let encoder: JSONEncoder = {
        let e = JSONEncoder()
        e.keyEncodingStrategy = .convertToSnakeCase
        return e
    }()

    /// A Codable value as the `[String: Any]` the core's JSON args want.
    static func object<T: Encodable>(_ v: T) -> Any {
        (try? JSONSerialization.jsonObject(with: encoder.encode(v))) ?? NSNull()
    }
}
