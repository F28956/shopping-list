import Foundation

/// The shape a collector expects, and nothing else.
///
/// OTLP/HTTP with a JSON body — the same protocol the protobuf transport speaks, in the
/// encoding the specification defines for it, which every collector accepts on the same
/// endpoint. JSON rather than protobuf because it needs no code generation and no
/// runtime, and because the whole payload is a few kilobytes a minute: the wire saving
/// from protobuf is real and is not worth a build step here.
///
/// The one trap this file exists to get right is that **protobuf's JSON mapping writes
/// 64-bit integers as strings**. A collector handed `"timeUnixNano": 1712345678000000000`
/// as a number rejects the whole report, and the error it returns says nothing useful.
/// Everything 64-bit below is a string for that reason.
enum OTLP {
    /// Delta: each report covers the window since the last one and stands alone. See
    /// ``Metrics/flush()``.
    private static let delta = 1

    static func body(
        counters: [Metrics.Instrument: Int],
        gauges: [Metrics.Instrument: Int],
        histograms: [Metrics.Instrument: Metrics.Histogram],
        from: Date,
        to: Date
    ) -> [String: Any] {
        let start = nanoseconds(from)
        let now = nanoseconds(to)

        var metrics: [[String: Any]] = []

        // Grouped by name, because OTLP is one entry per metric with its series inside
        // it. Sent one-per-series a collector accepts it and then reports "duplicate
        // metric name" in its own log, which is a failure nobody here would see.
        for (name, series) in Dictionary(grouping: counters.keys, by: \.name) {
            metrics.append([
                "name": name,
                "sum": [
                    "aggregationTemporality": delta,
                    "isMonotonic": true,
                    "dataPoints": series.map { key in
                        point(key, start: start, now: now, extra: ["asInt": String(counters[key] ?? 0)])
                    },
                ],
            ])
        }

        for (name, series) in Dictionary(grouping: gauges.keys, by: \.name) {
            metrics.append([
                "name": name,
                "gauge": [
                    "dataPoints": series.map { key in
                        point(key, start: start, now: now, extra: ["asInt": String(gauges[key] ?? 0)])
                    },
                ],
            ])
        }

        for (name, series) in Dictionary(grouping: histograms.keys, by: \.name) {
            metrics.append([
                "name": name,
                "unit": "ms",
                "histogram": [
                    "aggregationTemporality": delta,
                    "dataPoints": series.map { key -> [String: Any] in
                        let found = histograms[key] ?? Metrics.Histogram()
                        return point(key, start: start, now: now, extra: [
                            "count": String(found.count),
                            "sum": found.sum,
                            "bucketCounts": found.buckets.map(String.init),
                            "explicitBounds": Metrics.bounds,
                        ])
                    },
                ],
            ])
        }

        return [
            "resourceMetrics": [[
                "resource": ["attributes": resource],
                "scopeMetrics": [[
                    "scope": ["name": "shopping-list-app", "version": version],
                    "metrics": metrics,
                ]],
            ]],
        ]
    }

    private static func point(
        _ key: Metrics.Instrument,
        start: String,
        now: String,
        extra: [String: Any]
    ) -> [String: Any] {
        var point: [String: Any] = [
            "startTimeUnixNano": start,
            "timeUnixNano": now,
            "attributes": key.attributes.map {
                ["key": $0.name, "value": ["stringValue": $0.value]]
            },
        ]
        for (name, value) in extra { point[name] = value }
        return point
    }

    /// What is said about the thing reporting.
    ///
    /// Four facts, and every one of them is about an install rather than a person: which
    /// app, which version, which platform, and a uuid minted on this device so two
    /// devices' series can be told apart. No device name — a Mac's is usually somebody's
    /// first name — no model, no locale, no address.
    private static var resource: [[String: Any]] {
        [
            ["key": "service.name", "value": ["stringValue": "shopping-list-app"]],
            ["key": "service.version", "value": ["stringValue": version]],
            ["key": "service.instance.id", "value": ["stringValue": MetricsSettings.instance]],
            ["key": "app.platform", "value": ["stringValue": LogFile.thisDevice]],
        ]
    }

    private static var version: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0"
    }

    private static func nanoseconds(_ date: Date) -> String {
        String(UInt64(max(0, date.timeIntervalSince1970) * 1_000_000_000))
    }
}

