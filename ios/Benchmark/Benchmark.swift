//
//  Benchmark.swift
//  Pinenut
//
//  Created by Tangent on 2023/11/21.
//

@testable import Pinenut
import XCTest

final class Benchmark: XCTestCase {
    
    func testPinenut() {
        let rate = benchmark(times: 3) {
            let documents = NSSearchPathForDirectoriesInDomains(.documentDirectory, .userDomainMask, true).first!
            let directory = (documents as NSString).appendingPathComponent("Benckmark \($0)")
            try? FileManager.default.removeItem(atPath: directory)
            try? FileManager.default.createDirectory(atPath: directory, withIntermediateDirectories: false)
            
            let domain = Domain(identifier: "Benchmark", directory: directory)
            return domain.logger(config: .init(key: "AsoIniA0+QQjI7xnj74ILlpDAkJDYwWWV80t5kdMYfW/"))
        } log: { logger, logIndex in
            logger.info(tag: "Measure", "Hello World! \(logIndex)", file: "abc.swift")
        } finish: { logger in
            logger.flush()
        }
        
        print("Final Rate: \(rate)")
    }
    
    func benchmark<L>(
        times: Int,
        initialize: (_ benchmarkIndex: Int) -> L,
        log: (_ logger: L, _ logIndex: Int) -> Void,
        finish: (_ logger: L) -> Void
    ) -> Double {
        var rates: [Double] = []
        
        for i in (0..<times) {
            let logger = initialize(i)
            let rate = benchmarkUnit { log(logger, $0) }
            print("Rate: \(rate)")
            rates.append(rate)
            finish(logger)
            
            Thread.sleep(forTimeInterval: 3)
        }
        
        guard rates.count > 0 else { return 0 }
        return rates.reduce(0, +) / Double(rates.count)
    }

    func benchmarkUnit(log: (Int) -> Void) -> Double {
        var rates: [Double] = []
        
        let durations: [TimeInterval] = [2, 5, 10];
        for duration in durations {
            let count = measure(startTime: CFAbsoluteTimeGetCurrent(), duration: duration, log: log)
            let rate = Double(count) / duration
            print("Duration: \(duration), Count: \(count), Rate: \(rate)")
            rates.append(rate)
            
            Thread.sleep(forTimeInterval: 3)
        }
        
        guard rates.count > 0 else { return 0 }
        return rates.reduce(0, +) / Double(rates.count)
    }


    /// Returns the number of records logged during the specified duration.
    func measure(startTime: TimeInterval, duration: TimeInterval, log: (Int) -> Void) -> Int {
        var count = 0
        
        let startTime = CFAbsoluteTimeGetCurrent()
        while CFAbsoluteTimeGetCurrent() - startTime < duration {
            log(count)
            count += 1
        }
        
        return count
    }
}
