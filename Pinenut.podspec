Pod::Spec.new do |spec|
  spec.name         = "Pinenut"
  spec.version      = "0.0.1"
  spec.summary      = "An extremely high-performance logging system for clients (iOS, Android, Desktop), written in Rust."

  spec.homepage     = "https://github.com/TangentW/Pinenut"
  spec.license      = { :type => "MIT", :file => "LICENSE" }
  spec.author       = { "Tangent" => "tangent_w@outlook.com" }

  spec.platform     = :ios, "13.0"
  spec.source       = { :git => "https://github.com/TangentW/Pinenut.git", :tag => "#{spec.version}" }
  spec.swift_version = '4.0'

  spec.source_files = "ios/Pinenut/*.swift"
  spec.vendored_frameworks = "ios/PinenutFFI.xcframework"
end

