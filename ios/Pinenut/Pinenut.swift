//
//  Pinenut.swift
//  Pinenut
//
//  Created by Tangent on 2023/11/19.
//

import Foundation
import PinenutFFI

/// Represents the domain to which the logger belongs, the logs will be organized by
/// domain.
public struct Domain {
    /// Used to identity a specific domain for logger.
    public var identifier: String

    /// Used to specify the directory where the log files for this domian are stored.
    public var directory: String

    /// Constructs a new `Domain`.
    public init(identifier: String, directory: String) {
        self.identifier = identifier
        self.directory = directory
    }

    /// Obtains a logger with a specified configuration.
    @inlinable
    public func logger(config: Config = .init()) -> Logger {
        .init(domain: self, config: config)
    }
}

/// Represents the dimension of datetime, used for log rotation.
public enum TimeDimension: UInt8 {
    case day = 1
    case hour
    case minute
}

/// Configuration of a logger instance.
public struct Config {
    /// Whether or not to use `mmap` as the underlying storage for the buffer.
    ///
    /// With mmap, if the application terminates unexpectedly, the log data in the
    /// buffer is written to the mmap buffer file by the OS at a certain time, and
    /// then when the logger is restarted, the log data is written back to the log
    /// file, avoiding loss of log data.
    ///
    /// It is enabled by default.
    public var useMmap: Bool = true

    /// The buffer length.
    ///
    /// If mmap is used, it is rounded up to a multiple of pagesize.
    /// Pinenut uses a double cache system, so the buffer that is actually written
    /// to will be less than half of this.
    ///
    /// The default value is `320 KB`.
    public var bufferLength: UInt64 = 320 * 1024

    /// Time granularity of log extraction.
    ///
    /// The default value is `.minute`.
    public var rotation: TimeDimension = .minute

    /// The encryption key, the public key in ECDH, represented in `Base64`.
    ///
    /// It is used to negotiate the key for symmetric encryption of the log.
    ///
    /// A public key is a compressed elliptic curve point, with length of 33 bytes:
    /// `1 byte (encoding tag) + 32 bytes (256 bits)`.
    ///
    /// If the value is `nil` or invalid, there is no encryption.
    ///
    /// The default value is `nil`.
    public var key: String?

    /// The compression level.
    ///
    /// Pinenut uses `zstd` as the compression algorithm, which supports compression
    /// levels from 1 up to 22, it also offers negative compression levels.
    ///
    /// As the `std`'s documentation says: The lower the level, the faster the
    /// speed (at the cost of compression).
    ///
    /// The default value is `10`.
    public var compressionLevel: Int32 = 10

    /// Constructs a new `Config`.
    @inlinable
    public init(
        useMmap: Bool = true,
        bufferLength: UInt64 = 320 * 1024,
        rotation: TimeDimension = .minute,
        key: String? = nil,
        compressionLevel: Int32 = 10
    ) {
        self.useMmap = useMmap
        self.bufferLength = bufferLength
        self.rotation = rotation
        self.key = key
        self.compressionLevel = compressionLevel
    }
}

/// Represents logging levels of a `Pinenut` log.
public enum Level: UInt8 {
    /// The `error` log level.
    ///
    /// It is typically the highest level of severity and is used when an operation
    /// fails.
    case error = 1

    /// The `warning` log level.
    ///
    /// It is used when something unexpected happened, or there might be a problem in
    /// the near future.
    case warn

    /// The `informational` log level.
    ///
    /// Infomational messages to track the general flow of the application.
    case info

    /// The `debug` log level.
    ///
    /// Logs that contain information useful for debugging during development and
    /// troubleshooting.
    case debug

    /// The `verbose` log level.
    ///
    /// Logs may include more information than the `Debug` level and are usually not
    /// enabled in a production environment.
    case verbose
}

/// Represents a location in the code where a `Pinenut` log was generated.
public struct Location {
    /// The code file where the log was generated. `nil` if not available.
    public var file: String?

    /// The function where the log was generated. `nil` if not available.
    public var function: String?

    /// The code line in the file where the log was generated. `nil` if not available.
    public var line: UInt32?

