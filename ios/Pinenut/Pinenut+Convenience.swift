//
//  Pinenut+Convenience.swift
//  Pinenut
//
//  Created by Tangent on 2023/11/19.
//

import Foundation

public extension Logger {
    /// Logs `error` record.
    @inlinable
    func error(
        tag: String? = nil, _ content: String,
        file: String = #file, function: String = #function, line: UInt32 = #line
    ) {
        log(.error, tag: tag, content, file: file, function: function, line: line)
    }

    /// Logs `warn` record.
    @inlinable
    func warn(
        tag: String? = nil, _ content: String,
        file: String = #file, function: String = #function, line: UInt32 = #line
    ) {
        log(.warn, tag: tag, content, file: file, function: function, line: line)
    }

    /// Logs `info` record.
    @inlinable
    func info(
        tag: String? = nil, _ content: String,
        file: String = #file, function: String = #function, line: UInt32 = #line
    ) {
        log(.info, tag: tag, content, file: file, function: function, line: line)
    }

    /// Logs `debug` record.
    @inlinable
    func debug(
        tag: String? = nil, _ content: String,
        file: String = #file, function: String = #function, line: UInt32 = #line
    ) {
        log(.debug, tag: tag, content, file: file, function: function, line: line)
    }

    /// Logs `verbose` record.
    @inlinable
    func verbose(
        tag: String? = nil, _ content: String,
        file: String = #file, function: String = #function, line: UInt32 = #line
    ) {
        log(.verbose, tag: tag, content, file: file, function: function, line: line)
    }

    /// The convenient log method.
    @inlinable
    func log(
        _ level: Level, tag: String? = nil, _ content: String,
        file: String = #file, function: String = #function, line: UInt32 = #line
    ) {
        let date = Date()
        var threadId: UInt64 = 0
        pthread_threadid_np(pthread_self(), &threadId)

        let location = Location(file: file, function: function, line: line)
        let meta = Meta(level: level, date: date, location: location, tag: tag, threadId: threadId)
        let record = Record(meta: meta, content: content)

        log(record)
    }
}