    /// Constructs a new `Location`.
    @inlinable
    public init(file: String? = nil, function: String? = nil, line: UInt32? = nil) {
        self.file = file
        self.function = function
        self.line = line
    }
}

/// Represents metadata associated with a `Pinenut` log.
public struct Meta {
    /// The level of the log.
    public var level: Level

    /// The datetime when the log was generated.
    public var date: Date

    /// The location in the code where the log was generated.
    public var location: Location

    /// An optional tag associated with the log.
    public var tag: String?

    /// The identifier of the thread where the log was generated.
    public var threadId: UInt64?

    /// Constructs a new `Meta`.
    @inlinable
    public init(
        level: Level,
        date: Date,
        location: Location,
        tag: String? = nil,
        threadId: UInt64? = nil
    ) {
        self.level = level
        self.date = date
        self.location = location
        self.tag = tag
        self.threadId = threadId
    }
}

/// Represents a `Pinenut` log record.
public struct Record {
    /// The metadata associated with the log.
    public var meta: Meta

    /// The content of the log.
    public var content: String

    /// Constructs a new `Record`.
    @inlinable
    public init(meta: Meta, content: String) {
        self.meta = meta
        self.content = content
    }
}

/// The `Pinenut` logger.
public final class Logger {
    @usableFromInline
    let pointer: UnsafeMutableRawPointer

    /// Constructs a new `Logger`.
    @inlinable
    public init(domain: Domain, config: Config = .init()) {
        pointer = try! call { state in
            domain.ffiDomain { domain in
                config.ffiConfig { config in
                    pinenut_logger_new(domain, config, state)
                }
            }
        }
    }

    /// Logs the record.
    ///
    /// The low-level IO operations are performed asynchronously.
    @inlinable
    public func log(_ record: Record) {
        try! call { state in
            record.ffiRecord { record in
                pinenut_logger_log(pointer, record, state)
            }
        }
    }

    /// Flushes any buffered records asynchronously.
    ///
    /// The low-level IO operations are performed asynchronously.
    @inlinable
    public func flush() {
        try! call {
            pinenut_logger_flush(pointer, $0)
        }
    }

    /// Deletes the expired log files with lifetime (seconds).
    ///
    /// The low-level IO operations are performed asynchronously.
    @inlinable
    public func trim(lifetime: UInt64) {
        try! call {
            pinenut_logger_trim(pointer, lifetime, $0)
        }
    }

    /// Extracts the logs for the specified time range and writes them to the destination
    /// file.
    ///
    /// Errors may be occurred during log writing, and the destination file may have been
    /// created by then. The caller is responsible for managing the destination file
    /// (e.g., deleting it) afterwards.
    @inlinable
    public static func extract(
        domain: Domain, timeRange: ClosedRange<Date>, destPath: String
    ) throws {
        let timeRange = Int64(timeRange.lowerBound.timeIntervalSince1970)
            ... Int64(timeRange.upperBound.timeIntervalSince1970)
        try call { state in
            domain.ffiDomain { domain in
                destPath.ffiBytes { destPath in
                    let (startTime, endTime) = (timeRange.lowerBound, timeRange.upperBound)
                    pinenut_extract(domain, startTime, endTime, destPath, state)
                }
            }
        }
    }

    /// Parses the compressed and encrypted binary log file into readable text file.
    ///
    /// Errors may be occurred during log writing, and the destination file may have been
    /// created by then. The caller is responsible for managing the destination file
    /// (e.g., deleting it) afterwards
    public static func parse(path: String, to destPath: String, secretKey: String?) throws {
        let secretKey = secretKey.flatMap { Data(base64Encoded: $0) }
        try call { state in
            path.ffiBytes { path in
                destPath.ffiBytes { destPath in
                    secretKey.ffiBytes { secretKey in
                        pinenut_parse_to_file(path, destPath, secretKey, state)
                    }
                }
            }
        }
    }

    deinit {
        try! call {
            pinenut_logger_shutdown(pointer, $0)
        }
    }
}

/// The error type for `Pinenut`.
public struct Error: Swift.Error, CustomStringConvertible {
    /// Whether a Rust panic was generated.
    public let isPanic: Bool
    /// The error description.
    public let description: String
}

// MARK: - Internal

/// Calls FFI.
@usableFromInline
func call<T>(_ body: (UnsafeMutablePointer<FFICallState>) -> T?) throws -> T {
    var state = pinenut_call_state_success()
    let value = body(&state)

    if let error = Error(state: state) {
        throw error
    }
    if let value = value {
        return value
    }

    throw Error(isPanic: false, description: "unexpected nil result from FFI call")
}

extension Error {
    @inlinable
    init?(state: FFICallState) {
        guard state.code != FFICallSucces else {
            return nil
        }
        isPanic = state.code == FFICallPanic
        // Copies the error description then dealloc the original memory.
        description = .init(ffiBytes: state.err_desc) ?? "unknown"
        try! call {
            pinenut_dealloc_bytes(state.err_desc, $0)
        }
    }
}

extension Domain {
    @inlinable
    func ffiDomain<T>(_ domain: (FFIDomain) -> T) -> T {
        identifier.ffiBytes { identifier in
            directory.ffiBytes { directory in
                domain(.init(identifier: identifier, directory: directory))
            }
        }
    }
}

extension Config {
    @usableFromInline
    func ffiConfig<T>(_ config: (FFIConfig) -> T) -> T {
        key.ffiBytes { key in
            config(
                .init(
                    use_mmap: useMmap,
                    buffer_len: bufferLength,
                    rotation: rotation.rawValue,
                    key_str: key,
                    compression_level: compressionLevel
                )
            )
        }
    }
}

extension Record {
    @usableFromInline
    func ffiRecord<T>(_ record: (FFIRecord) -> T) -> T {
        let (datetimeSecs, datetimeNsecs) = meta.date.split()

        return meta.tag.ffiBytes { tag in
            meta.location.file.ffiBytes { file in
                meta.location.function.ffiBytes { function in
                    content.ffiBytes { content in
                        record(
                            .init(
                                level: meta.level.rawValue,
                                datetime_secs: datetimeSecs,
                                datetime_nsecs: datetimeNsecs,
                                tag: tag,
                                file: file,
                                func: function,
                                line: meta.location.line ?? UInt32.max,
                                thread_id: meta.threadId ?? UInt64.max,
                                content: content
                            )
                        )
                    }
                }
            }
        }
    }
}

protocol FFIBytesContainer {
    func ffiBytes<R>(_ call: (FFIBytes) throws -> R) rethrows -> R
}

extension Optional where Wrapped: FFIBytesContainer {
    func ffiBytes<R>(_ call: (FFIBytes) throws -> R) rethrows -> R {
        switch self {
        case let .some(container):
            return try container.ffiBytes(call)
        case .none:
            return try call(pinenut_bytes_null())
        }
    }
}

extension String: FFIBytesContainer {
    @inlinable
    init?(ffiBytes: FFIBytesBuf) {
        guard let ptr = ffiBytes.ptr else {
            // TODO: Validate
            return nil
        }
        let utf8 = ptr.bindMemory(to: CChar.self, capacity: Int(ffiBytes.len))
        self.init(validatingUTF8: utf8)
    }

    @inlinable
    func ffiBytes<R>(_ call: (FFIBytes) throws -> R) rethrows -> R {
        var str = self
        return try str.withUTF8 {
            let bytes = FFIBytes(ptr: UnsafeRawPointer($0.baseAddress), len: UInt64($0.count))
            return try call(bytes)
        }
    }
}

extension Data: FFIBytesContainer {
    @inlinable
    func ffiBytes<R>(_ call: (FFIBytes) throws -> R) rethrows -> R {
        try withUnsafeBytes {
            let bytes = FFIBytes(ptr: $0.baseAddress, len: UInt64($0.count))
            return try call(bytes)
        }
    }
}

extension Date {
    @inlinable
    func split() -> (secs: Int64, nsecs: UInt32) {
        let (intPart, fractPart) = modf(timeIntervalSince1970)
        return (Int64(intPart), UInt32(1_000_000_000 * fractPart))
    }
}
